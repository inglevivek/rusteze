use d3_graph_bench::{agent, clients, config, AppState};

use axum::{
    extract::{Json, Multipart, Path, Query, State},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tokio_util::sync::CancellationToken;

use clients::agentic::AgentClient;
use clients::groq::GroqClient;
use clients::ollama::OllamaClient;
use rig::providers::openai::Client as RigOpenAIClient;
use qdrant_client::Qdrant;


#[derive(Deserialize)]
struct ChatRequest {
    case_id: String,
    query: String,
}

fn build_client(
    provider: &str,
    ollama_url: &str,
    ollama_model: &str,
    groq_api_key: &str,
    groq_model: &str,
) -> Arc<dyn AgentClient> {
    match provider {
        "groq" => Arc::new(GroqClient {
            api_key:     groq_api_key.to_string(),
            fast_model:  groq_model.to_string(),
            heavy_model: groq_model.to_string(),
        }),
        _ => Arc::new(OllamaClient {
            base_url: ollama_url.to_string(),
            model:    ollama_model.to_string(),
        }),
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            async_main().await;
        });
}

async fn async_main() {
    let cfg = config::Config::load();

    let graph_pool =
        clients::neo4j::establish_connection(&cfg.neo4j_uri, &cfg.neo4j_user, &cfg.neo4j_pass)
            .await;
    let pg_pool = clients::postgres::establish_pool(&cfg.pg_url).await;

    // Self-Migration: Apply chat history schema if missing
    {
        tracing::info!("[Startup] Checking for chat history schema...");
        let client = pg_pool.get().await;
        match client {
            Ok(c) => {
                let migration_sql = std::fs::read_to_string("migrations/0003_chat_messages.sql")
                    .unwrap_or_default();
                if !migration_sql.is_empty() {
                    match c.batch_execute(&migration_sql).await {
                        Ok(_) => tracing::info!("✅ [Startup] Database schema upgraded (migrations applied)."),
                        Err(e) => tracing::warn!("⚠️ [Startup] Migration check finished: {}", e),
                    }
                }
            }
            Err(e) => tracing::error!("❌ [Startup] Could not get DB connection for migration: {}", e),
        }
    }


    // Launch Ollama in the background
    // tracing::info!("Starting Ollama server...");
    // let _ollama_process = std::process::Command::new("ollama")
    //     .arg("serve")
    //     .spawn()
    //     .map_err(|e| tracing::warn!("Failed to start Ollama automatically: {}", e));

    let main_llm = build_client(
        &cfg.main_llm_provider,
        &cfg.ollama_url, &cfg.main_model,
        &cfg.groq_api_key, &cfg.main_model,
    );

    let slm = build_client(
        &cfg.slm_provider,
        &cfg.ollama_url, &cfg.slm_model,
        &cfg.groq_api_key, &cfg.slm_model,
    );

    let ner_llm = build_client(
        &cfg.ner_llm_provider,
        &cfg.ollama_url, &cfg.ner_model,
        &cfg.groq_api_key, &cfg.ner_model,
    );

    let openai_client = Arc::new(RigOpenAIClient::new("dummy").expect("Failed to create Rig OpenAI client"));
    let qdrant_client = Arc::new(Qdrant::from_url(&cfg.qdrant_url).build().expect("Failed to create Qdrant client"));
    let pipeline_bus = Arc::new(d3_graph_bench::pipeline::bus::PipelineEventBus::new(1024));
    
    // sqlx pool for Phase 7 Telemetry Sink
    let sqlx_pool = Arc::new(sqlx::PgPool::connect(&cfg.pg_url).await.expect("Failed to connect to PostgreSQL via sqlx"));
    
    let sink = d3_graph_bench::pipeline::sink::BatchedTelemetrySink::new(pipeline_bus.clone(), sqlx_pool.clone());
    sink.spawn_listener();

    match crate::clients::qdrant::count_collection_points(&cfg.qdrant_url, "bodhi_global_knowledge").await {
        Ok(n) if n == 0 => tracing::error!(
            "❌ [Startup] bodhi_global_knowledge collection has 0 points. \
             Global knowledge grounding will return no results. \
             Run the data ingestion script in data_scripts/ to populate it."
        ),
        Ok(n) => tracing::info!("✅ [Startup] bodhi_global_knowledge has {} points ready.", n),
        Err(e) => tracing::error!("❌ [Startup] Could not reach Qdrant to check bodhi_global_knowledge: {}", e),
    }

    let shared_state = Arc::new(AppState {
        config: cfg,
        neo4j_client: graph_pool,
        pg_pool: Arc::new(pg_pool),
        main_llm,
        slm,
        ner_llm,
        openai_client,
        qdrant_client,
        pipeline_bus,
        sqlx_pool: sqlx_pool.clone(),
    });

    // Lifecycle Management (Phase 9)
    let cancel_token = CancellationToken::new();

    // Durable Queue Workers (Phase 8 & 9)
    let orchestrator = Arc::new(d3_graph_bench::pipeline::orchestrator::DagOrchestrator::new(
        shared_state.pipeline_bus.clone(),
        shared_state.clone(),
    ));
    d3_graph_bench::pipeline::queue::JobWorker::spawn_workers(
        4, 
        sqlx_pool.clone(), 
        orchestrator, 
        cancel_token.clone()
    );

    // Zombie Job Sweeper (Phase 9)
    d3_graph_bench::pipeline::queue::ZombieSweeper::spawn(
        sqlx_pool.clone(), 
        cancel_token.clone()
    );

    let app = Router::new()
        .merge(d3_graph_bench::api::pipeline::pipeline_routes())
        .route("/api/health", get(|| async { "D3-GraphBench Engine Live" }))
        .route("/api/ingest",          post(handle_ingest))
        .route("/api/chat",            post(handle_chat))
        // ── Case CRUD ──────────────────────────────────────────────
        .route("/api/cases",           get(handle_list_cases))
        .route("/api/cases/:id",       get(handle_get_case)
                                       .put(handle_update_case_text)
                                       .delete(handle_delete_case))
        // ── Adjudication retry ─────────────────────────────────────
        .route("/api/cases/:id/readjudicate", post(handle_readjudicate))
        // ── Chat history ───────────────────────────────────────────
        .route("/api/cases/:id/history",
               get(handle_get_history)
               .delete(handle_clear_history))
        .route("/api/messages/:msg_id", axum::routing::delete(handle_delete_message))
        // ── Batch ingest ───────────────────────────────────────────
        .route("/api/ingest/batch",    post(handle_ingest_batch))
        .fallback_service(ServeDir::new("public"))
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024))
        .with_state(shared_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Rust Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    
    // Graceful Shutdown handler
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for ctrl_c signal");
            tracing::info!("Shutdown signal received. Cancelling background workers...");
            cancel_token.cancel();
            
            // Give workers a moment to finish current jobs if any
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            tracing::info!("Graceful shutdown complete.");
        })
        .await
        .unwrap();
}

