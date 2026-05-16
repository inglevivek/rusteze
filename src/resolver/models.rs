use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedEntity {
    pub original_term: String,
    pub canonical_name: Option<String>,
    pub standard_id: Option<String>,
    pub standard_system: Option<String>, // e.g., "RxNorm", "ICD-11", "UMLS"
    pub confidence: f32,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedCaseFacts {
    pub diagnoses: Vec<ResolvedEntity>,
    pub medications: Vec<ResolvedEntity>,
    pub procedures: Vec<ResolvedEntity>,
    pub labs: Vec<ResolvedEntity>,
    pub claim_questions: Vec<String>,
}
