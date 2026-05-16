use std::sync::Arc;
use neo4rs::Graph;
use deadpool_postgres::Pool;
use crate::clients::agentic::AgentClient;

pub mod agent;
pub mod api;
pub mod clients;
pub mod config;
pub mod extraction;
pub mod ingestion;
pub mod knowledge;
pub mod observability;
pub mod pipeline;
pub mod resolver;
pub mod stages;

#[derive(Clone)]
pub struct AppState {
    pub config: config::Config,
    pub neo4j_client: Arc<Graph>,
    pub pg_pool: Arc<Pool>,
    pub main_llm: Arc<dyn AgentClient>,
    pub slm: Arc<dyn AgentClient>,
    pub ner_llm: Arc<dyn AgentClient>,
    pub openai_client: Arc<rig::providers::openai::Client>,
    pub qdrant_client: Arc<qdrant_client::Qdrant>,
    pub pipeline_bus: Arc<crate::pipeline::bus::PipelineEventBus>,
    pub sqlx_pool: Arc<sqlx::PgPool>,
}
