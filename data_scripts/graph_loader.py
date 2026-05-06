import psycopg2
from neo4j import GraphDatabase

# Configuration
PG_CONFIG = {
    "dbname": "nrces_dict",
    "user": "d3admin",
    "password": "graphbench2026",
    "host": "localhost",
    "port": "5432"
}

NEO4J_URI = "bolt://localhost:7687"
NEO4J_AUTH = ("neo4j", "graphbench2026")

BATCH_SIZE = 10000

def chunker(seq, size):
    return (seq[pos:pos + size] for pos in range(0, len(seq), size))

def create_constraints(session):
    print("Creating Constraints...")
    constraints = [
        "CREATE CONSTRAINT supplier_id IF NOT EXISTS FOR (s:Supplier) REQUIRE s.identifier IS UNIQUE",
        "CREATE CONSTRAINT substance_id IF NOT EXISTS FOR (s:Substance) REQUIRE s.identifier IS UNIQUE",
        "CREATE CONSTRAINT generic_id IF NOT EXISTS FOR (g:Generic) REQUIRE g.identifier IS UNIQUE",
        "CREATE CONSTRAINT product_id IF NOT EXISTS FOR (p:Product) REQUIRE p.identifier IS UNIQUE",
        "CREATE CONSTRAINT route_id IF NOT EXISTS FOR (r:Route) REQUIRE r.identifier IS UNIQUE",
        "CREATE CONSTRAINT doseform_id IF NOT EXISTS FOR (d:DoseForm) REQUIRE d.identifier IS UNIQUE",
    ]
    for q in constraints:
        session.run(q)

def load_nodes(pg_cur, session):
    print("Loading Route nodes...")
    pg_cur.execute("SELECT identifier, route_name FROM routes WHERE identifier IS NOT NULL")
    routes = [{"id": row[0], "name": row[1]} for row in pg_cur.fetchall()]
    for batch in chunker(routes, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MERGE (n:Route {identifier: row.id})
            SET n.name = row.name
        """, batch=batch)

    print("Loading DoseForm nodes...")
    pg_cur.execute("SELECT identifier, form_name FROM dose_forms WHERE identifier IS NOT NULL")
    forms = [{"id": row[0], "name": row[1]} for row in pg_cur.fetchall()]
    for batch in chunker(forms, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MERGE (n:DoseForm {identifier: row.id})
            SET n.name = row.name
        """, batch=batch)

    print("Loading Supplier nodes...")
    pg_cur.execute("SELECT identifier, supplier_name, country FROM suppliers WHERE identifier IS NOT NULL")
    suppliers = [{"id": row[0], "name": row[1], "country": row[2]} for row in pg_cur.fetchall()]
    for batch in chunker(suppliers, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MERGE (n:Supplier {identifier: row.id})
            SET n.name = row.name, n.country = row.country
        """, batch=batch)

    print("Loading Substance nodes...")
    pg_cur.execute("SELECT identifier, substance_name, unii, cas_number FROM substances WHERE identifier IS NOT NULL")
    substances = [{"id": row[0], "name": row[1], "unii": row[2], "cas": row[3]} for row in pg_cur.fetchall()]
    for batch in chunker(substances, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MERGE (n:Substance {identifier: row.id})
            SET n.name = row.name, n.unii = row.unii, n.cas_number = row.cas
        """, batch=batch)

    print("Loading Generic nodes...")
    pg_cur.execute("SELECT identifier, generic_name, therapeutic_role, indication FROM generics WHERE identifier IS NOT NULL")
    generics = [{"id": row[0], "name": row[1], "role": row[2], "ind": row[3]} for row in pg_cur.fetchall()]
    for batch in chunker(generics, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MERGE (n:Generic {identifier: row.id})
            SET n.name = row.name, n.therapeutic_role = row.role, n.indication = row.ind
        """, batch=batch)

    print("Loading Product nodes...")
    pg_cur.execute("SELECT identifier, brand_name, license_status FROM products WHERE identifier IS NOT NULL")
    products = [{"id": row[0], "name": row[1], "status": row[2]} for row in pg_cur.fetchall()]
    for batch in chunker(products, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MERGE (n:Product {identifier: row.id})
            SET n.brand_name = row.name, n.license_status = row.status
        """, batch=batch)


