use crate::clients::agentic::AgentClient;
use crate::clients::postgres::{search_dictionary, ClinicalEntity};
use deadpool_postgres::Pool;
use std::sync::Arc;

pub async fn resolve_entities(
    pool: &Pool,
    slm: Arc<dyn AgentClient>,
    raw_terms: Vec<String>,
) -> Vec<ClinicalEntity> {
    if raw_terms.is_empty() {
        return vec![];
    }

    let mut successful_entities = Vec::new();
    let mut db_misses = Vec::new();

    // Stage 1: Initial Bulk DB Sweep (Sequential but fast point-lookups)
    for term in &raw_terms {
        let result = search_dictionary(pool, term).await.unwrap_or(None);
        if let Some(entity) = result {
            // Resolve commercial -> chemical
            let resolved_id = entity.generic_concept_id.clone().unwrap_or(entity.concept_id.clone());
            tracing::info!("[Linker] Fast Hit: '{}' -> {} (type: {})", term, resolved_id, entity.term_type);
            
            let mut resolved_entity = entity;
            resolved_entity.concept_id = resolved_id;
            successful_entities.push(resolved_entity);
        } else {
            db_misses.push(term.clone());
        }
    }

    if db_misses.is_empty() {
        return successful_entities;
    }

    // Stage 2: Bulk LLM Normalization
    tracing::warn!(
        "[Linker] DB Miss for {} terms. Waking up Nano-Agent for bulk normalization...",
        db_misses.len()
    );

    let normalized_map = match slm.normalize_terms(&db_misses).await {
        Ok(map) => map,
        Err(e) => {
            tracing::error!("[Linker] Nano-Agent Bulk Error: {}", e);
            return successful_entities;
        }
    };

    // Stage 3: Second DB Sweep for Normalized Terms
    for (original, normalized) in normalized_map {
        if normalized.trim().is_empty() {
            tracing::error!("[Linker] Hard Fail: Entity '{}' was completely unmappable.", original);
            continue;
        }

        tracing::info!("[Linker] Nano-Agent mapped '{}' to '{}'", original, normalized);
        
        let result = search_dictionary(pool, &normalized).await.unwrap_or(None);
        
        if let Some(entity) = result {
            // Resolve commercial -> chemical
            let resolved_id = entity.generic_concept_id.clone().unwrap_or(entity.concept_id.clone());
            tracing::info!(
                "[Linker] Recovered: '{}' -> '{}' -> {} (type: {})",
                original,
                normalized,
                resolved_id,
                entity.term_type
            );
            
            let mut resolved_entity = entity;
            resolved_entity.concept_id = resolved_id;
            successful_entities.push(resolved_entity);
        } else {
            tracing::error!(
                "[Linker] Hard Fail: Normalized entity '{}' (from '{}') does not exist in vocabulary.",
                normalized,
                original
            );
        }
    }

    successful_entities
}
