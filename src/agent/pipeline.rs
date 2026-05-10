use crate::clients::agentic::AgentClient;
use crate::config::Config;
use crate::agent::prompt_builder::build_grounded_context;
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

    // 1. Build the grounded context (includes extraction, Postgres, Neo4j, Qdrant)
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

    // 2. Assemble the Mega-Prompt Content
    let full_context = format!(
        "### 1. RAW DOCUMENT EVIDENCE:\n{}\n\n{}",
        document_text, grounded_context
    );

    // 3. Strike LLM
    match main_llm.generate_adjudication_report(&full_context).await {
        Ok(res) => res,
        Err(e) => {
            let error_json = serde_json::json!({
                "status": "ERROR",
                "message": e.to_string()
            });
            error_json.to_string()
        }
    }
}

