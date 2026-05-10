"""
BioLORD Embedding Sidecar
Loads the model ONCE at startup and serves embedding requests over HTTP.
"""

import os
import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import List
import uvicorn

# ── Paths (relative to this file; sidecar lives inside d3-graph-bench/) ──────
BASE_DIR = os.path.dirname(os.path.abspath(__file__))
MODEL_PATH = os.path.join(BASE_DIR, "..", "models", "biolord-onnx", "model.onnx")
TOKENIZER_PATH = os.path.join(BASE_DIR, "..", "models", "biolord-onnx", "tokenizer.json")

app = FastAPI(title="BioLORD Embedding Sidecar", version="1.0.0")

# ── Load model once at startup ─────────────────────────────────────────────────
print(f"[EmbedSidecar] Loading tokenizer from {TOKENIZER_PATH} ...")
tokenizer = Tokenizer.from_file(TOKENIZER_PATH)
tokenizer.enable_padding(direction="right", pad_id=0, pad_token="[PAD]")
tokenizer.enable_truncation(max_length=512)
print("[EmbedSidecar] Tokenizer loaded.")

print(f"[EmbedSidecar] Loading ONNX model from {MODEL_PATH} ...")
sess_options = ort.SessionOptions()
sess_options.intra_op_num_threads = 4
sess_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
session = ort.InferenceSession(MODEL_PATH, sess_options=sess_options, providers=["CPUExecutionProvider"])
print("[EmbedSidecar] ONNX model loaded. Ready to serve.")


# ── Request / Response schemas ─────────────────────────────────────────────────
class EmbedRequest(BaseModel):
    texts: List[str]


class EmbedResponse(BaseModel):
    embeddings: List[List[float]]


# ── Helpers ─────────────────────────────────────────────────────────────────────
def mean_pool_and_normalize(last_hidden_state: np.ndarray, attention_mask: np.ndarray) -> np.ndarray:
    """Mean-pool token embeddings then L2-normalize."""
    mask_expanded = attention_mask[:, :, np.newaxis].astype(np.float32)
    sum_embeddings = (last_hidden_state * mask_expanded).sum(axis=1)
    sum_mask = mask_expanded.sum(axis=1).clip(min=1e-9)
    mean_pooled = sum_embeddings / sum_mask
    # L2 normalise
    norms = np.linalg.norm(mean_pooled, axis=1, keepdims=True).clip(min=1e-12)
    return mean_pooled / norms


# ── Endpoints ───────────────────────────────────────────────────────────────────
@app.get("/health")
def health():
    return {"status": "ok"}


@app.post("/embed", response_model=EmbedResponse)
def embed(request: EmbedRequest):
    if not request.texts:
        return EmbedResponse(embeddings=[])

    try:
        encodings = tokenizer.encode_batch(request.texts)

        input_ids = np.array([enc.ids for enc in encodings], dtype=np.int64)
        attention_mask = np.array([enc.attention_mask for enc in encodings], dtype=np.int64)

        outputs = session.run(
            None,
            {
                "input_ids": input_ids,
                "attention_mask": attention_mask,
            },
        )

        # outputs[0] is last_hidden_state: (batch, seq_len, hidden)
        last_hidden_state = outputs[0]
        embeddings = mean_pool_and_normalize(last_hidden_state, attention_mask)

        return EmbedResponse(embeddings=embeddings.tolist())

    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


if __name__ == "__main__":
    port = int(os.environ.get("EMBED_PORT", 8000))
    uvicorn.run(app, host="0.0.0.0", port=port, log_level="info")
