# D3-GraphBench: How It Works

The D3-GraphBench pipeline is a **Dual-Brain GraphRAG** system that uses three distinct data layers (PostgreSQL, Neo4j, and Qdrant) to verify and adjudicate clinical data.

---

## The Core Pipeline Workflow

### 1. The Entry Point: LLM Extraction
The pipeline first uses **Groq (Llama-3)** to scan the raw clinical text and extract "strings" of interest, such as medications and diagnoses. These are raw, messy strings (e.g., "aspirin 80mg").

### 2. The Linker: PostgreSQL Lookup
Before any graph or vector search can happen, the system must standardize these strings.
*   **Postgres Role**: Acting as the **Relational Dictionary**.
*   **The Flow**: The extracted strings are sent to the `linker` service. It performs a case-insensitive `ILIKE` search in the Postgres `dictionary` table to resolve the messy text into a canonical **Concept ID** (Standardized ID).
*   **Fallback**: If Postgres misses, a "Nano-Agent" (Ollama) normalizes the term and retries the Postgres lookup to ensure a hit.

### 3. The Logic Layer: Neo4j (Deterministic Topology)
Once we have the Standardized IDs from Postgres, we need to know if they make sense together.
*   **Neo4j Role**: Acting as the **Deterministic Brain**.
*   **The Flow**: The pipeline takes the list of resolved IDs and queries Neo4j to find direct pathways between them (e.g., `(Diagnosis)-[:INDICATES]->(Medication)`).
*   **The Output**: This provides "Hard Truth" links—logical proof that the entities are medically related according to the graph's ontology.

### 4. The Context Layer: Qdrant (Semantic Truth)
While Neo4j provides the logic, Qdrant provides the broader medical knowledge.
*   **Qdrant Role**: Acting as the **Semantic Brain**.
*   **The Flow**: The pipeline performs a global vector search using the names of the resolved entities. It retrieves the top 10 most relevant medical fact chunks from the vector database.
*   **The Output**: This provides unstructured "Soft Truth"—research papers, guidelines, or historical context that supports the adjudication.

### 5. Final Adjudication: Synthesis
Finally, the pipeline bundles everything together into a "Mega-Prompt":
1.  **Raw Evidence** (OCR Text)
2.  **Topological Proof** (from Neo4j)
3.  **Semantic Context** (from Qdrant)

The **Groq LLM** then synthesizes these three layers to produce a final, verified JSON report (e.g., `APPROVED` or `REJECTED`).

---

## Summary of Responsibilities

| Database | Function | Data Type | Flow Priority |
| :--- | :--- | :--- | :--- |
| **PostgreSQL** | Entity Resolution | Relational Dictionary | **High** (Resolves IDs) |
| **Neo4j** | Logical Validation | Knowledge Graph | **Medium** (Verifies Links) |
| **Qdrant** | Context Retrieval | Vector Store | **Medium** (Retrieves Facts) |
