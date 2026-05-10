import psycopg2
import pandas as pd
import io
import os
import sys

# ─── Configuration ───────────────────────────────────────────────────────────
DB_CONFIG = {
    "dbname":   "nrces_dict",
    "user":     "d3admin",
    "password": "graphbench2026",
    "host":     "localhost",
    "port":     "5432",
}

DATA_DIR = "../data/CommonDrugCodesForIndia_FlatFilePackage/CommonDrugCodesForIndia_FlatFilePackage"

# ─── DDL ─────────────────────────────────────────────────────────────────────
# Drop everything first (cases table is in a separate schema/app — never touched here)
FLUSH_SQL = """
DROP TABLE IF EXISTS dictionary CASCADE;
DROP TABLE IF EXISTS cases      CASCADE;
"""

SCHEMA_SQL = """
-- ── cases (app table — owned by the Rust service) ──────────────────────────
CREATE TABLE IF NOT EXISTS cases (
    case_id              TEXT        PRIMARY KEY,
    document_text        TEXT        NOT NULL,
    adjudication_report  JSONB,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── dictionary (flat denormalized drug lookup) ───────────────────────────────
-- One row per brand entry; all foreign data is pre-joined so the Rust
-- linker never needs to do a JOIN at query time.
CREATE TABLE IF NOT EXISTS dictionary (
    -- identity
    concept_id           TEXT        PRIMARY KEY,   -- brand SNOMED ID (linker resolves to this)
    snomed_id            TEXT,                      -- alias column read by search_dictionary()
    term_type            TEXT        NOT NULL DEFAULT 'brand',
    name                 TEXT        NOT NULL,       -- brand name  → full-text search target

    -- generic drug
    generic_concept_id   TEXT,                      -- GenericMaster.Identifier
    generic_name         TEXT,                      -- e.g. "amoxicillin 500 mg oral tablet"
    therapeutic_role     TEXT,
    indication           TEXT,
    contra_indication    TEXT,
    interaction_with_drugs TEXT,
    classification       TEXT,

    -- active substance (from SubstanceMaster via GenericMaster.SubstanceIdentifier)
    substance_id         TEXT,
    substance_name       TEXT,
    cas_number           TEXT,
    unii                 TEXT,
    molecular_formula    TEXT,
    substance_description TEXT,
    toxicity             TEXT,

    -- route + dose form (resolved from lookup masters)
    route_id             TEXT,
    route_name           TEXT,
    dose_form_id         TEXT,
    dose_form_name       TEXT,

    -- supplier / manufacturer
    supplier_id          TEXT,
    supplier_name        TEXT,
    supplier_country     TEXT,

    -- product form identifier (from ProductMaster via BrandMaster.ProductIdentifier)
    product_id           TEXT,
    product_name         TEXT,

    -- brand metadata
    license_number       TEXT,
    license_status       TEXT,
    excipient            TEXT,
    brand_updated_on     DATE
);

CREATE INDEX IF NOT EXISTS idx_dictionary_name       ON dictionary USING gin(to_tsvector('english', name));
CREATE INDEX IF NOT EXISTS idx_dictionary_generic    ON dictionary (generic_concept_id);
CREATE INDEX IF NOT EXISTS idx_dictionary_substance  ON dictionary (substance_id);
"""

# ─── Helpers ─────────────────────────────────────────────────────────────────

GARBAGE_VALUES = {"Not Available", "NA", "N/A", "null", "", "NAN", "nan", "UNKNOWN", "unknown"}

def read_tsv(path: str) -> pd.DataFrame:
    """Read a TSV master file; normalise column names."""
    df = pd.read_csv(
        path,
        sep="\t",
        dtype=str,
        quoting=3,          # QUOTE_NONE — medical text has stray quotes
        on_bad_lines="skip",
        index_col=False,
    )
    df.columns = [c.strip().lower().replace(" ", "_").replace("/", "_") for c in df.columns]
    df.replace(list(GARBAGE_VALUES) + [None], pd.NA, inplace=True)
    return df


def bulk_copy(conn, df: pd.DataFrame, table: str, columns: list[str]):
    """COPY a DataFrame into Postgres using STDIN; fast and NULL-safe."""
    valid = [c for c in columns if c in df.columns]
    chunk = df[valid].copy()
    buf = io.StringIO()
    chunk.to_csv(buf, index=False, header=False, sep="\t", na_rep="\\N")
    buf.seek(0)
    cols_str = ",".join(valid)
    sql = f"COPY {table} ({cols_str}) FROM STDIN WITH (FORMAT CSV, DELIMITER E'\\t', NULL '\\N')"
    with conn.cursor() as cur:
        cur.copy_expert(sql, buf)
    conn.commit()
    print(f"  OK {table:20s}  {len(chunk):>7,} rows")


