use crate::clients::{agentic::AgentClient, neo4j, qdrant};
use crate::knowledge::linker;
use neo4rs::Graph;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn build_grounded_context(
    main_llm: Arc<dyn AgentClient>,
    slm: Arc<dyn AgentClient>,
    graph: Arc<Graph>,
    pg_pool: Arc<deadpool_postgres::Pool>,
    qdrant_url: &str,
    input_text: &str,
) -> String {
    tracing::info!("[PromptBuilder] Starting context build for input: \n---\n{}\n---", input_text);
    tracing::info!("[PromptBuilder] Extracting entities for grounding...");
    let extracted_json = main_llm
        .extract_entities(input_text)
        .await
        .unwrap_or_default();

    let mut raw_terms = Vec::new();
    if let Some(meds) = extracted_json["medications"].as_array() {
        for m in meds {
            raw_terms.push(m.as_str().unwrap_or("").to_string());
        }
    }
    if let Some(diags) = extracted_json["diagnoses"].as_array() {
        for d in diags {
            raw_terms.push(d.as_str().unwrap_or("").to_string());
        }
    }

    if raw_terms.is_empty() {
        tracing::debug!("[PromptBuilder] Raw NER Output: {:?}", extracted_json);
        tracing::warn!("[PromptBuilder] No entities found to ground.");
        return "No specific topological or semantic context found.".to_string();
    }

    // 2. Resolve strings to concept IDs in bulk
    let mut concept_ids = Vec::new();
    let mut id_to_name = HashMap::new();
    let mut entity_names = Vec::new();

    let resolved_entities = linker::resolve_entities(&pg_pool, slm.clone(), raw_terms).await;
    
    for entity in resolved_entities {
        concept_ids.push(entity.concept_id.clone());
        id_to_name.insert(entity.concept_id.clone(), entity.name.clone());
        entity_names.push(entity.name.clone());
    }

    // FALLBACK: If LLM extraction failed, we sweep for keywords in the text that exist in our dictionary
    if concept_ids.is_empty() {
        tracing::info!("[PromptBuilder] LLM found no entities. Falling back to Keyword Dictionary Sweep...");
        // Simple word-based sweep (limited to avoid perf hit)
        let words: Vec<&str> = input_text.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 4).collect();
        for word in words.iter().take(50) {
            if let Ok(Some(entity)) = crate::clients::postgres::search_dictionary(&pg_pool, word).await {
                let graph_id = {
                    let base = entity.generic_concept_id
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&entity.name);
                    base.trim().to_lowercase()
                };
                if !concept_ids.contains(&graph_id) {
                    concept_ids.push(graph_id.clone());
                    id_to_name.insert(graph_id.clone(), entity.name.clone());
                    entity_names.push(entity.name.clone());
                }
            }
        }
    }

    // 3. Fetch Deterministic Pathways (The 'Current Case' Graph)
    let mut neo4j_context = String::new();
    if !concept_ids.is_empty() {
        match neo4j::fetch_deterministic_pathways(&graph, concept_ids.clone()).await {
            Ok(pathways) => {
                for (src, rel, tgt) in pathways {
                    let src_name = id_to_name.get(&src).unwrap_or(&src);
                    let tgt_name = id_to_name.get(&tgt).unwrap_or(&tgt);
                    neo4j_context.push_str(&format!("- [{}] -> [{}] -> [{}]\n", src_name, rel, tgt_name));
                }
            }
            Err(e) => tracing::error!("Neo4j fetch failed: {}", e),
        }

        // 4. Fetch Textbook Neighborhood (The 'Expert Knowledge' Graph)
        neo4j_context.push_str("\n### TEXTBOOK MEDICAL KNOWLEDGE (Neo4j Neighborhood):\n");
        match neo4j::fetch_entity_neighborhood(&graph, concept_ids).await {
            Ok(neighborhood) => {
                for (src, rel, tgt) in neighborhood.iter().take(15) {
                    let src_name = id_to_name.get(src).unwrap_or(src);
                    let tgt_name = id_to_name.get(tgt).unwrap_or(tgt);
                    neo4j_context.push_str(&format!("- [{}] --({})-- [{}]\n", src_name, rel, tgt_name));
                }
            }
            Err(e) => tracing::error!("Neo4j neighborhood fetch failed: {}", e),
        }
    }

    // Fetch Semantic Truth (Qdrant)
    let qdrant_context = if !entity_names.is_empty() {
        let global_search_query = format!("Medical facts regarding: {}", entity_names.join(", "));
        match qdrant::search_global_knowledge(qdrant_url, &global_search_query, 10).await {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::error!("Qdrant global search failed: {}", e);
                "Semantic context unavailable.".to_string()
            }
        }
    } else {
        "Semantic context unavailable.".to_string()
    };

    // Synthesize the output
    format!(
        "### DETERMINISTIC GRAPH PATHWAYS (Neo4j):\n{}\n### SEMANTIC MEDICAL TRUTH (BODHI):\n{}\n",
        neo4j_context, qdrant_context
    )
}
