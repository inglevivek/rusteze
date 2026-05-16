use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("Ingestion failed: {0}")]
    Ingestion(String),
    
    #[error("Extraction failed: {0}")]
    Extraction(String),
    
    #[error("Resolution tool failure: {0}")]
    Resolution(String),
    
    #[error("Query validation failed - AST rejection: {0}")]
    Validation(String),
    
    #[error("Graph execution timeout")]
    GraphTimeout,
    
    #[error("Internal pipeline error: {0}")]
    Internal(String),
}
