use serde_json::Value;

pub fn emit_stage_event(
    stage: &str,
    event_type: &str,
    target: Option<&str>,
    payload: Value,
    metadata: Value,
) {
    tracing::info!(
        stage = stage,
        event_type = event_type,
        target = target.unwrap_or("unknown"),
        payload = %payload,
        metadata = %metadata,
        "pipeline_event"
    );
}
