use crate::clients::agentic::AgentClient;
use crate::clients::postgres::{search_dictionary, ClinicalEntity};
use crate::clients;
use deadpool_postgres::Pool;
use std::sync::Arc;
use neo4rs::Graph;
use std::error::Error;

/// Extracts the first substance word from a parenthetical in a drug name.
/// "Cefotroy-SB Forte (ceftriaxone sodium and sulbactam sodium) 1g" → Some("ceftriaxone")
/// "Neomol (paracetamol) 250 mg" → Some("paracetamol")
fn extract_substance_from_parenthetical(name: &str) -> Option<String> {
    let start = name.find('(')?;
    let end = name.find(')')?;
    if end <= start { return None; }
    let inner = &name[start + 1..end];
    let first = inner
        .split(|c: char| c == ' ' || c == ',' || c == '/')
        .next()
        .unwrap_or("")
        .trim();
    if first.len() >= 4 && first.chars().all(|c| c.is_alphabetic()) {
        Some(first.to_lowercase())
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedConcepts {
    /// BODHI-compatible snomed_ids ready for Neo4j queries
    pub bodhi_snomed_ids: Vec<String>,
    /// Original resolved entities (kept for prompt context)
    pub entities: Vec<ClinicalEntity>,
    /// Substance/generic name strings used for BODHI bridge lookup
    pub bridge_terms: Vec<String>,
}

pub async fn resolve_entities(
    pool: &Pool,
    slm: Arc<dyn AgentClient>,
    graph: &Graph,
    raw_terms: Vec<String>,
    unresolved_diagnoses_input: Vec<String>, // We'll pass raw diagnoses from NER here
) -> Result<ResolvedConcepts, Box<dyn Error + Send + Sync>> {
    if raw_terms.is_empty() && unresolved_diagnoses_input.is_empty() {
        return Ok(ResolvedConcepts {
            bodhi_snomed_ids: vec![],
            entities: vec![],
            bridge_terms: vec![],
        });
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
            // Dash-strip fallback: "Pexime-200" → "Pexime", "Cefotroy-SB" → "Cefotroy"
            // Handles brand-name + dosage/variant suffixes joined by a dash.
            let stripped = term
                .split('-')
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| s.len() >= 4 && s != term);

            let mut dash_hit = false;
            if let Some(ref base) = stripped {
                tracing::info!("[Linker] Trying dash-strip fallback: '{}' → '{}'", term, base);
                if let Ok(Some(entity)) = crate::clients::postgres::search_dictionary_prefix(pool, base).await {
                    let graph_id = entity.snomed_id
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| entity.concept_id.as_str())
                        .trim()
                        .to_string();
                    tracing::info!(
                        "[Linker] ✅ Dash-strip hit: '{}' → name='{}' snomed_id='{}'",
                        term, entity.name, graph_id
                    );
                    let mut resolved_entity = entity;
                    resolved_entity.concept_id = graph_id;
                    successful_entities.push(resolved_entity);
                    dash_hit = true;
                }
            }

            if !dash_hit {
                tracing::warn!("[Linker] ❌ DB Miss:  '{}' not found in dictionary", term);
                db_misses.push(term.clone());
            }
        }
    }

    if db_misses.is_empty() {
        tracing::info!(
            "╔══ [Linker] Result ════════════════════════════════════════════╗\n  ✅ All {} terms resolved in Stage 1. Moving to BODHI bridge.\n╚══════════════════════════════════════════════════════════════╝",
            successful_entities.len()
        );
    } else {
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
                return Err(e);
            }
        };

        // Stage 3: Second DB Sweep for Normalized Terms
        tracing::info!("[Linker] ── Stage 3: Postgres re-sweep (normalized terms) ───");
        for (original, normalized) in normalized_map {
            if normalized.trim().is_empty() || normalized.trim().eq_ignore_ascii_case("UNKNOWN") {
                tracing::warn!(
                    "[Linker] ⚠️ Hard Fail: '{}' unmappable (LLM returned '{}'). \
                     Will fall through to BODHI_RESCUE injection.",
                    original, normalized.trim()
                );
                // BODHI_RESCUE: inject raw original term so it at least appears in
                // the neighborhood query attempt via Neo4j text index
                successful_entities.push(crate::clients::postgres::ClinicalEntity {
                    concept_id: original.trim().to_string(),
                    snomed_id: None,
                    term_type: "UNKNOWN_DRUG".to_string(),
                    name: original.trim().to_string(),
                    generic_concept_id: None,
                    substance_name: None,
                    generic_name: None,
                    indication: None,
                    interaction_with_drugs: None,
                });
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
                    substance_name: None,
                    generic_name: None,
                    indication: None,
                    interaction_with_drugs: None,
                });
            }
        }
    }

    // ── Stage 4: BODHI snomed bridge ─────────────────────────────────────────
    // Collect substance_name, generic_name from Postgres hits, plus any raw
    // diagnosis strings that did not hit Postgres, to form the bridge term list.
    let mut bridge_terms: Vec<String> = Vec::new();

    for entity in &successful_entities {
        let mut got_bridge = false;

        if let Some(sub) = &entity.substance_name {
            // strip dosage suffixes: "paracetamol 500 mg" → "paracetamol"
            let base = sub.split_whitespace().next().unwrap_or(sub).to_string();
            if base.len() >= 4 {
                bridge_terms.push(base);
                got_bridge = true;
            }
        }
        if let Some(gen) = &entity.generic_name {
            // take first two words: "cefixime 200 mg oral tablet" → "cefixime"
            let base = gen.split_whitespace().next().unwrap_or(gen).to_string();
            if base.len() >= 4 {
                bridge_terms.push(base);
                got_bridge = true;
            }
        }

        // Parenthetical fallback: parse "(ceftriaxone sodium ...)" from name
        // when both substance_name and generic_name are None in the DB.
        if !got_bridge {
            if let Some(parsed) = extract_substance_from_parenthetical(&entity.name) {
                tracing::info!(
                    "[Linker] Parenthetical fallback: '{}' → bridge='{}'",
                    entity.name, parsed
                );
                bridge_terms.push(parsed);
            }
        }
    }

    // Append raw diagnosis strings directly (these match BODHI Condition names)
    for term in &unresolved_diagnoses_input {
        bridge_terms.push(term.clone());
    }

    bridge_terms.sort();
    bridge_terms.dedup();

    tracing::info!(
        "╠══ [Linker] Stage 4: BODHI bridge — {} terms: {:?}",
        bridge_terms.len(), bridge_terms
    );

    let bodhi_snomed_ids = if bridge_terms.is_empty() {
        vec![]
    } else {
        clients::neo4j::resolve_bodhi_snomed_ids(graph, &bridge_terms)
            .await
            .unwrap_or_else(|e| {
                tracing::error!("[Linker] BODHI bridge lookup failed: {}", e);
                vec![]
            })
    };

    tracing::info!(
        "╚══ [Linker] Resolved {} BODHI snomed_id(s): {:?}",
        bodhi_snomed_ids.len(), bodhi_snomed_ids
    );

    Ok(ResolvedConcepts {
        bodhi_snomed_ids,
        entities: successful_entities,
        bridge_terms,
    })
}
