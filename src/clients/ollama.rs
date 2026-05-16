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

        let system_msg = r#"You are a strict clinical NER extractor. Output ONLY the JSON object below — no markdown fences, no commentary, no extra keys.

SCHEMA (fixed — never deviate):
{"medications": ["<brand or generic drug name>", ...], "diagnoses": ["<diagnosis or condition>", ...]}

RULES:
1. medications = prescription drugs, antibiotics, antipyretics, supplements, IV fluids by drug name (e.g. "Cefotroy", "Neomol", "Tab.Pexime-200"). Brand names are valid.
2. diagnoses = diseases, syndromes, clinical conditions, lab-confirmed findings (e.g. "Typhoid fever", "Anaemia", "Thrombocytopenia").
3. DO NOT include: patient name, age, gender, hospital name, lab values, units, dates, phone numbers, doctor credentials, test names (Haemoglobin, WBC, MCV), or any other field.
4. DO NOT output "testResults", "documentType", "patientName", or ANY key other than "medications" and "diagnoses".
5. If a drug or diagnosis is repeated, include it only once.
6. If nothing found, output: {"medications": [], "diagnoses": []}

EXAMPLE INPUT (fragment): "Patient admitted with high grade fever. Given Inj.Cefotroy-SB, Tab.Neomol 500mg. Diagnosis: Typhoid fever with Thrombocytopenia."
EXAMPLE OUTPUT: {"medications": ["Cefotroy-SB", "Neomol"], "diagnoses": ["Typhoid fever", "Thrombocytopenia"]}

Now extract from the provided text."#;

        // Truncate to 12000 chars to stay within qwen2.5:3b's reliable window
        // and avoid the model summarising instead of extracting.
        let truncated = if text.len() > 12000 { &text[..12000] } else { text };
        let user_msg = format!(
            "Extract ONLY medications and diagnoses from the clinical document below.\n\
             Do NOT describe the document. Do NOT return testResults, patient info, or lab values.\n\
             Output ONLY: {{\"medications\": [...], \"diagnoses\": [...]}}\n\n---\n{}",
            truncated
        );

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
            Output ONLY a JSON object where the keys are the EXACT original terms provided, and the values are the normalized generic names. If a term is invalid or cannot be normalized, map it to an empty string.\n\n\
            RULES:\n\
            1. medications = generic name (e.g. 'cefixime', 'paracetamol').\n\
            2. diagnoses = standard clinical term (e.g. 'Typhoid fever').\n\
            3. DO NOT include patient info, lab values, or dates.\n\
            4. If a drug or diagnosis is repeated, include it only once.\n\
            5. For brand names with a dash+number suffix (e.g. 'Pexime-200', 'Augmentin-625'), \
               strip the suffix and return just the brand root: 'Pexime', 'Augmentin'. \
               Do NOT return empty string for branded drugs you recognise at the root level.\n\
            6. If you truly cannot map a term, return the string 'UNKNOWN' (not empty string). \
               Empty string means the term was present in the prompt but you skipped it.\n\n\
            Terms: {}",
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
