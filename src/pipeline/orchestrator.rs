use std::sync::Arc;
use crate::AppState; 
use super::bus::PipelineEventBus;
use super::error::PipelineError;
use super::runner::run_stage_with_telemetry;
use crate::stages::{
    ingestion::{IngestionStage, RawPayload},
    extraction::ExtractionStage,
    resolution::ResolutionStage,
    planner::QueryPlannerStage,
    retrieval::RetrievalStage,
    report::{ReportGenerationStage, ReportInput, FinalReport},
};

pub struct DagOrchestrator {
    bus: Arc<PipelineEventBus>,
    app_state: Arc<AppState>,
}

impl DagOrchestrator {
    pub fn new(bus: Arc<PipelineEventBus>, app_state: Arc<AppState>) -> Self {
        Self { bus, app_state }
    }

    pub async fn execute_pipeline(
        &self,
        trace_id: String,
        case_id: String,
        payload: RawPayload,
    ) -> Result<FinalReport, PipelineError> {
        
        let stage1 = IngestionStage::new();
        let stage2 = ExtractionStage::new(self.app_state.openai_client.clone());
        let stage3 = ResolutionStage::new(self.app_state.openai_client.clone());
        let stage4 = QueryPlannerStage::new(); 
        let stage5 = RetrievalStage::new(
            self.app_state.neo4j_client.clone(),
            self.app_state.qdrant_client.clone()
        );
        let stage6 = ReportGenerationStage::new(self.app_state.openai_client.clone());

        let ingested_doc = run_stage_with_telemetry(&stage1, payload, &self.bus, trace_id.clone(), case_id.clone(), None).await?;
        let case_facts = run_stage_with_telemetry(&stage2, ingested_doc, &self.bus, trace_id.clone(), case_id.clone(), None).await?;
        let resolved_facts = run_stage_with_telemetry(&stage3, case_facts, &self.bus, trace_id.clone(), case_id.clone(), None).await?;
        let planned_queries = run_stage_with_telemetry(&stage4, resolved_facts.clone(), &self.bus, trace_id.clone(), case_id.clone(), None).await?;
        let evidence_packet = run_stage_with_telemetry(&stage5, planned_queries, &self.bus, trace_id.clone(), case_id.clone(), None).await?;

        let report_input = ReportInput {
            facts: resolved_facts,
            evidence: evidence_packet,
        };

        let final_report = run_stage_with_telemetry(&stage6, report_input, &self.bus, trace_id, case_id, None).await?;

        Ok(final_report)
    }
}
