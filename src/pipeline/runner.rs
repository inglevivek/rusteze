use std::time::Instant;
use serde::Serialize;
use super::bus::PipelineEventBus;
use super::events::{PipelineEvent, ToolCallRecord};
use super::error::PipelineError;
use super::stage::PipelineStage;

/// Executes a pipeline stage and forcefully emits complete I/O telemetry.
pub async fn run_stage_with_telemetry<S, I, O>(
    stage: &S,
    input: I,
    bus: &PipelineEventBus,
    trace_id: String,
    case_id: String,
    tool_calls: Option<Vec<ToolCallRecord>>,
) -> Result<O, PipelineError>
where
    S: PipelineStage<I, O>,
    I: Serialize + Send + Sync + Clone,
    O: Serialize + Send + Sync + Clone,
{
    let start_time = Instant::now();
    let stage_name = stage.name().to_string();
    
    // Serialize input exactly as it enters the processing block
    let payload_in = serde_json::to_value(input.clone())
        .unwrap_or_else(|e| serde_json::json!({ "serialization_error": e.to_string() }));

    // Execute the actual business logic
    let result = stage.execute(input).await;

    let duration_ms = start_time.elapsed().as_millis() as u64;

    match &result {
        Ok(output) => {
            let payload_out = serde_json::to_value(output)
                .unwrap_or_else(|e| serde_json::json!({ "serialization_error": e.to_string() }));

            bus.emit(PipelineEvent {
                trace_id,
                case_id,
                stage: stage_name,
                payload_in,
                payload_out,
                duration_ms,
                tool_calls,
            });
        }
        Err(err) => {
            // Emit failure state for debugging
            bus.emit(PipelineEvent {
                trace_id,
                case_id,
                stage: stage_name,
                payload_in,
                payload_out: serde_json::json!({ "fatal_error": err.to_string() }),
                duration_ms,
                tool_calls,
            });
        }
    }

    result
}