async fn handle_ingest(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<String, String> {
    let mut case_id = String::new();
    let mut documents = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();

        if name == "case_id" {
            case_id = field.text().await.map_err(|e| e.to_string())?;
        } else if name == "document" {
            let file_name = field.file_name().unwrap_or("upload.file").to_string();
            let file_bytes = field.bytes().await.map_err(|e| e.to_string())?.to_vec();
            if !file_bytes.is_empty() {
                documents.push((file_name, file_bytes));
            }
        }
    }

    if case_id.is_empty() || documents.is_empty() {
        return Err("Error: Form must include both 'case_id' and at least one 'document'".to_string());
    }

    tracing::info!(
        "Processing {} Documents for Case ID: {}",
        documents.len(),
        case_id
    );

    d3_graph_bench::observability::emit_stage_event(
        "handler",
        "handler_started",
        Some(&case_id),
        serde_json::json!({"docs_count": documents.len()}),
        serde_json::json!({"case_id": case_id}),
    );

    let mut aggregated_text = String::new();
    for (file_name, file_bytes) in documents {
        match d3_graph_bench::ingestion::pipeline::ingest_document(&state, file_name.clone(), file_bytes).await {
            Ok(doc) => {
                let name = doc.file_name.clone().unwrap_or(file_name);
                tracing::info!("Extracted {} chars from {}", doc.text_content.len(), name);
                
                // Phase 2: Call extraction service
                if let Ok(facts) = d3_graph_bench::extraction::service::extract_case_facts(&state.ner_llm, &doc).await {
                    tracing::info!("Extracted {} diagnoses and {} medications from {}", facts.diagnoses.len(), facts.medications.len(), name);
                    
                    // Phase 3: Call resolver agent
                    let resolver = d3_graph_bench::resolver::agent::ResolverAgent::new(state.openai_client.clone());
                    if let Ok(resolved) = d3_graph_bench::resolver::service::resolve_case_facts(&resolver, &facts).await {
                        tracing::info!("Resolved {} diagnoses and {} medications from {}", resolved.diagnoses.len(), resolved.medications.len(), name);
                    }
                }

                aggregated_text.push_str(&format!("\n--- Document: {} ---\n{}\n", name, doc.text_content));
            }
            Err(e) => {
                tracing::warn!("Failed to ingest document: {}", e);
            }
        }
    }

    d3_graph_bench::observability::emit_stage_event(
        "handler",
        "aggregated_text_ready",
        Some(&case_id),
        serde_json::json!({"total_len": aggregated_text.len()}),
        serde_json::json!({"case_id": case_id}),
    );

    let text = aggregated_text;

    if text.trim().is_empty() {
        return Err("Error: No text could be extracted from provided documents".to_string());
    }

    if let Err(e) = clients::qdrant::init_and_embed(
        &state.config.qdrant_url,
        &state.config.embedding_url,
        &text,
        &case_id,
        state.config.embedding_vector_size as u64,
    )
    .await
    {
        tracing::error!("Failed to embed document: {}", e);
    }

    // Save case to Database
    if let Err(e) = clients::postgres::save_case(&state.pg_pool, &case_id, &text).await {
        tracing::error!("Failed to save case record: {}", e);
    }

    let case = clients::postgres::Case {
        case_id: case_id.clone(),
        document_text: text,
        adjudication_report: None,
        created_at: "".to_string(),
    };

    let final_report = agent::pipeline::run_adjudication(
        state.config.clone(),
        state.main_llm.clone(),
        state.slm.clone(),
        state.ner_llm.clone(),
        state.neo4j_client.clone(),
        state.pg_pool.clone(),
        case,
    )
    .await;

    // Update case with adjudication report
    if let Err(e) =
        clients::postgres::update_case_report(&state.pg_pool, &case_id, &final_report).await
    {
        tracing::error!("Failed to save adjudication report: {}", e);
    }

    d3_graph_bench::observability::emit_stage_event(
        "handler",
        "handler_complete",
        Some(&case_id),
        serde_json::json!({"status": "success"}),
        serde_json::json!({"case_id": case_id}),
    );

    Ok(final_report)
}

