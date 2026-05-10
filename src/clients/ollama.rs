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

        let system_msg = r#"You are a clinical Named Entity Recognition (NER) extractor.
Your ONLY job is to find drug/medication names and clinical diagnosis names in the provided text.
Return ONLY this exact JSON object, no markdown, no explanation:
{"medications": ["drug_name_1", ...], "diagnoses": ["diagnosis_name_1", ...]}
If none found, return: {"medications": [], "diagnoses": []}
Do NOT return any other keys or schema."#;

        let user_msg = format!("Extract clinical entities from this text:\n\n{}", text);

        tracing::info!(
            "┌── [Ollama ▶ SEND] extract_entities ────────────────────────\n│  model: {}\n│  input_len: {} chars\n└────────────────────────────────────────────────────────────",
            &self.model,
            text.len()
        );

        let res = client.post(&format!("{}/api/chat", self.base_url))
            .json(&json!({
                "model": &self.model,
                "messages": [
                    {"role": "system", "content": system_msg},
                    {"role": "user",   "content": user_msg}
                ],
                "stream": false,
                "format": "json",
                "options": {
                    "temperature": 0.0,
                    "num_ctx": 20480
                }
            }))
            .send()
            .await?;

        let json_body: Value = match res.json().await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("[Ollama] Failed to parse JSON response: {}", e);
                return Err(e.into());
            }
        };

        let content = json_body["message"]["content"]
            .as_str()
            .unwrap_or("");

        if content.is_empty() {
            tracing::warn!("[Ollama] Empty content received. Full body: {}", json_body);
        }

        tracing::info!(
            "└── [Ollama ◀ RECV] extract_entities ────────────────────────\n│  Raw NER JSON: {}\n└────────────────────────────────────────────────────────────",
            content
        );

        let parsed: Value = serde_json::from_str(content).map_err(|e| {
            tracing::error!("[Ollama] Failed to parse content as JSON: {}. Content: {}", e, content);
            e
        })?;
        Ok(parsed)
    }

    async fn chat_with_context(
        &self,
        system_prompt: &str,
        user_query: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let client = Client::new();
        
        tracing::info!(
            "┌── [Ollama ▶ SEND] chat_with_context ───────────────────────\n│  model: {}\n└────────────────────────────────────────────────────────────",
            &self.model
        );

        let res = client.post(&format!("{}/api/chat", self.base_url))
            .json(&json!({
                "model": &self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user",   "content": user_query}
                ],
                "stream": false,
                "options": {
                    "temperature": 0.2,
                    "num_ctx": 20480
                }
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        let content = json_body["message"]["content"]
            .as_str()
            .ok_or("Ollama chat failed to return content")?;

        tracing::info!(
            "└── [Ollama ◀ RECV] chat_with_context ───────────────────────\n│  Response len: {} chars\n└────────────────────────────────────────────────────────────",
            content.len()
        );

        Ok(content.to_string())
    }

    async fn normalize_term(&self, term: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        let client = Client::new();
        let prompt = format!(
            "You are a strict clinical terminologist. Convert the following extracted OCR text into its base generic medication or standard clinical diagnosis name. Output ONLY the generic name, absolutely nothing else. Text: '{}'",
            term
        );

        let res = client.post(&format!("{}/api/chat", self.base_url))
            .json(&json!({
                "model": &self.model,
                "messages": [{"role": "user", "content": prompt}],
                "stream": false,
                "options": {
                    "temperature": 0.0,
                    "num_ctx": 20480
                }
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        let content = json_body["message"]["content"]
            .as_str()
            .unwrap_or("");
            
        Ok(content.trim().to_string())
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

        let res = client.post(&format!("{}/api/chat", self.base_url))
            .json(&json!({
                "model": &self.model,
                "messages": [{"role": "user", "content": prompt}],
                "stream": false,
                "format": "json",
                "options": {
                    "temperature": 0.0,
                    "num_ctx": 20480
                }
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        let content = json_body["message"]["content"]
            .as_str()
            .unwrap_or("{}");

        let map: std::collections::HashMap<String, String> = serde_json::from_str(content)?;
        Ok(map)
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

        let res = client.post(&format!("{}/api/chat", self.base_url))
            .json(&json!({
                "model": &self.model,
                "messages": [{"role": "user", "content": prompt}],
                "stream": false,
                "format": "json",
                "options": {
                    "temperature": 0.1,
                    "num_ctx": 20480
                }
            }))
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        let content = json_body["message"]["content"]
            .as_str()
            .unwrap_or("{}");

        Ok(content.to_string())
    }
}