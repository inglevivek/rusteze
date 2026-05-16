use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use rig::providers::openai::Client as OpenAiClient;
use crate::pipeline::{PipelineError, PipelineStage};
use crate::resolver::models::ResolvedCaseFacts;
use super::retrieval::EvidencePacket;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportInput {
    pub facts: ResolvedCaseFacts,
    pub evidence: EvidencePacket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalReport {
    pub adjudication_prose: String,
}

pub struct ReportGenerationStage {
    llm_client: Arc<OpenAiClient>,
}

impl ReportGenerationStage {
    pub fn new(llm_client: Arc<OpenAiClient>) -> Self {
        Self { llm_client }
    }
}

#[async_trait]
impl PipelineStage<ReportInput, FinalReport> for ReportGenerationStage {
    fn name(&self) -> &'static str {
        "Stage6_ReportGeneration"
    }

    async fn execute(&self, input: ReportInput) -> Result<FinalReport, PipelineError> {
        let _system_prompt = "You are a clinical adjudication reporter. Your explicit and only task is to synthesize the provided facts and retrieved graph evidence into a coherent prose report. DO NOT invent new medical facts. DO NOT attempt to resolve terminology. Write prose only based strictly on the JSON evidence packet provided.";
        
        let _user_prompt = format!(
            "Facts:\n{}\n\nEvidence:\n{}",
            serde_json::to_string_pretty(&input.facts).unwrap_or_default(),
            serde_json::to_string_pretty(&input.evidence).unwrap_or_default()
        );

        // Scaffolded response using the injected client
        // let response = self.llm_client.agent("gpt-4o").preamble(system_prompt).prompt(&user_prompt).await...

        Ok(FinalReport {
            adjudication_prose: "Simulated Final Clinical Adjudication Report...".to_string(),
        })
    }
}
