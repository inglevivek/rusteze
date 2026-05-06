use crate::clients::agentic::AgentClient;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::error::Error;

pub struct OllamaClient {
    pub base_url: String,
    pub model: String,
}

#[async_trait]
impl AgentClient for OllamaClient {
    async fn extract_entities(&self, text: &str) -> Result<Value, Box<dyn Error + Send + Sync>> {
        let client = Client::new();
        let prompt = format!(
            "Extract all medications/drugs and diagnoses from the following text. Output strictly as JSON in this format: {{\"medications\": [\"drug1\"], \"diagnoses\": [\"diag1\"]}}. No other text.\n\nText: {}",
            text
        );

        let res = client.post(&format!("{}/api/generate", self.base_url))
            .json(&json!({
                "model": &self.model,
                "prompt": prompt,
                "stream": false,
                "format": "json"
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        if let Some(response) = json_body["response"].as_str() {
            return Ok(serde_json::from_str(response)?);
        }
        Err("Ollama extraction failed".into())
    }

    async fn chat_with_context(
        &self,
        system_prompt: &str,
        user_query: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let client = Client::new();
        let prompt = format!("{}\n\nUser: {}", system_prompt, user_query);
        let res = client.post(&format!("{}/api/generate", self.base_url))
            .json(&json!({
                "model": &self.model,
                "prompt": prompt,
                "stream": false
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        if let Some(response) = json_body["response"].as_str() {
            return Ok(response.to_string());
        }
        Err("Ollama chat failed".into())
    }

    async fn normalize_term(&self, term: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        let client = Client::new();
        let prompt = format!(
            "You are a strict clinical terminologist. Convert the following extracted OCR text into its base generic medication or standard clinical diagnosis name. Output ONLY the generic name, absolutely nothing else. Text: '{}'",
            term
        );

        let res = client.post(&format!("{}/api/generate", self.base_url))
            .json(&json!({
                "model": &self.model,
                "prompt": prompt,
                "stream": false
            }))
            .send()
            .await?;

        if res.status().is_success() {
            let json_body: Value = res.json().await?;
            if let Some(response) = json_body["response"].as_str() {
                return Ok(response.trim().to_string());
            }
        }
        Err("Local Ollama Agent failed to respond properly".into())
    }

    async fn normalize_terms(
        &self,
        terms: &[String],
    ) -> Result<std::collections::HashMap<String, String>, Box<dyn Error + Send + Sync>> {
        if terms.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let client = Client::new();
        let terms_json = serde_json::to_string(terms)?;
        let prompt = format!(
            "You are a strict clinical terminologist. Given the following JSON array of extracted OCR terms, convert each term into its base generic medication or standard clinical diagnosis name. \
            Output ONLY a JSON object where the keys are the EXACT original terms provided, and the values are the normalized generic names. If a term is invalid or cannot be normalized, map it to an empty string.\n\nTerms: {}",
            terms_json
        );

        let res = client.post(&format!("{}/api/generate", self.base_url))
            .json(&json!({
                "model": &self.model,
                "prompt": prompt,
                "stream": false,
                "format": "json"
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        if let Some(response) = json_body["response"].as_str() {
            let map: std::collections::HashMap<String, String> = serde_json::from_str(response)?;
            return Ok(map);
        }
        Err("Ollama bulk normalization failed".into())
    }

    async fn generate_adjudication_report(
        &self,
        context: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let client = Client::new();
        let prompt = format!(
            "You are a strict, clinical AI adjudicator evaluating medical claims. \
            You must synthesize the raw document text with the exact topological relationships and semantic medical truth provided below.\n\n\
            {}\n\n\
            Respond ONLY with a valid JSON object containing: \
            {{\"decision\": \"APPROVED|REJECTED|MANUAL_REVIEW\", \"confidence\": 0.0-1.0, \"reasoning\": \"Detailed synthesis...\", \"entities_evaluated\": [\"...\"]}}",
            context
        );

        let res = client.post(&format!("{}/api/generate", self.base_url))
            .json(&json!({
                "model": &self.model,
                "prompt": prompt,
                "stream": false,
                "format": "json"
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        if let Some(response) = json_body["response"].as_str() {
            return Ok(response.to_string());
        }
        Err("Ollama adjudication failed".into())
    }
}