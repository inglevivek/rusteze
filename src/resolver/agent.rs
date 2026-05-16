use rig::providers::openai::Client as OpenAiClient;
use rig::agent::Agent;
use std::sync::Arc;
use tokio::sync::Semaphore;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use super::tools::{RxNormExactTool, Icd11Tool};

pub struct ResolverAgent {
    agent: Agent<rig::providers::openai::responses_api::GenericResponsesCompletionModel>,
    request_limiter: Arc<Semaphore>,
}

impl ResolverAgent {
    pub fn new(llm_client: Arc<OpenAiClient>) -> Self {
        let agent = llm_client.agent("gpt-4o")
            .preamble("You are a clinical terminology resolver. Use the provided tools to map raw clinical terms to canonical standard IDs (RxNorm, ICD-11). Do not invent standard IDs.")
            .tool(RxNormExactTool)
            .tool(Icd11Tool)
            .build();

        Self { 
            agent,
            // Restrict to max 10 concurrent LLM API calls globally for this agent instance
            request_limiter: Arc::new(Semaphore::new(10)),
        }
    }

    pub async fn resolve_term(&self, term: &str, category: &str) -> Result<String, String> {
        // Acquire permit before making network call; backpressure applied here
        let _permit = self.request_limiter.acquire().await
            .map_err(|e| format!("Semaphore closed: {}", e))?;

        let prompt = format!("Resolve this {}: '{}'. Output ONLY a JSON string containing standard_id, canonical_name, standard_system, and confidence.", category, term);
        self.agent.prompt(&prompt).await.map_err(|e| e.to_string())
    }
}