async fn handle_chat(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Result<String, String> {
    if payload.case_id.trim().is_empty() || payload.query.trim().is_empty() {
        return Err("case_id and query cannot be empty".to_string());
    }

    // Validate case exists before touching LLM stack
    let case = match clients::postgres::get_case(&state.pg_pool, &payload.case_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Err(format!(
                "Case '{}' not found. Please ingest the case documents first via /api/ingest.",
                payload.case_id
            ));
        }
        Err(e) => {
            tracing::error!("[Chat] DB error checking case existence: {}", e);
            return Err("Database error validating case ID.".to_string());
        }
    };

    // Log the user turn BEFORE calling the LLM (preserved even on LLM error)
    let sanitized_query = payload.query.replace('\0', "");
    if let Err(e) = clients::postgres::append_chat_message(
        &state.pg_pool, &payload.case_id, "user", &sanitized_query,
    ).await {
        tracing::warn!("[Chat] Failed to log user message for case '{}'. Error: {:?}", payload.case_id, e);
    }

    let response = agent::chat::process_chat(
        state.config.clone(),
        state.main_llm.clone(),
        state.slm.clone(),
        state.ner_llm.clone(),
        state.neo4j_client.clone(),
        state.pg_pool.clone(),
        case,
        payload.query,
    )
    .await;

    // Log the assistant turn
    // Sanitization: Postgres TEXT cannot contain null bytes
    let sanitized_response = response.replace('\0', "");
    
    if let Err(e) = clients::postgres::append_chat_message(
        &state.pg_pool, &payload.case_id, "assistant", &sanitized_response,
    ).await {
        tracing::warn!("[Chat] Failed to log assistant response for case '{}'. Error: {:?}", payload.case_id, e);
    }

    Ok(response)
}

