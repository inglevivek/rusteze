use async_trait::async_trait;
use super::error::PipelineError;

/// Core contract for all pipeline stages.
/// Forces decoupled input (I) and output (O) boundaries.
#[async_trait]
pub trait PipelineStage<I, O> 
where
    I: Send + Sync,
    O: Send + Sync,
{
    /// Returns the static identifier of the stage for observability routing.
    fn name(&self) -> &'static str;
    
    /// Executes the stage logic.
    async fn execute(&self, input: I) -> Result<O, PipelineError>;
}