def load_relationships(pg_cur, session):
    print("Loading Relationships...")

    # Product -> Supplier
    print("  (Product)-[:MANUFACTURED_BY]->(Supplier)")
    pg_cur.execute("SELECT identifier, supplier_identifier FROM products WHERE identifier IS NOT NULL AND supplier_identifier IS NOT NULL")
    batch_data = [{"pid": row[0], "sid": row[1]} for row in pg_cur.fetchall()]
    for batch in chunker(batch_data, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MATCH (p:Product {identifier: row.pid})
            MATCH (s:Supplier {identifier: row.sid})
            MERGE (p)-[:MANUFACTURED_BY]->(s)
        """, batch=batch)

    # Product -> Generic
    print("  (Product)-[:IS_A]->(Generic)")
    pg_cur.execute("SELECT identifier, generic_identifier FROM products WHERE identifier IS NOT NULL AND generic_identifier IS NOT NULL")
    batch_data = [{"pid": row[0], "gid": row[1]} for row in pg_cur.fetchall()]
    for batch in chunker(batch_data, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MATCH (p:Product {identifier: row.pid})
            MATCH (g:Generic {identifier: row.gid})
            MERGE (p)-[:IS_A]->(g)
        """, batch=batch)

    # Generic -> Substance
    print("  (Generic)-[:CONTAINS_SUBSTANCE]->(Substance)")
    # Split delimited identifiers if multiple exist, e.g., combination drugs "ID1+ID2"
    pg_cur.execute("SELECT identifier, substance_identifier FROM generics WHERE identifier IS NOT NULL AND substance_identifier IS NOT NULL")
    rel_list = []
    for row in pg_cur.fetchall():
        gid = row[0]
        substances = str(row[1]).split('+')
        for sub in substances:
            rel_list.append({"gid": gid, "sid": sub.strip()})
            
    for batch in chunker(rel_list, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MATCH (g:Generic {identifier: row.gid})
            MATCH (s:Substance {identifier: row.sid})
            MERGE (g)-[:CONTAINS_SUBSTANCE]->(s)
        """, batch=batch)

    # Generic -> Route
    print("  (Generic)-[:ADMINISTERED_VIA]->(Route)")
    pg_cur.execute("SELECT identifier, route_identifier FROM generics WHERE identifier IS NOT NULL AND route_identifier IS NOT NULL")
    batch_data = [{"gid": row[0], "rid": row[1]} for row in pg_cur.fetchall()]
    for batch in chunker(batch_data, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MATCH (g:Generic {identifier: row.gid})
            MATCH (r:Route {identifier: row.rid})
            MERGE (g)-[:ADMINISTERED_VIA]->(r)
        """, batch=batch)

    # Generic -> DoseForm
    print("  (Generic)-[:DELIVERED_AS]->(DoseForm)")
    pg_cur.execute("SELECT identifier, form_identifier FROM generics WHERE identifier IS NOT NULL AND form_identifier IS NOT NULL")
    batch_data = [{"gid": row[0], "fid": row[1]} for row in pg_cur.fetchall()]
    for batch in chunker(batch_data, BATCH_SIZE):
        session.run("""
            UNWIND $batch AS row
            MATCH (g:Generic {identifier: row.gid})
            MATCH (d:DoseForm {identifier: row.fid})
            MERGE (g)-[:DELIVERED_AS]->(d)
        """, batch=batch)

def main():
    print("Connecting to Postgres...")
    pg_conn = psycopg2.connect(**PG_CONFIG)
    pg_cur = pg_conn.cursor()

    print("Connecting to Neo4j...")
    neo4j_driver = GraphDatabase.driver(NEO4J_URI, auth=NEO4J_AUTH)

    try:
        with neo4j_driver.session() as session:
            create_constraints(session)
            load_nodes(pg_cur, session)
            load_relationships(pg_cur, session)
            print("Graph synchronization complete!")
    finally:
        pg_cur.close()
        pg_conn.close()
        neo4j_driver.close()

if __name__ == "__main__":
    main()
