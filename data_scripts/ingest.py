import json, uuid, requests
from qdrant_client import QdrantClient
from qdrant_client.models import Distance, VectorParams, PointStruct

QDRANT_URL   = "http://localhost:6334"
EMBED_URL    = "http://localhost:11434/api/embeddings"
COLLECTION   = "bodhi_global_knowledge"
MODEL        = "qwen2.5:3b" # Use the same model as NER for consistency, or specify your embedding model

def embed(text: str) -> list[float]:
    resp = requests.post(EMBED_URL, json={"model": MODEL, "prompt": text})
    resp.raise_for_status()
    return resp.json()["embedding"]

def main():
    client = QdrantClient(url=QDRANT_URL)
    existing = [c.name for c in client.get_collections().collections]
    
    # Check embedding dimension
    test_vec = embed("test")
    dim = len(test_vec)
    
    if COLLECTION not in existing:
        client.create_collection(COLLECTION, vectors_config=VectorParams(size=dim, distance=Distance.COSINE))
        print(f"Created '{COLLECTION}' (dim={dim})")
    else:
        # Recreate if dimension mismatch
        col_info = client.get_collection(COLLECTION)
        if col_info.config.params.vectors.size != dim:
            print(f"Dimension mismatch (expected {dim}, got {col_info.config.params.vectors.size}). Recreating...")
            client.delete_collection(COLLECTION)
            client.create_collection(COLLECTION, vectors_config=VectorParams(size=dim, distance=Distance.COSINE))

    points = []
    with open("seed_facts.jsonl") as f:
        for line in f:
            if not line.strip(): continue
            rec = json.loads(line)
            vec = embed(rec["fact"])
            points.append(PointStruct(
                id=str(uuid.uuid4()),
                vector=vec,
                payload={"fact": rec["fact"], "source_id": rec["id"]}
            ))

    client.upsert(collection_name=COLLECTION, points=points)
    count = client.count(COLLECTION).count
    print(f"Done. {COLLECTION} now has {count} points.")

if __name__ == "__main__":
    main()
