use crate::clients::agentic::AgentClient;
use crate::clients::qdrant;
use crate::config::Config;
use crate::agent::prompt_builder::build_grounded_context;
use neo4rs::Graph;
use std::sync::Arc;

pub async fn process_chat(
    config: Config,
    main_llm: Arc<dyn AgentClient>,
    slm: Arc<dyn AgentClient>,
    graph: Arc<Graph>,
    pg_pool: Arc<deadpool_postgres::Pool>,
    case_id: String,
    query: String,
) -> String {
    tracing::info!("[Chat] Searching context for Case ID: {}", case_id);

    // 1. Pull the top 5 most relevant chunks from Qdrant for this specific case
    let case_context = match qdrant::search_case_context(&config.qdrant_url, &config.embedding_url, &query, &case_id, 5).await {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!("Qdrant search failed: {}", e);
            "".to_string()
        }
    };

    // 2. Build the grounded context (Graph + Global Qdrant) based on the user's query
    let grounded_context = build_grounded_context(
        main_llm.clone(),
        slm.clone(),
        graph.clone(),
        pg_pool.clone(),
        &config.qdrant_url,
        &config.embedding_url,
        &query,
    )
    .await;

    // 3. Build the System Prompt with both local RAG context and global graph grounding
    let system_prompt = format!(
        "You are a clinical AI assistant analyzing patient case data. \
        Use the following context extracted from the patient's medical records to answer the query. \
        If the answer is not in the context, state that clearly. Do not hallucinate external medical history.\n\n\
        ### PATIENT CONTEXT (Case ID: {}):\n{}\n\n\
        ### GLOBAL KNOWLEDGE GROUNDING:\n{}",
        case_id, case_context, grounded_context
    );

    // 4. Fire at the LLM
    match main_llm.chat_with_context(&system_prompt, &query).await {
        Ok(response) => response,
        Err(e) => format!("Error communicating with LLM: {}", e),
    }
}

