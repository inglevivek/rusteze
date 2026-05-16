use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use crate::pipeline::{PipelineError, PipelineStage};
use crate::resolver::models::ResolvedCaseFacts;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedEvidenceRequest {
    pub cypher_queries: Vec<String>,
    pub semantic_queries: Vec<String>, // Terms to be vectorized for Qdrant
}

pub struct CypherValidator;

impl CypherValidator {
    /// Security Gate: Statically analyzes Cypher strings to prevent injection.
    /// Rejects any query containing mutation keywords or structural anomalies.
    pub fn validate(query: &str) -> Result<(), PipelineError> {
        let normalized = query.to_uppercase();
        let forbidden_keywords = [
            "CREATE", "MERGE", "SET", "DELETE", "REMOVE", 
            "DROP", "CALL", "YIELD", "LOAD CSV", "FOREACH"
        ];

        for keyword in forbidden_keywords.iter() {
            // Basic substring boundary check to avoid rejecting e.g., a node named 'CREATE_DATE'
            // In a production AST, this would check token types.
            if normalized.split_whitespace().any(|token| token == *keyword) {
                return Err(PipelineError::Validation(format!(
                    "Malicious or unauthorized Cypher mutation detected: {}", keyword
                )));
            }
        }

        if !normalized.starts_with("MATCH") && !normalized.starts_with("WITH") {
            return Err(PipelineError::Validation(
                "Query must start with MATCH or WITH".to_string()
            ));
        }

        Ok(())
    }
}

pub struct QueryPlannerStage {
    // llm_client: Arc<dyn LlmClient>,
}

impl QueryPlannerStage {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl PipelineStage<ResolvedCaseFacts, PlannedEvidenceRequest> for QueryPlannerStage {
    fn name(&self) -> &'static str {
        "Stage4_QueryPlanner"
    }

    async fn execute(&self, input: ResolvedCaseFacts) -> Result<PlannedEvidenceRequest, PipelineError> {
        // TODO: Ask SLM to map `input` to Cypher and Qdrant queries based on BODHI schema.
        
        // DUMMY IMPLEMENTATION FOR SCAFFOLDING
        let raw_queries = vec![
            format!(
                "MATCH (d:Diagnosis {{id: '{}'}})-[:TREATS]-(m:Medication) RETURN m",
                input.diagnoses.first().and_then(|d| d.standard_id.clone()).unwrap_or_default()
            )
        ];

        // Security execution: Validate ALL generated queries before allowing them into the payload
        for q in &raw_queries {
            CypherValidator::validate(q)?;
        }

        Ok(PlannedEvidenceRequest {
            cypher_queries: raw_queries,
            semantic_queries: input.claim_questions.clone(),
        })
    }
}
