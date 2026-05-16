use sqlx::PgPool;
use std::sync::Arc;
use crate::pipeline::error::PipelineError;
use crate::stages::ingestion::RawPayload;

pub struct JobEnqueuer {
    db_pool: Arc<PgPool>,
}

impl JobEnqueuer {
    pub fn new(db_pool: Arc<PgPool>) -> Self {
        Self { db_pool }
    }

    /// Enqueues a case payload into Postgres for durable background processing.
    pub async fn enqueue(
        &self, 
        trace_id: &str, 
        case_id: &str, 
        payload: &RawPayload
    ) -> Result<(), PipelineError> {
        let payload_json = serde_json::to_value(payload)
            .map_err(|e| PipelineError::Internal(format!("Payload serialization failed: {}", e)))?;

        sqlx::query(
            r#"
            INSERT INTO pipeline_jobs (trace_id, case_id, status, payload_in, created_at)
            VALUES ($1, $2, 'Pending', $3, NOW())
            "#
        )
        .bind(trace_id)
        .bind(case_id)
        .bind(payload_json)
        .execute(&*self.db_pool)
        .await
        .map_err(|e| PipelineError::Internal(format!("Failed to enqueue job: {}", e)))?;

        Ok(())
    }
}
