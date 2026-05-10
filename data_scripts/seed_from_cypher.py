import re
import json
import requests
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct, VectorParams, Distance
import os

# Configuration
CYPHER_FILES = [
    "../data/bodhi/bodhi_s.cypher",
    "../data/bodhi/bodhi_m.cypher"
]
QDRANT_URL = "http://localhost:6333"
EMBEDDING_URL = "http://localhost:8000/embed"
COLLECTION_NAME = "bodhi_global_knowledge"

client = QdrantClient(url=QDRANT_URL)

def parse_props(prop_str):
    """
    Parses a Cypher property string into a dict.
    Example: {snomed_id: '123', name: 'Test', synonyms: "['A', 'B']"}
    """
    # This is a bit tricky because keys aren't quoted and values might contain commas.
    # We'll use a regex that looks for key: value pairs.
    props = {}
    
    # Clean brackets
    prop_str = prop_str.strip("{} ")
    
    # Regex to find key: value
    # key: any alpha-numeric plus underscores
    # value: either a quoted string '...' or a number
    matches = re.finditer(r"(\w+):\s*('(?:[^'\\]|\\.)*'|[\d\.]+)", prop_str)
    
    for match in matches:
        key = match.group(1)
        val = match.group(2)
        if val.startswith("'") and val.endswith("'"):
            val = val[1:-1].replace("\\'", "'")
        elif "." in val:
            val = float(val)
        else:
            try:
                val = int(val)
            except:
                pass
        props[key] = val
        
    return props

def generate_fact(label, props):
    """Generates a natural language fact from node properties."""
    name = props.get("name") or props.get("display_name") or "Unknown"
    snomed_id = props.get("snomed_id", "Unknown")
    
    if label == "Condition":
        triage = props.get("triage_level", "unknown severity")
        type_cond = props.get("type_condition", "condition")
        fact = f"Condition: {name} (SNOMED: {snomed_id}). Triage Level: {triage}. Type: {type_cond}."
        
        # Add likelihood context if available
        if "overall_likelihood" in props:
            fact += f" Overall Likelihood: {props['overall_likelihood']}."
            
        return fact
    
    elif label == "Concept" or label == "Drug":
        synonyms = props.get("synonyms", "[]")
        # Try to parse synonyms if they look like a list
        if synonyms.startswith("[") and synonyms.endswith("]"):
            try:
                # Cypher exports synonyms as "['A', 'B']" which is not valid JSON (single quotes)
                # We'll do a simple cleanup
                syn_list = [s.strip(" '") for s in synonyms[1:-1].split(",")]
                syn_list = [s for s in syn_list if s and s != name]
                if syn_list:
                    return f"Clinical Concept: {name} (SNOMED: {snomed_id}) is also known as: {', '.join(syn_list[:10])}."
            except:
                pass
        return f"Clinical Concept: {name} (SNOMED: {snomed_id})."
    
    return f"{label}: {name} (SNOMED: {snomed_id})."

def get_embeddings(texts):
    """Fetch embeddings in batch from the sidecar."""
    response = requests.post(EMBEDDING_URL, json={"texts": texts})
    response.raise_for_status()
    return response.json()["embeddings"]

def main():
    print(f"Ensuring collection '{COLLECTION_NAME}' exists...")
    # Get dimension from sidecar
    try:
        dummy_embs = get_embeddings(["test"])
        dim = len(dummy_embs[0])
        print(f"Detected embedding dimension: {dim}")
    except Exception as e:
        print(f"Error connecting to embedding sidecar: {e}")
        return

    client.recreate_collection(
        collection_name=COLLECTION_NAME,
        vectors_config=VectorParams(size=dim, distance=Distance.COSINE),
    )

    facts = []
    
    # Regex to find MERGE (n:Label {props})
    node_pattern = re.compile(r"MERGE \(n:(\w+)\s+({.*?})\);")

    print("Parsing Cypher files...")
    for file_path in CYPHER_FILES:
        if not os.path.exists(file_path):
            print(f"Skipping missing file: {file_path}")
            continue
            
        print(f"Processing {file_path}...")
        with open(file_path, "r", encoding="utf-8") as f:
            for line in f:
                match = node_pattern.search(line)
                if match:
                    label = match.group(1)
                    prop_str = match.group(2)
                    props = parse_props(prop_str)
                    fact_text = generate_fact(label, props)
                    facts.append(fact_text)

    print(f"Generated {len(facts)} clinical facts. Starting ingestion...")
    
    batch_size = 50
    for i in range(0, len(facts), batch_size):
        batch_texts = facts[i:i+batch_size]
        points = []
        
        try:
            batch_embs = get_embeddings(batch_texts)
            for idx, (text, emb) in enumerate(zip(batch_texts, batch_embs)):
                points.append(PointStruct(
                    id=i + idx,
                    vector=emb,
                    payload={"fact": text}
                ))
        except Exception as e:
            print(f"Error embedding batch starting at {i}: {e}")
            continue
        
        if points:
            client.upsert(collection_name=COLLECTION_NAME, points=points)
            print(f"Upserted batch {i//batch_size + 1}/{(len(facts)-1)//batch_size + 1}")

    print("Ingestion complete.")

if __name__ == "__main__":
    main()
