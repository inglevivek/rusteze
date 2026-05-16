use rig::tool::Tool;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct RxNormArgs {
    pub term: String,
}

#[derive(Serialize, Deserialize)]
pub struct ResolutionResult {
    pub standard_id: Option<String>,
    pub canonical_name: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[error("Tool error: {0}")]
pub struct ToolError(pub String);

pub struct RxNormExactTool;

impl Tool for RxNormExactTool {
    const NAME: &'static str = "rxnorm_exact_match";
    type Args = RxNormArgs;
    type Output = ResolutionResult;
    type Error = ToolError;

    async fn definition(&self, _prompt: String) -> rig::completion::ToolDefinition {
        rig::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Finds the exact RxCUI for a medication term.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "term": { "type": "string", "description": "The raw medication string" }
                },
                "required": ["term"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Scaffold for actual RxNorm REST API call
        // In production: HTTP GET https://rxnav.nlm.nih.gov/REST/rxcui.json?name=...
        let _term = args.term.to_lowercase();
        
        // DUMMY IMPLEMENTATION
        Ok(ResolutionResult {
            standard_id: Some("860975".to_string()),
            canonical_name: Some("Lisinopril 10 MG".to_string()),
            confidence: 0.95,
        })
    }
}

#[derive(Deserialize)]
pub struct Icd11Args {
    pub term: String,
}

pub struct Icd11Tool;

impl Tool for Icd11Tool {
    const NAME: &'static str = "icd11_search";
    type Args = Icd11Args;
    type Output = ResolutionResult;
    type Error = ToolError;

    async fn definition(&self, _prompt: String) -> rig::completion::ToolDefinition {
        rig::completion::ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Finds the ICD-11 code for a diagnosis.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "term": { "type": "string", "description": "The raw diagnosis string" }
                },
                "required": ["term"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Scaffold for WHO ICD-11 API
        Ok(ResolutionResult {
            standard_id: Some("BA43".to_string()),
            canonical_name: Some("Essential hypertension".to_string()),
            confidence: 0.90,
        })
    }
}
