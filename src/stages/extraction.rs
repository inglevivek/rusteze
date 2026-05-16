use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use rig::providers::openai::Client as OpenAiClient;
use crate::pipeline::{PipelineError, PipelineStage};
use super::ingestion::IngestedDocument;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseFacts {
    pub diagnoses: Vec<String>,
    pub medications: Vec<String>,
    pub procedures: Vec<String>,
    pub labs: Vec<String>,
    pub claim_questions: Vec<String>,
}

pub struct ExtractionStage {
    llm_client: Arc<OpenAiClient>,
}

impl ExtractionStage {
    pub fn new(llm_client: Arc<OpenAiClient>) -> Self {
        Self { llm_client }
    }
}

#[async_trait]
impl PipelineStage<IngestedDocument, CaseFacts> for ExtractionStage {
    fn name(&self) -> &'static str {
        "Stage2_Extraction"
    }

    async fn execute(&self, input: IngestedDocument) -> Result<CaseFacts, PipelineError> {
        let _system_prompt = "You are a clinical data extractor. Extract exact terms from the document. Do not normalize or resolve them to medical standards. Return strictly in the required JSON schema.";
        
        let _user_prompt = format!(
            "Extract entities from this document:\n\n---\n{}\n---",
            input.text_content
        );

        // Uses the injected LLM client
        // let response = self.llm_client.agent("gpt-4o")...
        
        Ok(CaseFacts {
            diagnoses: vec![],
            medications: vec![],
            procedures: vec![],
            labs: vec![],
            claim_questions: vec![],
        })
    }
}
