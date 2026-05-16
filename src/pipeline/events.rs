use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineEvent {
    pub trace_id: String,
    pub case_id: String,
    pub stage: String,
    pub payload_in: serde_json::Value,
    pub payload_out: serde_json::Value,
    pub duration_ms: u64,
    pub tool_calls: Option<Vec<ToolCallRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub request: serde_json::Value,
    pub response: serde_json::Value,
    pub confidence: f32,
}
