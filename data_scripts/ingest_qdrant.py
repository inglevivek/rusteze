import json
import os
import uuid
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct, VectorParams, Distance
from fastembed import TextEmbedding
from tqdm import tqdm

QDRANT_URL = "http://localhost:6333"
COLLECTION_NAME = "bodhi_global_knowledge"
FACTS_FILES = [
    "../data/BODHI-M/data/nl_facts.jsonl",
    "../data/BODHI-S/data/nl_facts.jsonl"
]

print("Initializing Qdrant...")
client = QdrantClient(url=QDRANT_URL, timeout=20.0)

print("Nuking existing Qdrant Collection...")
# THE FIX: Cleanly check and delete to avoid the Deprecation Warning
if client.collection_exists(collection_name=COLLECTION_NAME):
    client.delete_collection(collection_name=COLLECTION_NAME)

client.create_collection(
    collection_name=COLLECTION_NAME,
    vectors_config=VectorParams(size=384, distance=Distance.COSINE),
)

print("Booting FastEmbed on 24 CPU Threads...")
model = TextEmbedding(model_name="BAAI/bge-small-en-v1.5", threads=24)

def ingest_facts(file_path):
    if not os.path.exists(file_path):
        print(f"Skipping {file_path} - Not found.")
        return

    print(f"\n--- Hardware-Accelerated Processing: {file_path} ---")
    
    # Fast pass to get total count for the progress bar
    total_lines = sum(1 for _ in open(file_path, 'r', encoding='utf-8') if _.strip())
    
    batch_texts = []
    count = 0

    with open(file_path, 'r', encoding='utf-8') as f:
        # THE FIX: Hook the progress bar directly to the payload logic
        with tqdm(total=total_lines, desc="Vectorizing & Embedding", unit="fact", bar_format="{l_bar}{bar:40}{r_bar}") as pbar:
            for line in f:
                if not line.strip(): continue
                data = json.loads(line)
                
                if 'text' in data:
                    batch_texts.append(data['text'])
                
                if len(batch_texts) >= 500:
                    embeddings = list(model.embed(batch_texts))
                    points = [
                        PointStruct(
                            id=str(uuid.uuid4()),
                            vector=emb.tolist(),
                            payload={"fact": text}
                        )
                        for text, emb in zip(batch_texts, embeddings)
                    ]
                    client.upsert(collection_name=COLLECTION_NAME, points=points)
                    count += len(batch_texts)
                    pbar.update(len(batch_texts))
                    batch_texts = []

            if batch_texts:
                embeddings = list(model.embed(batch_texts))
                points = [
                    PointStruct(
                        id=str(uuid.uuid4()),
                        vector=emb.tolist(),
                        payload={"fact": text}
                    )
                    for text, emb in zip(batch_texts, embeddings)
                ]
                client.upsert(collection_name=COLLECTION_NAME, points=points)
                count += len(batch_texts)
                pbar.update(len(batch_texts))

    print(f"Finished {file_path}. Total embedded: {count}")

if __name__ == "__main__":
    for file in FACTS_FILES:
        ingest_facts(file)
    
    print("\n[Qdrant] Hardware-Accelerated Vectorization Complete.")