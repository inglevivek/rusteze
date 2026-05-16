use crate::clients::agentic::AgentClient;
use crate::stages::extraction::CaseFacts;
use crate::stages::ingestion::IngestedDocument;
use std::sync::Arc;

pub async fn extract_case_facts(
    _llm: &Arc<dyn AgentClient>,
    _doc: &IngestedDocument,
) -> Result<CaseFacts, String> {
    // This is a shim for legacy code in main.rs.
    // In the new architecture, use ExtractionStage.
    
    // Return a dummy CaseFacts to allow compilation.
    Ok(CaseFacts {
        diagnoses: Vec::new(),
        medications: Vec::new(),
        procedures: Vec::new(),
        labs: Vec::new(),
        claim_questions: Vec::new(),
    })
}
