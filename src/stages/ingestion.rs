use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::pipeline::{PipelineError, PipelineStage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPayload {
    pub file_name: String,
    pub mime_type: String,
    pub raw_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestedDocument {
    pub file_name: Option<String>,
    pub text_content: String,
    pub source_format: String,
    pub extraction_method: String,
    pub metadata: serde_json::Value,
}

pub struct IngestionStage;

impl IngestionStage {
    pub fn new() -> Self {
        Self
    }

    fn flatten_json(value: &serde_json::Value, prefix: String, acc: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    let new_prefix = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                    Self::flatten_json(v, new_prefix, acc);
                }
            }
            serde_json::Value::Array(arr) => {
                for (i, v) in arr.iter().enumerate() {
                    Self::flatten_json(v, format!("{}[{}]", prefix, i), acc);
                }
            }
            serde_json::Value::String(s) => acc.push(format!("{}: {}", prefix, s)),
            serde_json::Value::Number(n) => acc.push(format!("{}: {}", prefix, n)),
            serde_json::Value::Bool(b) => acc.push(format!("{}: {}", prefix, b)),
            serde_json::Value::Null => acc.push(format!("{}: null", prefix)),
        }
    }
}

#[async_trait]
impl PipelineStage<RawPayload, IngestedDocument> for IngestionStage {
    fn name(&self) -> &'static str {
        "Stage1_Ingestion"
    }

    async fn execute(&self, input: RawPayload) -> Result<IngestedDocument, PipelineError> {
        match input.mime_type.as_str() {
            "application/json" => {
                let parsed: serde_json::Value = serde_json::from_slice(&input.raw_bytes)
                    .map_err(|e| PipelineError::Ingestion(format!("Invalid JSON: {}", e)))?;
                
                let mut flattened_pairs = Vec::new();
                Self::flatten_json(&parsed, String::new(), &mut flattened_pairs);
                
                Ok(IngestedDocument {
                    file_name: Some(input.file_name.clone()),
                    text_content: flattened_pairs.join("\n"),
                    source_format: "json".to_string(),
                    extraction_method: "flatten".to_string(),
                    metadata: serde_json::json!({ "keys_extracted": flattened_pairs.len() }),
                })
            }
            "application/pdf" => {
                // PDF extraction placeholder. Replaces main.rs branching.
                let text = String::from_utf8_lossy(&input.raw_bytes).into_owned();
                Ok(IngestedDocument {
                    file_name: None,
                    text_content: text,
                    source_format: "pdf".to_string(),
                    extraction_method: "native_fallback_ocr".to_string(),
                    metadata: serde_json::json!({ "pages": 1 }),
                })
            }
            "text/plain" => {
                let text = String::from_utf8(input.raw_bytes)
                    .map_err(|_| PipelineError::Ingestion("Invalid UTF-8 in text file".to_string()))?;
                Ok(IngestedDocument {
                    file_name: None,
                    text_content: text,
                    source_format: "txt".to_string(),
                    extraction_method: "raw".to_string(),
                    metadata: serde_json::json!({}),
                })
            }
            _ => Err(PipelineError::Ingestion(format!("Unsupported mime type: {}", input.mime_type))),
        }
    }
}
