use std::sync::Arc;
use std::time::Duration;
use sqlx::PgPool;
use tokio::time::interval;
use crate::pipeline::bus::PipelineEventBus;
use crate::pipeline::events::PipelineEvent;

pub struct BatchedTelemetrySink {
    bus: Arc<PipelineEventBus>,
    db_pool: Arc<PgPool>,
}

impl BatchedTelemetrySink {
    const MAX_BATCH_SIZE: usize = 100;
    const FLUSH_INTERVAL_SECS: u64 = 3;

    pub fn new(bus: Arc<PipelineEventBus>, db_pool: Arc<PgPool>) -> Self {
        Self { bus, db_pool }
    }

    pub fn spawn_listener(&self) {
        let mut receiver = self.bus.subscribe();
        let pool = self.db_pool.clone();

        tokio::spawn(async move {
            let mut batch = Vec::with_capacity(Self::MAX_BATCH_SIZE);
            let mut flush_interval = interval(Duration::from_secs(Self::FLUSH_INTERVAL_SECS));

            loop {
                tokio::select! {
                    // 1. Listen for new events
                    result = receiver.recv() => {
                        match result {
                            Ok(event) => {
                                batch.push(event);
                                if batch.len() >= Self::MAX_BATCH_SIZE {
                                    Self::flush_batch(&mut batch, &pool).await;
                                }
                            }
                            Err(_) => {
                                // Channel lag or close. If lag, we might want to continue, 
                                // but for simplicity and safety (as per spec), we handle flush and break.
                                if !batch.is_empty() {
                                    Self::flush_batch(&mut batch, &pool).await;
                                }
                                break;
                            }
                        }
                    }
                    
                    // 2. Flush periodically even if batch isn't full
                    _ = flush_interval.tick() => {
                        if !batch.is_empty() {
                            Self::flush_batch(&mut batch, &pool).await;
                        }
                    }
                }
            }
        });
    }

    async fn flush_batch(batch: &mut Vec<PipelineEvent>, pool: &PgPool) {
        let mut trace_ids = Vec::with_capacity(batch.len());
        let mut case_ids = Vec::with_capacity(batch.len());
        let mut stages = Vec::with_capacity(batch.len());
        let mut payloads_in = Vec::with_capacity(batch.len());
        let mut payloads_out = Vec::with_capacity(batch.len());
        let mut durations = Vec::with_capacity(batch.len());
        let mut tool_calls = Vec::with_capacity(batch.len());

        let batch_size = batch.len();

        for event in batch.drain(..) {
            trace_ids.push(event.trace_id);
            case_ids.push(event.case_id);
            stages.push(event.stage);
            payloads_in.push(serde_json::to_value(&event.payload_in).unwrap_or_default());
            payloads_out.push(serde_json::to_value(&event.payload_out).unwrap_or_default());
            durations.push(event.duration_ms as i64);
            tool_calls.push(serde_json::to_value(&event.tool_calls).unwrap_or_default());
        }

        let query = sqlx::query(
            r#"
            INSERT INTO pipeline_events 
            (trace_id, case_id, stage, payload_in, payload_out, duration_ms, tool_calls, created_at)
            SELECT * FROM UNNEST($1, $2, $3, $4, $5, $6, $7, 
                ARRAY(SELECT NOW() FROM generate_series(1, array_length($1, 1))))
            "#
        )
        .bind(&trace_ids)
        .bind(&case_ids)
        .bind(&stages)
        .bind(&payloads_in)
        .bind(&payloads_out)
        .bind(&durations)
        .bind(&tool_calls);

        if let Err(e) = query.execute(pool).await {
            eprintln!("CRITICAL: BatchedTelemetrySink failed to flush {} events: {}", batch_size, e);
        }
    }
}
