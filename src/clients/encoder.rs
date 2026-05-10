use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct EmbedRequest {
    texts: Vec<String>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[derive(Clone)]
pub struct BioLordEncoder {
    client: Client,
    url: String,
}

impl BioLordEncoder {
    pub fn new(url: &str) -> Self {
        Self {
            client: Client::new(),
            url: url.to_string(),
        }
    }

    pub async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let req = EmbedRequest { texts };
        
        let endpoint = format!("{}/embed", self.url.trim_end_matches('/'));

        let response = self.client
            .post(&endpoint)
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Request to embedding sidecar failed: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("Embedding sidecar returned error: {}", error_text));
        }

        let resp_data: EmbedResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse embedding response: {}", e))?;

        Ok(resp_data.embeddings)
    }
}
