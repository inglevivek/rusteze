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
    embedding_url: &str,
    input_text: &str,
) -> String {
    tracing::info!("[PromptBuilder] Starting context build...");

    // 1. NER: extract medications and diagnoses
    let extracted_json = match main_llm.extract_entities(input_text).await {
        Ok(v) => {
            tracing::debug!("[PromptBuilder] NER response: {}", v);
            v
        }
        Err(e) => {
            tracing::error!("[PromptBuilder] NER call failed: {}", e);
            return "NER failed — check Groq API key and model name in config.".to_string();
        }
    };

    let medications = extracted_json["medications"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let diagnoses = extracted_json["diagnoses"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Fix 3: Schema bleed guard — detect if adjudication schema leaked into NER response
    if medications.is_empty() && diagnoses.is_empty() {
        if extracted_json.get("fraud_risk_misdx_mgmt").is_some()
            || extracted_json.get("case_id").is_some()
            || extracted_json.get("decision").is_some()
        {
            tracing::error!(
                "[PromptBuilder] ❗ NER returned ADJUDICATION schema instead of NER schema! \
                 Keys found: {:?}. Check extract_entities system prompt for format_instructions bleed.",
                extracted_json.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>())
            );
            return "NER schema mismatch — adjudication prompt contaminating NER call.".to_string();
        }
    }

    let mut raw_terms: Vec<String> = Vec::new();
    for m in &medications {
        if let Some(s) = m.as_str() {
            let s = s.trim();
            if !s.is_empty() { raw_terms.push(s.to_string()); }
        }
    }
    for d in &diagnoses {
        if let Some(s) = d.as_str() {
            let s = s.trim();
            if !s.is_empty() { raw_terms.push(s.to_string()); }
        }
    }

    // 2. Resolve to snomed_ids via Postgres dictionary + LLM normalization fallback
    let mut concept_ids: Vec<String> = Vec::new();
    let mut id_to_name: HashMap<String, String> = HashMap::new();
    let mut entity_names: Vec<String> = Vec::new();

    let resolved_entities = linker::resolve_entities(&pg_pool, slm.clone(), raw_terms).await;

    for entity in &resolved_entities {
        if !entity.concept_id.is_empty() {
            concept_ids.push(entity.concept_id.clone());
            id_to_name.insert(entity.concept_id.clone(), entity.name.clone());
            entity_names.push(entity.name.clone());
        }
    }

    // 3. Keyword dictionary sweep fallback if linker returned nothing
    if concept_ids.is_empty() {
        tracing::warn!("[PromptBuilder] Linker returned nothing. Running filtered keyword sweep...");

        // Fix 2: Stopword-filtered, deduped clinical keyword sweep
        // Only pass terms that could plausibly be in the clinical dictionary.
        const SWEEP_STOPWORDS: &[&str] = &[
            "PATIENT", "HOSPITAL", "CERTIFIED", "LABORATORY", "GENDER",
            "REPORTING", "RESULT", "NORMAL", "RANGE", "COUNT", "VOLUME",
            "DOCUMENT", "INCLUDING", "EXAMINATION", "ANALYSIS", "UNNAMED",
            "CELLS", "MILLION", "FLUID", "SMEAR", "DOCTOR", "REPORT",
            "TOTAL", "LEVEL", "VALUE", "UNITS", "PATHOLOGY", "SAMPLE",
            "PATIENT", "FEMALE", "MALE", "BLOOD", "URINE", "SERUM",
        ];

        let sweep_terms: Vec<String> = input_text
            .split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|s| s.len() >= 5)                                    // drop short tokens
            .filter(|s| s.chars().all(|c| c.is_alphabetic()))           // drop numeric/unit tokens
            .filter(|s| !SWEEP_STOPWORDS.contains(&s.to_uppercase().as_str()))  // drop clinical noise
            .collect::<std::collections::HashSet<_>>()                   // dedup
            .into_iter()
            .collect();

        tracing::info!("[PromptBuilder] Keyword sweep: {} candidate terms after filtering", sweep_terms.len());

        for word in sweep_terms.iter().take(20) {
            if let Ok(Some(entity)) = crate::clients::postgres::search_dictionary(&pg_pool, word).await {
                let graph_id = entity.snomed_id
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(entity.concept_id.as_str())
                    .trim()
                    .to_string();
                if !concept_ids.contains(&graph_id) {
                    tracing::info!("[PromptBuilder] Sweep hit: '{}' → snomed_id='{}'", word, graph_id);
                    concept_ids.push(graph_id.clone());
                    id_to_name.insert(graph_id.clone(), entity.name.clone());
                    entity_names.push(entity.name.clone());
                }
            }
        }
    }

    let mut neo4j_context = String::new();

    if !concept_ids.is_empty() {
        // 4. Deterministic pathways: direct edges between the case's own concepts
        match neo4j::fetch_deterministic_pathways(&graph, concept_ids.clone()).await {
            Ok(pathways) if !pathways.is_empty() => {
                neo4j_context.push_str("### DETERMINISTIC GRAPH PATHWAYS (Neo4j):\n");
                for (src, rel, tgt) in &pathways {
                    let src_name = id_to_name.get(src).unwrap_or(src);
                    let tgt_name = id_to_name.get(tgt).unwrap_or(tgt);
                    neo4j_context.push_str(&format!("- [{}] -[{}]-> [{}]\n", src_name, rel, tgt_name));
                }
                neo4j_context.push('\n');
            }
            Ok(_) => tracing::warn!("[PromptBuilder] Pathways query returned 0 rows for ids={:?}", concept_ids),
            Err(e) => tracing::error!("[Neo4j] fetch_deterministic_pathways failed: {}", e),
        }

        // 5. Textbook neighborhood: depth-1 neighbors for medical context
        match neo4j::fetch_entity_neighborhood(&graph, concept_ids.clone()).await {
            Ok(neighborhood) if !neighborhood.is_empty() => {
                neo4j_context.push_str("### TEXTBOOK MEDICAL KNOWLEDGE (Neo4j Neighborhood):\n");
                for (_, rel, _, src_name, tgt_name) in neighborhood.iter().take(15) {
                    neo4j_context.push_str(&format!("- [{}] --({})-- [{}]\n", src_name, rel, tgt_name));
                }
                neo4j_context.push('\n');
            }
            Ok(_) => tracing::warn!("[PromptBuilder] Neighborhood query returned 0 rows for ids={:?}", concept_ids),
            Err(e) => tracing::error!("[Neo4j] fetch_entity_neighborhood failed: {}", e),
        }
    } else {
        tracing::warn!("[PromptBuilder] No concept_ids resolved. Skipping all Neo4j queries.");
    }

    // 6. Semantic context from Qdrant global knowledge base
    let qdrant_context = if !entity_names.is_empty() {
        let global_query = format!("Medical facts regarding: {}", entity_names.join(", "));
        match qdrant::search_global_knowledge(qdrant_url, embedding_url, &global_query, 10).await {
            Ok(ctx) => ctx,
            Err(e) => {
                tracing::error!("Qdrant global search failed: {}", e);
                "Semantic context unavailable.".to_string()
            }
        }
    } else {
        "Semantic context unavailable.".to_string()
    };

    format!(
        "{}### SEMANTIC MEDICAL TRUTH (BODHI):\n{}\n",
        neo4j_context, qdrant_context
    )
}
