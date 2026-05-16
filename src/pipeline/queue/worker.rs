use sqlx::{PgPool, Row};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use crate::pipeline::orchestrator::DagOrchestrator;
use crate::stages::ingestion::RawPayload;

pub struct JobWorker;

impl JobWorker {
    /// Spawns N concurrent background workers to process the DAG pipeline with graceful shutdown support.
    pub fn spawn_workers(
        concurrency: usize,
        db_pool: Arc<PgPool>,
        orchestrator: Arc<DagOrchestrator>,
        cancel_token: CancellationToken,
    ) {
        for worker_id in 0..concurrency {
            let pool = db_pool.clone();
            let orch = orchestrator.clone();
            let token = cancel_token.clone();

            tokio::spawn(async move {
                loop {
                    // Check for shutdown signal before acquiring a new job
                    if token.is_cancelled() {
                        println!("Worker {} shutting down gracefully...", worker_id);
                        break;
                    }

                    // 1. Atomically fetch and lock the next pending job
                    let job_opt = sqlx::query(
                        r#"
                        UPDATE pipeline_jobs 
                        SET status = 'Processing', started_at = NOW() 
                        WHERE id = (
                            SELECT id FROM pipeline_jobs 
                            WHERE status = 'Pending' 
                            ORDER BY created_at ASC 
                            FOR UPDATE SKIP LOCKED 
                            LIMIT 1
                        ) 
                        RETURNING id, trace_id, case_id, payload_in
                        "#
                    )
                    .fetch_optional(&*pool)
                    .await;

                    match job_opt {
                        Ok(Some(row)) => {
                            let job_id: uuid::Uuid = row.get("id");
                            let trace_id: String = row.get("trace_id");
                            let case_id: String = row.get("case_id");
                            let payload_in: serde_json::Value = row.get("payload_in");

                            let payload: RawPayload = serde_json::from_value(payload_in).unwrap();
                            
                            // 2. Execute the heavy pipeline
                            // Await execution. If token cancels here, we still finish the current job to prevent data corruption.
                            let result = orch.execute_pipeline(trace_id, case_id, payload).await;

                            // 3. Mark job completion status
                            let status = if result.is_ok() { "Completed" } else { "Failed" };
                            let error_msg = result.err().map(|e| e.to_string());

                            let _ = sqlx::query(
                                r#"
                                UPDATE pipeline_jobs 
                                SET status = $1, completed_at = NOW(), error = $2 
                                WHERE id = $3
                                "#
                            )
                            .bind(status)
                            .bind(error_msg)
                            .bind(job_id)
                            .execute(&*pool)
                            .await;
                        }
                        Ok(None) => {
                            // Queue is empty. Use tokio::select to allow immediate wake-up on cancellation.
                            tokio::select! {
                                _ = sleep(Duration::from_secs(2)) => {}
                                _ = token.cancelled() => {
                                    println!("Worker {} shutting down gracefully during idle...", worker_id);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Worker {} encountered DB error: {}", worker_id, e);
                            tokio::select! {
                                _ = sleep(Duration::from_secs(5)) => {}
                                _ = token.cancelled() => break,
                            }
                        }
                    }
                }
            });
        }
    }
}
