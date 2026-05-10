use crate::clients::agentic::AgentClient;
use crate::clients::neo4j;
use crate::config::Config;
use crate::agent::prompt_builder::build_grounded_context;
use crate::knowledge::linker;
use neo4rs::Graph;
use std::sync::Arc;

pub async fn run_adjudication(
    config: Config,
    main_llm: Arc<dyn AgentClient>,
    slm: Arc<dyn AgentClient>,
    graph: Arc<Graph>,
    pg_pool: Arc<deadpool_postgres::Pool>,
    document_text: String,
) -> String {
    tracing::info!("[Pipeline] Initiating Dual-Brain GraphRAG Adjudication...");

    // 1. Extract entities for graph medical necessity checks
    let extracted_json = main_llm
        .extract_entities(&document_text)
        .await
        .unwrap_or_default();

    let mut raw_terms = Vec::new();
    if let Some(meds) = extracted_json["medications"].as_array() {
        for m in meds { raw_terms.push(m.as_str().unwrap_or("").to_string()); }
    }
    if let Some(diags) = extracted_json["diagnoses"].as_array() {
        for d in diags { raw_terms.push(d.as_str().unwrap_or("").to_string()); }
    }

    let resolved = linker::resolve_entities(&pg_pool, slm.clone(), raw_terms).await;

    // 2. Run check_medical_necessity for every (diagnosis, drug) pair
    let diagnoses: Vec<_> = resolved.iter().filter(|e| e.term_type == "Disorder" || e.term_type == "Condition").collect();
    let drugs: Vec<_>     = resolved.iter().filter(|e| e.term_type == "Drug" || e.term_type == "Substance").collect();

    let mut graph_verdicts: Vec<String> = Vec::new();
    for diag in &diagnoses {
        for drug in &drugs {
            match neo4j::check_medical_necessity(&graph, &diag.concept_id, &drug.concept_id).await {
                Ok(true)  => {
                    graph_verdicts.push(format!("✅ GRAPH CONFIRMED: {} (snomed:{}) is medically indicated for {} (snomed:{})", drug.name, drug.concept_id, diag.name, diag.concept_id));
                }
                Ok(false) => {
                    graph_verdicts.push(format!("❌ GRAPH FLAG: {} (snomed:{}) has NO documented indication edge for {} (snomed:{}) in BODHI knowledge graph", drug.name, drug.concept_id, diag.name, diag.concept_id));
                }
                Err(e) => {
                    tracing::warn!("[Pipeline] check_medical_necessity error for ({}, {}): {}", diag.concept_id, drug.concept_id, e);
                }
            }
        }
    }

    // 3. Build grounded context (Neo4j neighborhood + Qdrant semantic)
    let grounded_context = build_grounded_context(
        main_llm.clone(),
        slm.clone(),
        graph.clone(),
        pg_pool.clone(),
        &config.qdrant_url,
        &config.embedding_url,
        &document_text,
    )
    .await;

    // 4. Assemble final prompt with graph verdicts injected
    let graph_verdict_section = if !graph_verdicts.is_empty() {
        format!("### GRAPH MEDICAL NECESSITY VERDICTS:\n{}\n\n", graph_verdicts.join("\n"))
    } else {
        "### GRAPH MEDICAL NECESSITY VERDICTS:\nNo drug-diagnosis pairs found for graph validation.\n\n".to_string()
    };

    let full_context = format!(
        "{}### RAW DOCUMENT EVIDENCE:\n{}\n\n{}",
        graph_verdict_section, document_text, grounded_context
    );

    // 5. Strike LLM with full context
    match main_llm.generate_adjudication_report(&full_context).await {
        Ok(res) => res,
        Err(e) => {
            serde_json::json!({
                "status": "ERROR",
                "message": e.to_string()
            }).to_string()
        }
    }
}
