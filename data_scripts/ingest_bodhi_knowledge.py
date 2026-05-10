"""
Ingest global medical knowledge into Qdrant `bodhi_global_knowledge` collection.
Source: A JSONL file where each line is {"id": "...", "text": "..."}
The text should contain drug facts, disease facts, treatment protocols.

Usage: python ingest_bodhi_knowledge.py --source facts.jsonl
"""
import argparse, json, uuid
from qdrant_client import QdrantClient
from qdrant_client.models import Distance, VectorParams, PointStruct
from fastembed import TextEmbedding

COLLECTION = "bodhi_global_knowledge"
QDRANT_URL = "http://localhost:6334"
DIM = 768  # BioLORD-2023 matches our Rust setup

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, help="Path to JSONL file")
    args = parser.parse_args()

    client = QdrantClient(url=QDRANT_URL)
    # Using the same model as our sidecar for consistency
    model = TextEmbedding("FremyCompany/BioLORD-2023")

    # Create or re-use collection
    existing = [c.name for c in client.get_collections().collections]
    if COLLECTION not in existing:
        client.create_collection(
            COLLECTION,
            vectors_config=VectorParams(size=DIM, distance=Distance.COSINE)
        )
        print(f"Created collection '{COLLECTION}'")

    points = []
    with open(args.source) as f:
        for line in f:
            if not line.strip(): continue
            rec = json.loads(line)
            text = rec["text"]
            # Fastembed returns a generator, we take the first result
            vec = list(model.embed([text]))[0].tolist()
            points.append(PointStruct(
                id=str(uuid.uuid4()),
                vector=vec,
                payload={"fact": text, "source_id": rec.get("id", "")}
            ))

    # Upsert in batches of 100
    for i in range(0, len(points), 100):
        client.upsert(collection_name=COLLECTION, points=points[i:i+100])
        print(f"Upserted {min(i+100, len(points))}/{len(points)} points")

    count = client.count(COLLECTION).count
    print(f"Done. {COLLECTION} now has {count} points.")

if __name__ == "__main__":
    main()
