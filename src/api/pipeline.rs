use axum::{
    extract::{State, Path, ws::{WebSocketUpgrade, WebSocket, Message}},
    response::IntoResponse,
    routing::{post, get},
    Json, Router, http::StatusCode,
};
use std::sync::Arc;
use uuid::Uuid;
use sqlx::Row;
use crate::AppState; 
use crate::stages::ingestion::RawPayload;
use crate::pipeline::queue::enqueuer::JobEnqueuer;

pub fn pipeline_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/pipeline/execute", post(execute_case_durable))
        .route("/api/cases/:id/pipeline-events", get(get_case_telemetry))
        .route("/api/debug/stream", get(telemetry_stream))
}

async fn execute_case_durable(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RawPayload>,
) -> impl IntoResponse {
    let trace_id = Uuid::new_v4().to_string();
    let case_id = format!("CASE-{}", Uuid::new_v4().simple());

    let enqueuer = JobEnqueuer::new(state.sqlx_pool.clone());

    match enqueuer.enqueue(&trace_id, &case_id, &payload).await {
        Ok(_) => (StatusCode::ACCEPTED, axum::response::Json(serde_json::json!({
            "status": "queued",
            "trace_id": trace_id,
            "case_id": case_id,
            "message": "Case durably enqueued for processing."
        }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, axum::response::Json(serde_json::json!({
            "status": "error",
            "trace_id": trace_id,
            "error": e.to_string()
        })))
    }
}

async fn get_case_telemetry(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<String>,
) -> impl IntoResponse {
    // Using sqlx::query (unchecked) to avoid compile-time DATABASE_URL requirement
    let result = sqlx::query(
        r#"
        SELECT trace_id, stage, payload_in, payload_out, duration_ms, tool_calls 
        FROM pipeline_events 
        WHERE case_id = $1 
        ORDER BY created_at ASC
        "#
    )
    .bind(case_id)
    .fetch_all(&*state.sqlx_pool)
    .await;

    match result {
        Ok(records) => {
            let events: Vec<_> = records.into_iter().map(|r| {
                serde_json::json!({
                    "trace_id": r.get::<String, _>("trace_id"),
                    "stage": r.get::<String, _>("stage"),
                    "payload_in": r.get::<serde_json::Value, _>("payload_in"),
                    "payload_out": r.get::<serde_json::Value, _>("payload_out"),
                    "duration_ms": r.get::<i64, _>("duration_ms"),
                    "tool_calls": r.get::<serde_json::Value, _>("tool_calls"),
                })
            }).collect();
            
            (StatusCode::OK, axum::response::Json(serde_json::json!({ "events": events })))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, axum::response::Json(serde_json::json!({ "error": e.to_string() }))),
    }
}

async fn telemetry_stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let receiver = state.pipeline_bus.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, receiver))
}

async fn handle_socket(
    mut socket: WebSocket,
    mut receiver: tokio::sync::broadcast::Receiver<crate::pipeline::events::PipelineEvent>,
) {
    while let Ok(event) = receiver.recv().await {
        if let Ok(json) = serde_json::to_string(&event) {
            if socket.send(Message::Text(json)).await.is_err() {
                break;
            }
        }
    }
}
