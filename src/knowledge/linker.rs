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

    tracing::info!(
        "╔══ [Linker] resolve_entities ═════════════════════════════════╗\n  {} terms to resolve: {:?}\n╚══════════════════════════════════════════════════════════════╝",
        raw_terms.len(),
        raw_terms
    );

    let mut successful_entities = Vec::new();
    let mut db_misses = Vec::new();

    // Stage 1: Initial Bulk DB Sweep (Sequential but fast point-lookups)
    tracing::info!("[Linker] ── Stage 1: Postgres dictionary sweep ──────────────");
    for term in &raw_terms {
        let result = search_dictionary(pool, term).await.unwrap_or(None);
        if let Some(entity) = result {
            let graph_id = entity.snomed_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| entity.concept_id.as_str())
                .trim()
                .to_string();
            tracing::info!(
                "[Linker] ✅ Fast Hit: '{}' → snomed_id='{}' name='{}' type='{}'",
                term,
                graph_id,
                entity.name,
                entity.term_type
            );

            let mut resolved_entity = entity;
            resolved_entity.concept_id = graph_id;
            successful_entities.push(resolved_entity);
        } else {
            tracing::warn!("[Linker] ❌ DB Miss:  '{}' not found in dictionary", term);
            db_misses.push(term.clone());
        }
    }

    if db_misses.is_empty() {
        tracing::info!(
            "╔══ [Linker] Result ════════════════════════════════════════════╗\n  ✅ All {} terms resolved in Stage 1. Skipping LLM normalization.\n╚══════════════════════════════════════════════════════════════╝",
            successful_entities.len()
        );
        return successful_entities;
    }

    // Stage 2: Bulk LLM Normalization
    tracing::warn!(
        "[Linker] ── Stage 2: LLM normalization ─────────────────────────\n  {} terms need normalization: {:?}",
        db_misses.len(),
        db_misses
    );

    let normalized_map = match slm.normalize_terms(&db_misses).await {
        Ok(map) => {
            tracing::info!(
                "[Linker] ✅ LLM returned {} normalized mappings: {:?}",
                map.len(),
                map.iter().map(|(k, v)| format!("'{}' → '{}'", k, v)).collect::<Vec<_>>()
            );
            map
        }
        Err(e) => {
            tracing::error!("[Linker] ❌ LLM normalization failed: {}", e);
            return successful_entities;
        }
    };

    // Stage 3: Second DB Sweep for Normalized Terms
    tracing::info!("[Linker] ── Stage 3: Postgres re-sweep (normalized terms) ───");
    for (original, normalized) in normalized_map {
        if normalized.trim().is_empty() {
            tracing::error!(
                "[Linker] ❌ Hard Fail: '{}' was completely unmappable by LLM.",
                original
            );
            continue;
        }

        tracing::info!("[Linker]    LLM mapped '{}' → '{}'", original, normalized);

        let result = search_dictionary(pool, &normalized).await.unwrap_or(None);

        if let Some(entity) = result {
            let graph_id = entity.snomed_id
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| entity.concept_id.as_str())
                .trim()
                .to_string();
            tracing::info!(
                "[Linker] ✅ Recovered: '{}' → '{}' → snomed_id='{}' type='{}'",
                original,
                normalized,
                graph_id,
                entity.term_type
            );

            let mut resolved_entity = entity;
            resolved_entity.concept_id = graph_id;
            successful_entities.push(resolved_entity);
        } else {
            // Bodhi rescue: synthesize a ClinicalEntity using the normalized name directly
            tracing::warn!(
                "[Linker] 🚨 BODHI_RESCUE: vocab miss for '{}' (normalized from '{}'). Injecting as raw id.",
                normalized,
                original
            );
            successful_entities.push(ClinicalEntity {
                concept_id: normalized.trim().to_string(),
                snomed_id: None,
                term_type: "BODHI_RESCUE".to_string(),
                name: normalized.trim().to_string(),
                generic_concept_id: None,
            });
        }
    }

    tracing::info!(
        "╔══ [Linker] Final Result ══════════════════════════════════════╗\n  {} entities resolved total:\n  {}\n╚══════════════════════════════════════════════════════════════╝",
        successful_entities.len(),
        successful_entities
            .iter()
            .map(|e| format!("  • '{}' (snomed:{}, type:{})", e.name, e.concept_id, e.term_type))
            .collect::<Vec<_>>()
            .join("\n")
    );

    successful_entities
}
