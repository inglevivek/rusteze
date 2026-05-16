use crate::resolver::agent::ResolverAgent;
use crate::resolver::models::ResolvedCaseFacts;
use crate::stages::extraction::CaseFacts;

pub async fn resolve_case_facts(
    _agent: &ResolverAgent,
    facts: &CaseFacts,
) -> Result<ResolvedCaseFacts, String> {
    // This is a shim for legacy code in main.rs.
    // In the new architecture, use ResolutionStage.
    
    // For now, return a dummy ResolvedCaseFacts to allow compilation.
    Ok(ResolvedCaseFacts {
        diagnoses: Vec::new(),
        medications: Vec::new(),
        procedures: Vec::new(),
        labs: Vec::new(),
        claim_questions: facts.claim_questions.clone(),
    })
}
