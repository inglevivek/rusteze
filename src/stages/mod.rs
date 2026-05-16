pub mod ingestion;
pub mod extraction;
pub mod resolution;
pub mod planner;
pub mod retrieval;
pub mod report;

pub use ingestion::{IngestionStage, RawPayload, IngestedDocument};
pub use extraction::{ExtractionStage, CaseFacts};
pub use resolution::ResolutionStage;
pub use planner::{QueryPlannerStage, PlannedEvidenceRequest, CypherValidator};
pub use retrieval::{RetrievalStage, EvidencePacket};
pub use report::{ReportGenerationStage, ReportInput, FinalReport};