async fn handle_list_cases(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<clients::postgres::CaseSummary>>, String> {
    match clients::postgres::list_cases(&state.pg_pool).await {
        Ok(cases) => Ok(Json(cases)),
        Err(e) => Err(format!("Database error: {}", e)),
    }
}

async fn handle_get_case(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<String>,
) -> Result<Json<clients::postgres::Case>, String> {
    match clients::postgres::get_case(&state.pg_pool, &case_id).await {
        Ok(Some(case)) => Ok(Json(case)),
        Ok(None) => Err("Case not found".to_string()),
        Err(e) => Err(format!("Database error: {}", e)),
    }
}

/// DELETE /api/cases/:id
/// Permanently removes a case and (via CASCADE) all its chat history.
async fn handle_delete_case(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<String>,
) -> Result<String, String> {
    match clients::postgres::delete_case(&state.pg_pool, &case_id).await {
        Ok(true)  => Ok(format!("Case '{}' deleted", case_id)),
        Ok(false) => Err(format!("Case '{}' not found", case_id)),
        Err(e)    => Err(format!("Database error: {}", e)),
    }
}

/// PUT /api/cases/:id
/// Body: plain-text new document_text. Does NOT re-adjudicate.
/// Use POST /api/cases/:id/readjudicate afterwards if needed.
#[derive(Deserialize)]
struct UpdateCaseTextBody {
    document_text: String,
}

async fn handle_update_case_text(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<String>,
    Json(body): Json<UpdateCaseTextBody>,
) -> Result<String, String> {
    if body.document_text.trim().is_empty() {
        return Err("document_text cannot be empty".to_string());
    }
    match clients::postgres::update_case_text(&state.pg_pool, &case_id, &body.document_text).await {
        Ok(true)  => Ok(format!("Case '{}' document text updated", case_id)),
        Ok(false) => Err(format!("Case '{}' not found", case_id)),
        Err(e)    => Err(format!("Database error: {}", e)),
    }
}

/// POST /api/cases/:id/readjudicate
/// Re-runs the full adjudication pipeline for an existing case and
/// overwrites the stored report. Useful after graph updates.
async fn handle_readjudicate(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<String>,
) -> Result<String, String> {
    let case = match clients::postgres::get_case(&state.pg_pool, &case_id).await {
        Ok(Some(c)) => c,
        Ok(None)    => return Err(format!("Case '{}' not found", case_id)),
        Err(e)      => return Err(format!("Database error: {}", e)),
    };

    tracing::info!("[Readjudicate] Re-running adjudication for case '{}'", case_id);

    let final_report = agent::pipeline::run_adjudication(
        state.config.clone(),
        state.main_llm.clone(),
        state.slm.clone(),
        state.ner_llm.clone(),
        state.neo4j_client.clone(),
        state.pg_pool.clone(),
        case,
    )
    .await;

    if let Err(e) = clients::postgres::update_case_report(&state.pg_pool, &case_id, &final_report).await {
        tracing::error!("[Readjudicate] Failed to save new report: {}", e);
    }

    Ok(final_report)
}

/// GET /api/cases/:id/history?limit=50&before_id=
/// Returns paginated chat history for a case, oldest-first.
/// Use `before_id` (the lowest id from the previous page) to load older messages.
#[derive(Deserialize)]
struct HistoryParams {
    limit:     Option<i64>,
    before_id: Option<i64>,
}

async fn handle_get_history(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<Vec<clients::postgres::ChatMessage>>, String> {
    match clients::postgres::get_chat_history(
        &state.pg_pool,
        &case_id,
        params.limit,
        params.before_id,
    )
    .await
    {
        Ok(msgs) => Ok(Json(msgs)),
        Err(e)   => Err(format!("Database error: {}", e)),
    }
}

/// DELETE /api/cases/:id/history
/// Wipes all chat history for a case. Returns count of deleted rows.
async fn handle_clear_history(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<String>,
) -> Result<String, String> {
    match clients::postgres::clear_chat_history(&state.pg_pool, &case_id).await {
        Ok(n)  => Ok(format!("Deleted {} message(s) for case '{}'", n, case_id)),
        Err(e) => Err(format!("Database error: {}", e)),
    }
}

/// DELETE /api/messages/:msg_id
/// Deletes a single message by its numeric ID.
async fn handle_delete_message(
    State(state): State<Arc<AppState>>,
    Path(msg_id): Path<i64>,
) -> Result<String, String> {
    match clients::postgres::delete_chat_message(&state.pg_pool, msg_id).await {
        Ok(true)  => Ok(format!("Message {} deleted", msg_id)),
        Ok(false) => Err(format!("Message {} not found", msg_id)),
        Err(e)    => Err(format!("Database error: {}", e)),
    }
}


#[derive(serde::Serialize)]
struct BatchFileResult {
    file_name: String,
    status: String,
    report: Option<serde_json::Value>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct BatchResponse {
    case_id: String,
    results: Vec<BatchFileResult>,
}

async fn handle_ingest_batch(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<BatchResponse>, String> {
    let mut case_id = String::new();
    let mut documents = Vec::new();

    tracing::info!(
        "[BatchIngest] Starting multipart extraction..."
    );

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();

        if name == "case_id" {
            case_id = field.text().await.map_err(|e| e.to_string())?;
            tracing::info!("[BatchIngest] Found case_id: {}", case_id);
        } else if name == "document" {
            let file_name = field.file_name().unwrap_or("upload.file").to_string();
            let file_bytes = field.bytes().await.map_err(|e| e.to_string())?.to_vec();
            tracing::info!("[BatchIngest] Extracted document: {} ({} bytes)", file_name, file_bytes.len());
            if !file_bytes.is_empty() {
                documents.push((file_name, file_bytes));
            }
        }
    }

    if case_id.is_empty() || documents.is_empty() {
        tracing::error!("[BatchIngest] Validation failed: case_id='{}', docs_count={}", case_id, documents.len());
        return Err("Error: Form must include both 'case_id' and at least one 'document'".to_string());
    }

    tracing::info!(
        "[BatchIngest] Processing {} Documents for Case ID: {}",
        documents.len(),
        case_id
    );

    d3_graph_bench::observability::emit_stage_event(
        "handler",
        "handler_started",
        Some(&case_id),
        serde_json::json!({"docs_count": documents.len(), "batch": true}),
        serde_json::json!({"case_id": case_id}),
    );

    let mut aggregated_text = String::new();
    let mut results = Vec::new();

    for (file_name, file_bytes) in documents {
        match d3_graph_bench::ingestion::pipeline::ingest_document(&state, file_name.clone(), file_bytes).await {
            Ok(doc) => {
                let name = doc.file_name.clone().unwrap_or(file_name);
                tracing::info!("Extracted {} chars from {}", doc.text_content.len(), name);

                // Phase 2: Call extraction service
                if let Ok(facts) = d3_graph_bench::extraction::service::extract_case_facts(&state.ner_llm, &doc).await {
                    tracing::info!("Extracted {} diagnoses and {} medications from {}", facts.diagnoses.len(), facts.medications.len(), name);

                    // Phase 3: Call resolver agent
                    let resolver = d3_graph_bench::resolver::agent::ResolverAgent::new(state.openai_client.clone());
                    if let Ok(resolved) = d3_graph_bench::resolver::service::resolve_case_facts(&resolver, &facts).await {
                        tracing::info!("Resolved {} diagnoses and {} medications from {}", resolved.diagnoses.len(), resolved.medications.len(), name);
                    }
                }

                aggregated_text.push_str(&format!("\n--- Document: {} ---\n{}\n", name, doc.text_content));
                results.push(BatchFileResult {
                    file_name: name,
                    status: "completed".to_string(),
                    report: None, // Will be populated after adjudication
                    error: None,
                });
            }
            Err(e) => {
                results.push(BatchFileResult {
                    file_name,
                    status: "failed".to_string(),
                    report: None,
                    error: Some(e),
                });
            }
        }
    }

    d3_graph_bench::observability::emit_stage_event(
        "handler",
        "aggregated_text_ready",
        Some(&case_id),
        serde_json::json!({"total_len": aggregated_text.len(), "batch": true}),
        serde_json::json!({"case_id": case_id}),
    );

    if aggregated_text.trim().is_empty() {
        return Ok(Json(BatchResponse {
            case_id,
            results,
        }));
    }

    if let Err(e) = clients::qdrant::init_and_embed(
        &state.config.qdrant_url,
        &state.config.embedding_url,
        &aggregated_text,
        &case_id,
        state.config.embedding_vector_size as u64,
    )
    .await
    {
        tracing::error!("Failed to embed aggregated document: {}", e);
    }

    if let Err(e) = clients::postgres::save_case(&state.pg_pool, &case_id, &aggregated_text).await {
        tracing::error!("Failed to save case record: {}", e);
    }

    let case = clients::postgres::Case {
        case_id: case_id.clone(),
        document_text: aggregated_text,
        adjudication_report: None,
        created_at: "".to_string(),
    };

    let final_report = agent::pipeline::run_adjudication(
        state.config.clone(),
        state.main_llm.clone(),
        state.slm.clone(),
        state.ner_llm.clone(),
        state.neo4j_client.clone(),
        state.pg_pool.clone(),
        case,
    )
    .await;

    if let Err(e) = clients::postgres::update_case_report(&state.pg_pool, &case_id, &final_report).await {
        tracing::error!("Failed to save adjudication report: {}", e);
    }

    // Parse the final report so we can embed it in the JSON response
    let parsed_report = serde_json::from_str::<serde_json::Value>(&final_report)
        .unwrap_or(serde_json::Value::String(final_report));

    for result in &mut results {
        if result.status == "completed" {
            result.report = Some(parsed_report.clone());
        }
    }

    d3_graph_bench::observability::emit_stage_event(
        "handler",
        "handler_complete",
        Some(&case_id),
        serde_json::json!({"status": "success", "batch": true}),
        serde_json::json!({"case_id": case_id}),
    );

    Ok(Json(BatchResponse {
        case_id,
        results,
    }))
}

