CREATE TABLE IF NOT EXISTS dictionary (
    concept_id VARCHAR(255) PRIMARY KEY,
    term_type VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    generic_concept_id VARCHAR(255)
);

-- We use pg_trgm for fast case-insensitive LIKE matches
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX idx_dictionary_name_trgm ON dictionary USING gin (name gin_trgm_ops);

CREATE TABLE IF NOT EXISTS cases (
    case_id VARCHAR(255) PRIMARY KEY,
    document_text TEXT,
    adjudication_report JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);