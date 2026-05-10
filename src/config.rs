// In src/config.rs
use std::env;

#[derive(Clone)]
pub struct Config {
    pub neo4j_uri: String,
    pub neo4j_user: String,
    pub neo4j_pass: String,
    pub qdrant_url: String,
    pub ocr_service_url: String,
    pub groq_api_key: String,
    pub pg_url: String,
    pub embedding_vector_size: usize,
    pub embedding_url: String,
}

impl Config {
    pub fn load() -> Self {
        dotenvy::dotenv().ok();

        Self {
            neo4j_uri: env::var("NEO4J_URI").expect("NEO4J_URI must be set"),
            neo4j_user: env::var("NEO4J_USER").expect("NEO4J_USER must be set"),
            neo4j_pass: env::var("NEO4J_PASS").expect("NEO4J_PASS must be set"),
            qdrant_url: env::var("QDRANT_URL").expect("QDRANT_URL must be set"),
            ocr_service_url: env::var("OCR_URL").expect("OCR_URL must be set"),
            groq_api_key: env::var("GROQ_API_KEY").unwrap_or_else(|_| "placeholder".to_string()),
            pg_url: env::var("PG_URL").unwrap_or_else(|_| {
                "postgres://d3admin:graphbench2026@localhost:5432/nrces_dict".to_string()
            }),
            embedding_vector_size: env::var("EMBEDDING_VECTOR_SIZE")
                .unwrap_or_else(|_| "768".to_string())
                .parse()
                .unwrap_or(768),
            embedding_url: env::var("EMBED_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string()),
        }
    }
}
