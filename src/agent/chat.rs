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
    if query.len() > 100000 {
        tracing::error!(
            "[Chat] ❌ INVALID QUERY: oversized string rejected. len={}",
            query.len()
        );
        return "Internal error: query string is too large.".to_string();
    }

    let case_id = &case.case_id;
    let query_preview = &query[..query.len().min(80)];

    tracing::info!(
        "╔══ [Chat] Processing ══════════════════════════════════════╗\n  \
         case_id : {}\n  \
         query   : {}{}",
        case_id,
        query_preview,
        if query.len() > 80 { "…" } else { "" }
    );

    // Step 1: Qdrant case-specific retrieval
    let case_context = match qdrant::search_case_context(
        &config.qdrant_url,
        &config.embedding_url,
        &query,
        case_id,
        15,
    )
    .await
    {
        Ok(ctx) => {
            let chunks = ctx.lines().count();
            tracing::info!("  ├─ [Qdrant] case context   : {} chunks retrieved", chunks);
            ctx
        }
        Err(e) => {
            tracing::warn!("  ├─ [Qdrant] case context   : ❌ FAILED ({})", e);
            String::new()
        }
    };
    let case_context_split: Vec<String> = case_context.lines().map(|s| s.to_string()).collect();

    // Step 2: Global grounding (Bodhi + Neo4j)
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

    tracing::info!(
        "  ├─ [Bodhi+Neo4j] grounding : {} lines",
        grounded_context.lines().count()
    );

    // Step 3: Build system prompt and fire LLM
    let system_prompt = format!(
        "You are a clinical AI assistant analyzing patient case data. \
        Use the following context extracted from the patient's medical records to answer the query. \
        If the answer is not in the context, state that clearly. \
        Do not hallucinate external medical history.\n\n\
        ### PATIENT CONTEXT (Case ID: {}):\n{}\n\n\
        ### GLOBAL KNOWLEDGE GROUNDING:\n{}",
        case_id, case_context, grounded_context
    );

    match main_llm.chat_with_context(&system_prompt, &query).await {
        Ok(response) => {
            tracing::info!(
                "  └─ [LLM] response          : {} chars\n\
                 ╚════════════════════════════════════════════════════════════╝",
                response.len()
            );
            format!(
                "{}\n\n---\n**Grounding Context (Neo4j + BODHI):**\n{}",
                response, grounded_context
            )
        }
        Err(e) => {
            tracing::error!(
                "  └─ [Chat] ❌ LLM failed: {}\n\
                 ╚════════════════════════════════════════════════════════════╝",
                e
            );
            format!("Error communicating with LLM: {}", e)
        }
    }
}

