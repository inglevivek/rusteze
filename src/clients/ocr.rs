use reqwest::multipart;
use serde::Deserialize;
use std::error::Error;

#[derive(Deserialize, Debug)]
pub struct OcrResponse {
    pub text: String,
}

pub async fn extract_text_from_bytes(
    ocr_url: &str,
    bytes: Vec<u8>,
    filename: String,
) -> Result<String, Box<dyn Error>> {
    let client = reqwest::Client::new();
    
    // Package the raw bytes into a multipart form
    let part = multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str("application/octet-stream")?;
        
    let form = multipart::Form::new().part("document", part);

    // Fire it at the NodeJS sidecar
    let res = client.post(ocr_url).multipart(form).send().await?;

    if res.status().is_success() {
        let json: OcrResponse = res.json().await?;
        Ok(json.text)
    } else {
        Err(format!("OCR Service Failed with status: {}", res.status()).into())
    }
}