# ─── Load each master into a plain dict keyed by identifier ──────────────────

def load_all_masters(data_dir: str) -> dict:
    masters = {}

    # RouteOfAdministrationMaster  →  Identifier | RouteOfAdministration
    path = os.path.join(data_dir, "RouteOfAdministrationMaster.txt")
    df = read_tsv(path)
    df.rename(columns={"routeofadministration": "route_name"}, inplace=True)
    masters["routes"] = df.set_index("identifier")[["route_name"]]
    print(f"  loaded routes        {len(df):>7,} rows")

    # DrugFormMaster  →  Identifier | Dose Form
    path = os.path.join(data_dir, "DrugFormMaster.txt")
    df = read_tsv(path)
    df.rename(columns={"dose_form": "dose_form_name"}, inplace=True)
    masters["forms"] = df.set_index("identifier")[["dose_form_name"]]
    print(f"  loaded dose_forms    {len(df):>7,} rows")

    # SupplierMaster  →  Identifier | Supplier Name | Country
    path = os.path.join(data_dir, "SupplierMaster.txt")
    df = read_tsv(path)
    df.rename(columns={"supplier_name": "supplier_name", "country": "supplier_country"}, inplace=True)
    # normalise the "Supplier Name" header variant
    if "supplier_name" not in df.columns:
        name_col = [c for c in df.columns if "name" in c]
        if name_col:
            df.rename(columns={name_col[0]: "supplier_name"}, inplace=True)
    masters["suppliers"] = df.set_index("identifier")[["supplier_name", "supplier_country"]]
    print(f"  loaded suppliers     {len(df):>7,} rows")

    # SubstanceMaster  →  Identifier | Substance Name | CAS Number | UNII |
    #                      Substance Description | Molecular Weight | Toxicity |
    #                      SMILE | InChI | IUPAC Name | Molecular Formula | last_updated_on
    path = os.path.join(data_dir, "SubstanceMaster.txt")
    df = read_tsv(path)
    df.rename(columns={
        "substance_name":        "substance_name",
        "cas_number":            "cas_number",
        "unii":                  "unii",
        "substance_description": "substance_description",
        "toxicity":              "toxicity",
        "molecular_formula":     "molecular_formula",
    }, inplace=True)
    masters["substances"] = df.set_index("identifier")
    print(f"  loaded substances    {len(df):>7,} rows")

    # GenericMaster  →  Identifier | Generic Name | Substance Identifier |
    #                    Route of Administration | Dose Form | Therapeutic Role |
    #                    Indication | Contra Indication | Interaction with Drugs |
    #                    Classification of Drugs | Source | Regulatory | lastupdatedon
    path = os.path.join(data_dir, "GenericMaster.txt")
    df = read_tsv(path)
    df.rename(columns={
        "generic_name":              "generic_name",
        "substance_identifier":      "substance_id",
        "route_of_administration":   "route_id",
        "dose_form":                 "dose_form_id",
        "therapeutic_role":          "therapeutic_role",
        "indication":                "indication",
        "contra_indication":         "contra_indication",
        "interaction_with_drugs":    "interaction_with_drugs",
        "classification_of_drugs":   "classification",
        "lastupdatedon":             "last_updated_on",
    }, inplace=True)
    masters["generics"] = df.set_index("identifier")
    print(f"  loaded generics      {len(df):>7,} rows")

    # ProductMaster  →  Identifier | Product Name
    path = os.path.join(data_dir, "ProductMaster.txt")
    df = read_tsv(path)
    df.rename(columns={"product_name": "product_name"}, inplace=True)
    if "product_name" not in df.columns:
        name_col = [c for c in df.columns if "name" in c]
        if name_col:
            df.rename(columns={name_col[0]: "product_name"}, inplace=True)
    masters["products"] = df.set_index("identifier")[["product_name"]]
    print(f"  loaded products      {len(df):>7,} rows")

    return masters


# ─── Build the flat dictionary table ─────────────────────────────────────────

