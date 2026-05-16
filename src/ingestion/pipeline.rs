use crate::AppState;
use crate::stages::ingestion::IngestedDocument;

pub async fn ingest_document(
    _state: &AppState,
    file_name: String,
    _file_bytes: Vec<u8>,
) -> Result<IngestedDocument, String> {
    // This is a shim for legacy code in main.rs.
    // In the new architecture, use IngestionStage.
    
    // Return a dummy IngestedDocument to allow compilation.
    Ok(IngestedDocument {
        file_name: Some(file_name.clone()),
        text_content: "Shim text".to_string(),
        source_format: "shim".to_string(),
        extraction_method: "shim".to_string(),
        metadata: serde_json::json!({ "file_name": file_name }),
    })
}
