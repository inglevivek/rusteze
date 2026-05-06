mod agent;
mod clients;
mod config;
mod knowledge;

use axum::{
    extract::{Json, Multipart, Path, State},
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

#[derive(Clone)]
struct AppState {
    config: config::Config,
    graph: Arc<Graph>,
    pg_pool: Arc<Pool>,
    main_llm: Arc<dyn AgentClient>,
    slm: Arc<dyn AgentClient>,
}

#[derive(Deserialize)]
struct ChatRequest {
    case_id: String,
    query: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cfg = config::Config::load();

    let graph_pool =
        clients::neo4j::establish_connection(&cfg.neo4j_uri, &cfg.neo4j_user, &cfg.neo4j_pass)
            .await;
    let pg_pool = clients::postgres::establish_pool(&cfg.pg_url).await;

    // Launch Ollama in the background
    // tracing::info!("Starting Ollama server...");
    // let _ollama_process = std::process::Command::new("ollama")
    //     .arg("serve")
    //     .spawn()
    //     .map_err(|e| tracing::warn!("Failed to start Ollama automatically: {}", e));

    let main_llm = Arc::new(GroqClient {
        api_key: cfg.groq_api_key.clone(),
        fast_model: "groq/compound-mini".to_string(),
        heavy_model: "groq/compound".to_string(),
    });

    let slm = Arc::new(GroqClient {
        api_key: cfg.groq_api_key.clone(),
        fast_model: "groq/compound-mini".to_string(),
        heavy_model: "groq/compound-mini".to_string(),
    });

    let shared_state = Arc::new(AppState {
        config: cfg,
        graph: graph_pool,
        pg_pool: Arc::new(pg_pool),
        main_llm,
        slm,
    });

    let app = Router::new()
        .route("/api/health", get(|| async { "D3-GraphBench Engine Live" }))
        .route("/api/ingest", post(handle_ingest))
        .route("/api/chat", post(handle_chat))
        .route("/api/cases", get(handle_list_cases))
        .route("/api/cases/:id", get(handle_get_case))
        .fallback_service(ServeDir::new("public"))
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
    let mut file_bytes = Vec::new();
    let mut file_name = String::new();

    while let Some(field) = multipart.next_field().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();

        if name == "case_id" {
            case_id = field.text().await.map_err(|e| e.to_string())?;
        } else if name == "document" {
            file_name = field.file_name().unwrap_or("upload.file").to_string();
            file_bytes = field.bytes().await.map_err(|e| e.to_string())?.to_vec();
        }
    }

    if case_id.is_empty() || file_bytes.is_empty() {
        return Err("Error: Form must include both 'case_id' and 'document'".to_string());
    }

    tracing::info!(
        "Processing Document: {} for Case ID: {}",
        file_name,
        case_id
    );

    let filename_lower = file_name.to_lowercase();
    let text = if filename_lower.ends_with(".json") {
        tracing::info!("Parsing JSON natively...");
        String::from_utf8(file_bytes).unwrap_or_else(|_| "Invalid JSON UTF-8".to_string())
    } else if filename_lower.ends_with(".pdf") {
        tracing::info!("Extracting PDF text locally...");
        match pdf_extract::extract_text_from_mem(&file_bytes) {
            Ok(t) if t.trim().len() > 50 => {
                tracing::info!("PDF text extracted natively.");
                t
            }
            _ => {
                tracing::warn!(
                    "PDF text empty or extraction failed. Falling back to OCR sidecar..."
                );
                match clients::ocr::extract_text_from_bytes(
                    &state.config.ocr_service_url,
                    file_bytes,
                    file_name,
                )
                .await
                {
                    Ok(t) => t,
                    Err(e) => return Err(format!("{{\"error\": \"OCR Pipeline Error: {}\"}}", e)),
                }
            }
        }
    } else {
        tracing::info!("Sending to OCR sidecar...");
        match clients::ocr::extract_text_from_bytes(
            &state.config.ocr_service_url,
            file_bytes,
            file_name,
        )
        .await
        {
            Ok(t) => t,
            Err(e) => return Err(format!("{{\"error\": \"OCR Pipeline Error: {}\"}}", e)),
        }
    };

    if let Err(e) = clients::qdrant::init_and_embed(&state.config.qdrant_url, &text, &case_id).await
    {
        tracing::error!("Failed to embed document: {}", e);
    }

    // Save case to Database
    if let Err(e) = clients::postgres::save_case(&state.pg_pool, &case_id, &text).await {
        tracing::error!("Failed to save case record: {}", e);
    }

    let final_report = agent::pipeline::run_adjudication(
        state.config.clone(),
        state.main_llm.clone(),
        state.slm.clone(),
        state.graph.clone(),
        state.pg_pool.clone(),
        text,
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

    let response = agent::chat::process_chat(
        state.config.clone(),
        state.main_llm.clone(),
        state.slm.clone(),
        state.graph.clone(),
        state.pg_pool.clone(),
        payload.case_id,
        payload.query,
    )
    .await;

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
