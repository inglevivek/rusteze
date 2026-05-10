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
    ner_llm: Arc<dyn AgentClient>,
    graph: Arc<Graph>,
    pg_pool: Arc<deadpool_postgres::Pool>,
    case: crate::clients::postgres::Case,
    query: String,
) -> String {
    // Guard: detect if the caller accidentally passed a massive/junk string, 
    // but allow legitimate large clinical documents (up to 100k chars).
    if query.len() > 100000 {
        tracing::error!(
            "[Chat] ❌ INVALID QUERY: received oversized string as user query. \
             len={} preview='{}...'",
            query.len(),
            &query[..query.len().min(120)]
        );
        return "Internal error: query string is too large.".to_string();
    }

    let case_id = &case.case_id;
    tracing::info!("[Chat] Searching context for Case ID: {}", case_id);

    let case_context = match qdrant::search_case_context(&config.qdrant_url, &config.embedding_url, &query, &case_id, 5).await {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!("Qdrant search failed: {}", e);
            "".to_string()
        }
    };
    let case_context_split: Vec<String> = case_context.lines().map(|s| s.to_string()).collect();



    let grounded_context = build_grounded_context(
        ner_llm.clone(),
        slm.clone(),
        graph.clone(),
        pg_pool.clone(),
        &case,
        &config.qdrant_url,
        &config.embedding_url,
        &case_context_split,
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
        Ok(response) => {
            format!("{}\n\n---\n**Grounding Context (Neo4j + BODHI):**\n{}", response, grounded_context)
        },
        Err(e) => format!("Error communicating with LLM: {}", e),
    }
}

