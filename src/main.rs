use d3_graph_bench::{agent, clients, config};

use axum::{
    extract::{Json, Multipart, Path, Query, State},
    routing::{get, post},
    Router,
};
use deadpool_postgres::Pool;
use neo4rs::Graph;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::services::ServeDir;

use clients::agentic::AgentClient;
use clients::groq::GroqClient;
use clients::ollama::OllamaClient;

#[derive(Clone)]
struct AppState {
    config: config::Config,
    graph: Arc<Graph>,
    pg_pool: Arc<Pool>,
    main_llm: Arc<dyn AgentClient>,
    slm: Arc<dyn AgentClient>,
    ner_llm: Arc<dyn AgentClient>,
}

#[derive(Deserialize)]
struct ChatRequest {
    case_id: String,
    query: String,
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

    let main_llm = Arc::new(OllamaClient {
        base_url: cfg.ollama_url.clone(),
        model: cfg.main_model.clone(),
    });

    let slm = Arc::new(OllamaClient {
        base_url: cfg.ollama_url.clone(),
        model: cfg.slm_model.clone(),
    });

    let ner_llm = Arc::new(OllamaClient {
        base_url: cfg.ollama_url.clone(),
        model: cfg.slm_model.clone(),
    });

    // Verify bodhi_global_knowledge is populated at startup
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
        graph: graph_pool,
        pg_pool: Arc::new(pg_pool),
        main_llm,
        slm,
        ner_llm,
    });

    let app = Router::new()
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
    axum::serve(listener, app).await.unwrap();
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

    let mut aggregated_text = String::new();
    for (file_name, file_bytes) in documents {
        match extract_file_text(&state, &file_name, file_bytes).await {
            Ok(t) => {
                tracing::info!("Extracted {} chars from {}", t.len(), file_name);
                aggregated_text.push_str(&format!("\n--- Document: {} ---\n{}\n", file_name, t));
            }
            Err(e) => {
                tracing::warn!("Failed to extract text from {}: {}", file_name, e);
            }
        }
    }

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
        state.graph.clone(),
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
        state.graph.clone(),
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
        state.graph.clone(),
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

async fn extract_file_text(
    state: &Arc<AppState>,
    file_name: &str,
    file_bytes: Vec<u8>,
) -> Result<String, String> {
    let filename_lower = file_name.to_lowercase();
    if filename_lower.ends_with(".json") {
        tracing::info!("Parsing JSON natively...");
        Ok(String::from_utf8(file_bytes).unwrap_or_else(|_| "Invalid JSON UTF-8".to_string()))
    } else if filename_lower.ends_with(".pdf") {
        tracing::info!("Extracting PDF text locally...");
        match pdf_extract::extract_text_from_mem(&file_bytes) {
            Ok(t) if t.trim().len() > 50 => {
                tracing::info!("PDF text extracted natively.");
                Ok(t)
            }
            _ => {
                tracing::warn!(
                    "PDF text empty or extraction failed. Falling back to OCR sidecar..."
                );
                match clients::ocr::extract_text_from_bytes(
                    &state.config.ocr_service_url,
                    file_bytes,
                    file_name.to_string(),
                )
                .await
                {
                    Ok(t) => Ok(t),
                    Err(e) => Err(format!("OCR Pipeline Error: {}", e)),
                }
            }
        }
    } else {
        tracing::info!("Sending to OCR sidecar...");
        match clients::ocr::extract_text_from_bytes(
            &state.config.ocr_service_url,
            file_bytes,
            file_name.to_string(),
        )
        .await
        {
            Ok(t) => Ok(t),
            Err(e) => Err(format!("OCR Pipeline Error: {}", e)),
        }
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

    let mut aggregated_text = String::new();
    let mut results = Vec::new();

    for (file_name, file_bytes) in documents {
        match extract_file_text(&state, &file_name, file_bytes).await {
            Ok(t) => {
                tracing::info!("Extracted {} chars from {}", t.len(), file_name);
                aggregated_text.push_str(&format!("\n--- Document: {} ---\n{}\n", file_name, t));
                results.push(BatchFileResult {
                    file_name,
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
        state.graph.clone(),
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

    Ok(Json(BatchResponse {
        case_id,
        results,
    }))
}
