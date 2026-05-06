import psycopg2
import pandas as pd
import io
import os

# 1. Database Configuration
DB_CONFIG = {
    "dbname": "nrces_dict",
    "user": "d3admin",
    "password": "graphbench2026",
    "host": "localhost",
    "port": "5432"
}

# 2. Data Directory
DATA_DIR = "../data/CommonDrugCodesForIndia_FlatFilePackage/CommonDrugCodesForIndia_FlatFilePackage"

# 2. Schema Definition
SCHEMA_SQL = """
DROP TABLE IF EXISTS products CASCADE;
DROP TABLE IF EXISTS generics CASCADE;
DROP TABLE IF EXISTS substances CASCADE;
DROP TABLE IF EXISTS dose_forms CASCADE;
DROP TABLE IF EXISTS routes CASCADE;
DROP TABLE IF EXISTS suppliers CASCADE;

CREATE TABLE IF NOT EXISTS suppliers (
    identifier TEXT PRIMARY KEY,
    supplier_name TEXT,
    country TEXT
);

CREATE TABLE IF NOT EXISTS routes (
    identifier TEXT PRIMARY KEY,
    route_name TEXT
);

CREATE TABLE IF NOT EXISTS dose_forms (
    identifier TEXT PRIMARY KEY,
    form_name TEXT
);

CREATE TABLE IF NOT EXISTS substances (
    identifier TEXT PRIMARY KEY,
    substance_name TEXT,
    cas_number TEXT,
    unii TEXT,
    molecular_formula TEXT,
    last_updated_on DATE
);

CREATE TABLE IF NOT EXISTS generics (
    identifier TEXT PRIMARY KEY,
    generic_name TEXT,
    substance_identifier TEXT,
    route_identifier TEXT,
    form_identifier TEXT,
    therapeutic_role TEXT,
    indication TEXT,
    last_updated_on DATE
);

CREATE TABLE IF NOT EXISTS products (
    identifier TEXT PRIMARY KEY,
    brand_name TEXT,
    generic_identifier TEXT,
    supplier_identifier TEXT,
    license_status TEXT,
    last_updated_on DATE
);
"""

def setup_database(conn):
    """Executes the DDL to ensure tables exist."""
    with conn.cursor() as cur:
        cur.execute(SCHEMA_SQL)
    conn.commit()
    print("Schema initialized.")

def clean_and_load(conn, file_path, table_name, columns):
    """
    Reads TSV, cleans garbage values, fixes dates, and bulk loads via COPY.
    """
    if not os.path.exists(file_path):
        print(f"SKIPPING: File not found at {file_path}")
        return

    print(f"Processing {file_path} into {table_name}...")
    
    # Read TSV as strings to prevent Pandas from mangling identifiers
    # quoting=3 (csv.QUOTE_NONE) helps with stray quotes in medical text
    # index_col=False prevents Pandas from using the first column as an index if there are trailing tabs
    df = pd.read_csv(file_path, sep='\t', dtype=str, quoting=3, on_bad_lines='skip', index_col=False)

    # Normalize column names: lowercase and replace spaces with underscores
    df.columns = [c.lower().replace(' ', '_').replace('/', '_') for c in df.columns]

    # Bridge the gap between file headers and our schema
    renames = {
        "route_of_administration": "route_identifier",
        "dose_form": "form_identifier",
        "license_status": "license_status"
    }
    df.rename(columns=renames, inplace=True)

    # Clean the raw strings
    df.replace(["Not Available", "NA", "N/A", "null", "", "NAN", "nan"], None, inplace=True)

    # Standardize dates if the column exists in the file
    if 'last_updated_on' in df.columns:
        df['last_updated_on'] = pd.to_datetime(
            df['last_updated_on'], 
            format='%Y%m%d', 
            errors='coerce' # Bad dates become NaT (mapped to NULL in SQL)
        )

    # Drop duplicates to satisfy PRIMARY KEY constraints
    if 'identifier' in df.columns:
        df.drop_duplicates(subset=['identifier'], keep='first', inplace=True)

    # Filter and reorder to match the exact SQL columns required
    # Some columns might not exist in all files, so we take the intersection
    valid_cols = [c for c in columns if c in df.columns]
    df = df[valid_cols]

    # Dump to in-memory CSV buffer for Postgres COPY
    buffer = io.StringIO()
    # na_rep='\N' is Postgres's default string representation for NULL
    df.to_csv(buffer, index=False, header=False, sep='\t', na_rep='\\N')
    buffer.seek(0)

    # Bulk insert
    with conn.cursor() as cur:
        try:
            sql = f"COPY {table_name} ({','.join(valid_cols)}) FROM STDIN WITH (FORMAT CSV, DELIMITER '\t', NULL '\\N')"
            cur.copy_expert(sql, buffer)
            conn.commit()
            print(f"SUCCESS: Loaded {len(df)} rows into {table_name}")
        except Exception as e:
            conn.rollback()
            print(f"FAILED loading {table_name}: {e}")

if __name__ == "__main__":
    conn = psycopg2.connect(**DB_CONFIG)
    
    try:
        setup_database(conn)

        # Mapping of File Path -> (Table Name, [Expected Columns in File])
        # IMPORTANT: The columns list must match both your TSV headers AND the SQL table columns.
        load_plan = [
            (os.path.join(DATA_DIR, "SupplierMaster.txt"), "suppliers", ["identifier", "supplier_name", "country"]),
            (os.path.join(DATA_DIR, "RouteOfAdministrationMaster.txt"), "routes", ["identifier", "route_name"]),
            (os.path.join(DATA_DIR, "DrugFormMaster.txt"), "dose_forms", ["identifier", "form_name"]),
            (os.path.join(DATA_DIR, "SubstanceMaster.txt"), "substances", ["identifier", "substance_name", "cas_number", "unii", "molecular_formula", "last_updated_on"]),
            (os.path.join(DATA_DIR, "GenericMaster.txt"), "generics", ["identifier", "generic_name", "substance_identifier", "route_identifier", "form_identifier", "therapeutic_role", "indication", "last_updated_on"]),
            (os.path.join(DATA_DIR, "BrandMaster.txt"), "products", ["identifier", "brand_name", "generic_identifier", "supplier_identifier", "license_status", "last_updated_on"])
        ]

        # Order matters here due to Foreign Key constraints. 
        # Master lookup tables must be populated before generic/product tables.
        for file_path, table_name, columns in load_plan:
            clean_and_load(conn, file_path, table_name, columns)

    finally:
        conn.close()
        print("Database connection closed.")