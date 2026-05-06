# D3-GraphBench Project Blueprint

## 1. What is the Project?
**d3-graph-bench** is a backend system designed for clinical document processing, entity extraction, and intelligent medical adjudication. It acts as an orchestrator (a "bench") that combines multiple specialized data stores—relational dictionaries, vector databases, and graph databases—with Large Language Models (LLMs) to accurately ingest medical text (like OCR'd prescriptions or discharge summaries), resolve clinical entities, and provide a verified knowledge graph for conversational AI.

The core goal is to take messy, unstructured clinical text and turn it into highly structured, semantically verified data that adheres to established medical ontologies (like SNOMED/NRCES) and treatment guidelines.

## 2. What We Have & What Works

### Infrastructure & Databases (Working via Docker)
- **PostgreSQL**: Serving as the relational ontology lookup. It successfully stores the Indian Common Drug Codes (NRCES) flat file package, including normalized tables for `suppliers`, `routes`, `dose_forms`, `substances`, `generics`, and `products` (~100k+ records).
- **Qdrant**: Serving as the vector database for semantic search and Retrieval-Augmented Generation (RAG) context.
- **Neo4j**: Serving as the graph database to represent medical topology, such as validating "medical necessity" between a diagnosis and a prescribed drug.
- **Actix-Web Server**: The core Rust backend running the APIs.

### Backend Services & Clients (Working in Rust)
- **Postgres Client (`postgres.rs`)**: Connects to the DB via `deadpool-postgres` and safely queries the knowledge dictionary.
- **Qdrant Client (`qdrant.rs`)**: Performs semantic vector searches to retrieve similar medical cases or fact chunks.
- **Neo4j Client (`neo4j.rs`)**: Queries the graph to check paths between nodes (e.g., Diagnosis -> Indicates -> Medication).
- **Groq/LLM Client (`groq.rs`)**: Communicates with the Groq API for ultra-fast, structured extraction of clinical entities from raw text.
- **OCR Integration (`ocr.rs`)**: Interfaces with an external/sidecar OCR service to digitize documents.

### Data Scripts (Working in Python)
- **`db_builder.py`**: A robust, automated ingestion script that cleans, normalizes, and bulk-loads the messy NRCES flat files into the PostgreSQL schema, handling misaligned TSV columns and skipping bad lines.

## 3. Logical Pipeline & Data Flow

The system operates primarily in two modes: **Ingestion/Adjudication** and **Chat/Retrieval**.

### Stage 1: Evidence Ingestion & Adjudication Pipeline (`/api/ingest`)
1. **Input**: Raw clinical text (or an OCR'd image) is submitted to the API.
2. **Extraction (LLM)**: The system prompts the Groq LLM to extract key medical entities (diagnoses, medications, procedures) in a structured JSON format.
3. **Dictionary Resolution (Postgres)**: For each extracted medication/entity, the pipeline queries the PostgreSQL database (the NRCES dictionary) to find the exact canonical Concept ID, ensuring standardization.
4. **Topology Validation (Neo4j)**: The system queries Neo4j to verify the logical relationship between the extracted entities. For example, it checks if the extracted diagnosis medically justifies the prescribed medication according to the graph guidelines.
5. **Context Assembly (Qdrant)**: Relevant historical context or similar medical protocols are fetched from Qdrant.
6. **Final Adjudication (LLM)**: All data (standardized IDs, graph validation results, vector context) is passed back to the LLM to generate a final, adjudicated clinical report.

### Stage 2: Conversational Chat (`/api/chat`)
1. **Input**: A user queries a specific case ID.
2. **Vector Retrieval**: The system pulls the top 5 most relevant semantic chunks from Qdrant associated with that case.
3. **Prompt Construction**: A strict system prompt is built, injecting the RAG context and explicitly instructing the LLM not to hallucinate.
4. **Response**: The Groq LLM generates an answer grounded entirely in the patient's verified medical record.

## 4. Workflows

- **Data Bootstrap Workflow**: Developers run `db_builder.py` (and potentially other scripts like `ingest_qdrant.py` / `ingest_neo4j.py`) to hydrate the local databases with baseline medical ontologies.
- **Clinical Ingestion Workflow**: A clinic uploads a patient record. The Rust backend orchestrates the multi-database validation and produces a structured, SNOMED-linked summary of the patient's state.
- **Provider Query Workflow**: A doctor asks "What was the patient's last prescribed dosage for Safedox?" The chat endpoint retrieves the specific facts from the vector DB and answers accurately.

## 5. Features & Specifications

- **Multi-Modal Verification**: Doesn't just trust the LLM. It cross-references LLM outputs against a relational dictionary (Postgres) and a logical graph (Neo4j).
- **High-Performance Concurrency**: Built in Rust with Actix-Web and Tokio, utilizing connection pools (`deadpool_postgres`) to handle heavy parallel requests efficiently.
- **Robust Error Handling**: The data ingestion layer automatically sanitizes mismatched TSV formats, drops duplicates gracefully, and ignores stray quotes in medical text to ensure database integrity.
- **API Endpoints**: 
  - `POST /api/ingest`
  - `POST /api/chat`
- **Ontology Support**: Built to natively handle NRCES (National Resource Centre for EHR Standards) flat file structures for the Indian healthcare context.

## 6. Performance Characteristics

- **Postgres Bulk Loading**: Optimized with `COPY FROM STDIN` via memory buffers, allowing 100,000+ complex relational records to be ingested in under 10 seconds.
- **Inference Speed**: Utilizes Groq for near-instantaneous LLM inference, overcoming the traditional latency bottleneck of multi-stage LLM chains.
- **Connection Management**: Uses pooling for Neo4j and Postgres, preventing connection exhaustion during high-throughput ingestion spikes.
