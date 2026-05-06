import sqlite3
import os

DB_PATH = "../data/nrces_dict.db"

def build_database():
    print(f"Building SQLite Dictionary at {DB_PATH}...")
    
    # Ensure the data directory exists
    os.makedirs(os.path.dirname(DB_PATH), exist_ok=True)
    
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()

    # 1. Create the base table for strict entity linking
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS clinical_entities (
        concept_id TEXT PRIMARY KEY,
        term_type TEXT, -- e.g., 'Brand', 'Generic', 'Symptom'
        name TEXT,
        generic_concept_id TEXT -- Maps a brand directly to its generic equivalent
    );
    """)

    # 2. Create the FTS5 Virtual Table for fast, fuzzy-like searching
    cursor.execute("""
    CREATE VIRTUAL TABLE IF NOT EXISTS entity_search USING fts5(
        name,
        concept_id UNINDEXED,
        term_type UNINDEXED
    );
    """)

    # 3. Insert baseline testing data
    # (In production, you will read the NRCeS CSV via pandas here)
    sample_data = [
        ("C001", "Generic", "Amoxicillin and Clavulanate Potassium", "C001"),
        ("B001", "Brand", "Augmentin 625", "C001"),
        ("C002", "Generic", "Paracetamol", "C002"),
        ("B002", "Brand", "Calpol 500", "C002"),
        ("S001", "Symptom", "Fever", "S001"),
        ("S002", "Symptom", "Bacterial Infection", "S002")
    ]

    cursor.executemany("""
    INSERT OR IGNORE INTO clinical_entities (concept_id, term_type, name, generic_concept_id)
    VALUES (?, ?, ?, ?)
    """, sample_data)

    # Sync the FTS table
    cursor.execute("DELETE FROM entity_search;")
    cursor.execute("""
    INSERT INTO entity_search (name, concept_id, term_type)
    SELECT name, concept_id, term_type FROM clinical_entities;
    """)

    conn.commit()
    conn.close()
    print("Database built and seeded with FTS5 search enabled.")

if __name__ == "__main__":
    build_database()