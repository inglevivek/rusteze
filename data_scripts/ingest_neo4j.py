import json
import os
import concurrent.futures
from neo4j import GraphDatabase
from tqdm import tqdm

URI = "bolt://localhost:7687"
AUTH = ("neo4j", "graphbench2026")

TRIPLES_FILES = [
    "../data/BODHI-M/data/triples.jsonl",
    "../data/BODHI-S/data/triples.jsonl"
]

def flush_graph(driver):
    print("WARNING: Nuking existing Neo4j Graph Data...")
    with driver.session() as session:
        session.run("MATCH (n) CALL { WITH n DETACH DELETE n } IN TRANSACTIONS OF 10000 ROWS")
    print("Graph Flushed. Ready for fresh ingestion.")

def ingest_batch(tx, batch):
    query = """
    UNWIND $batch AS record
    MERGE (h:ClinicalEntity {id: record.head})
    MERGE (t:ClinicalEntity {id: record.tail})
    MERGE (h)-[r:CLINICAL_RELATION]->(t)
    SET r.type = record.relation
    """
    tx.run(query, batch=batch)

def process_batch(driver, batch):
    with driver.session() as session:
        session.execute_write(ingest_batch, batch)

def parallel_ingest(driver, file_path, max_workers=14, batch_size=5000):
    if not os.path.exists(file_path):
        print(f"Skipping {file_path} - Not found.")
        return

    print(f"\n--- Multi-Threaded Processing: {file_path} ---")
    
    futures = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as executor:
        with open(file_path, 'r', encoding='utf-8') as f:
            batch = []
            count = 0
            for line in f:
                if not line.strip(): continue
                
                data = json.loads(line)
                clean_record = {
                    'head': data.get('head', data.get('subject', data.get('source'))),
                    'relation': data.get('relation', data.get('predicate', data.get('type'))),
                    'tail': data.get('tail', data.get('object', data.get('target')))
                }
                
                batch.append(clean_record)
                count += 1
                
                if len(batch) >= batch_size:
                    futures.append(executor.submit(process_batch, driver, batch))
                    batch = []
            
            if batch:
                futures.append(executor.submit(process_batch, driver, batch))

        # THE FIX: Wrap the completion loop in a dynamic progress bar
        with tqdm(total=len(futures), desc="Writing to Neo4j", unit="batch", bar_format="{l_bar}{bar:40}{r_bar}") as pbar:
            for _ in concurrent.futures.as_completed(futures):
                pbar.update(1)

    print(f"Finished {file_path}. Total Triples: {count}")

if __name__ == "__main__":
    print("Connecting to Neo4j...")
    driver = GraphDatabase.driver(URI, auth=AUTH)
    
    flush_graph(driver)
    
    for file in TRIPLES_FILES:
        parallel_ingest(driver, file)
        
    driver.close()
    print("\n[Neo4j] Hardware-Accelerated Graph Ingestion Complete.")