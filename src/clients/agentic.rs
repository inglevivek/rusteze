use async_trait::async_trait;
use serde_json::Value;
use std::error::Error;

#[async_trait]
pub trait AgentClient: Send + Sync {
    async fn extract_entities(&self, text: &str) -> Result<Value, Box<dyn Error + Send + Sync>>;
    async fn chat_with_context(
        &self,
        system_prompt: &str,
        user_query: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>>;
    async fn normalize_term(&self, term: &str) -> Result<String, Box<dyn Error + Send + Sync>>;
    async fn normalize_terms(
        &self,
        terms: &[String],
    ) -> Result<std::collections::HashMap<String, String>, Box<dyn Error + Send + Sync>>;
    async fn generate_adjudication_report(
        &self,
        context: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>>;
}