def build_dictionary(data_dir: str, masters: dict) -> pd.DataFrame:
    """
    BrandMaster columns (actual):
      Identifier | Brand Name | Product Identifier | Supplier Identifier |
      Generic Identifier | License Number | License Status | Excipient | lastupdatedon
    """
    path = os.path.join(data_dir, "BrandMaster.txt")
    brands = read_tsv(path)
    brands.rename(columns={
        "identifier":          "concept_id",
        "brand_name":          "name",
        "product_identifier":  "product_id",
        "supplier_identifier": "supplier_id",
        "generic_identifier":  "generic_concept_id",
        "license_number":      "license_number",
        "license_status":      "license_status",
        "excipient":           "excipient",
        "last_updated_on":     "brand_updated_on",
    }, inplace=True)

    brands.drop_duplicates(subset=["concept_id"], keep="first", inplace=True)
    brands["snomed_id"]  = brands["concept_id"]
    brands["term_type"]  = "brand"

    print(f"  loaded brands        {len(brands):>7,} rows")

    # ── join generics ───────────────────────────────────────────────
    gen = masters["generics"].reset_index().rename(columns={"identifier": "generic_concept_id"})
    brands = brands.merge(gen[["generic_concept_id","generic_name","substance_id",
                                "route_id","dose_form_id","therapeutic_role","indication",
                                "contra_indication","interaction_with_drugs","classification"]],
                          on="generic_concept_id", how="left")

    # ── join substances ─────────────────────────────────────────────
    sub = masters["substances"].reset_index().rename(columns={"identifier": "substance_id"})
    brands = brands.merge(sub[["substance_id","substance_name","cas_number","unii",
                                "molecular_formula","substance_description","toxicity"]],
                          on="substance_id", how="left")

    # ── join routes ─────────────────────────────────────────────────
    routes = masters["routes"].reset_index().rename(columns={"identifier": "route_id"})
    brands = brands.merge(routes, on="route_id", how="left")

    # ── join dose forms ─────────────────────────────────────────────
    forms = masters["forms"].reset_index().rename(columns={"identifier": "dose_form_id"})
    brands = brands.merge(forms, on="dose_form_id", how="left")

    # ── join suppliers ──────────────────────────────────────────────
    sup = masters["suppliers"].reset_index().rename(columns={"identifier": "supplier_id"})
    brands = brands.merge(sup, on="supplier_id", how="left")

    # ── join products ───────────────────────────────────────────────
    prod = masters["products"].reset_index().rename(columns={"identifier": "product_id"})
    brands = brands.merge(prod, on="product_id", how="left")

    # ── fix dates ───────────────────────────────────────────────────
    brands["brand_updated_on"] = pd.to_datetime(
        brands["brand_updated_on"], format="%Y%m%d", errors="coerce"
    )

    return brands


# ─── Entry point ─────────────────────────────────────────────────────────────

def main():
    print("\n+-- BODHI DB Builder ---------------------------------------+")
    conn = psycopg2.connect(**DB_CONFIG)
    try:
        print("\n[1/4] Flushing existing schema...")
        with conn.cursor() as cur:
            cur.execute(FLUSH_SQL)
        conn.commit()
        print("  OK All old tables dropped")

        print("\n[2/4] Creating new schema...")
        with conn.cursor() as cur:
            cur.execute(SCHEMA_SQL)
        conn.commit()
        print("  OK cases + dictionary tables created")

        print("\n[3/4] Loading master files...")
        masters = load_all_masters(DATA_DIR)

        print("\n[4/4] Building flat dictionary and bulk-loading...")
        df = build_dictionary(DATA_DIR, masters)

        # Define the exact column order matching the SQL CREATE TABLE
        cols = [
            "concept_id", "snomed_id", "term_type", "name",
            "generic_concept_id", "generic_name", "therapeutic_role",
            "indication", "contra_indication", "interaction_with_drugs", "classification",
            "substance_id", "substance_name", "cas_number", "unii",
            "molecular_formula", "substance_description", "toxicity",
            "route_id", "route_name",
            "dose_form_id", "dose_form_name",
            "supplier_id", "supplier_name", "supplier_country",
            "product_id", "product_name",
            "license_number", "license_status", "excipient", "brand_updated_on",
        ]

        bulk_copy(conn, df, "dictionary", cols)

        with conn.cursor() as cur:
            cur.execute("SELECT COUNT(*) FROM dictionary")
            count = cur.fetchone()[0]
        print(f"\n+-- Final row count: {count:,} rows in dictionary")

        print("+-- Done. ---------------------------------------------------+\n")

    except Exception as e:
        conn.rollback()
        print(f"\n✗ FATAL: {e}", file=sys.stderr)
        raise
    finally:
        conn.close()


if __name__ == "__main__":
    main()
