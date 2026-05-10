use crate::clients::{agentic::AgentClient, neo4j, qdrant};
use crate::knowledge::linker;
use neo4rs::Graph;
use std::collections::HashMap;
use std::sync::Arc;

pub async fn build_grounded_context(
    ner_llm: Arc<dyn AgentClient>,
    slm: Arc<dyn AgentClient>,
    graph: Arc<Graph>,
    pg_pool: Arc<deadpool_postgres::Pool>,
    case: &crate::clients::postgres::Case,
    qdrant_url: &str,
    embedding_url: &str,
    _rag_chunks: &[String], // Kept for signature but NER uses case.document_text
) -> String {
    tracing::info!("[PromptBuilder] Starting context build...");

    // 1. NER: extract medications and diagnoses from the FULL document
    // NER always runs on the complete case document, not just retrieved chunks.
    // RAG chunks may be lab pages with no drug names; the full document always
    // contains the medication sheet.
    let ner_input = &case.document_text;
    let extracted_json = match ner_llm.extract_entities(ner_input).await {
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
    let mut diagnoses_strings: Vec<String> = Vec::new();

    for m in &medications {
        if let Some(s) = m.as_str() {
            let s = s.trim();
            if !s.is_empty() { raw_terms.push(s.to_string()); }
        }
    }
    for d in &diagnoses {
        if let Some(s) = d.as_str() {
            let s = s.trim();
            if !s.is_empty() {
                raw_terms.push(s.to_string());
                diagnoses_strings.push(s.to_string());
            }
        }
    }

    // 2. Resolve to snomed_ids via Postgres dictionary + LLM normalization fallback
    // The linker now returns ResolvedConcepts which includes bridged BODHI IDs.
    let resolved = match linker::resolve_entities(&pg_pool, slm.clone(), &graph, raw_terms, diagnoses_strings).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("[PromptBuilder] Linker failed: {}", e);
            linker::ResolvedConcepts {
                bodhi_snomed_ids: vec![],
                entities: vec![],
                bridge_terms: vec![],
            }
        }
    };

    let mut bodhi_snomed_ids = resolved.bodhi_snomed_ids;
    let mut id_to_name: HashMap<String, String> = HashMap::new();
    let mut entity_names: Vec<String> = Vec::new();

    for entity in &resolved.entities {
        id_to_name.insert(entity.concept_id.clone(), entity.name.clone());
        entity_names.push(entity.name.clone());
    }

    // 3. Keyword dictionary sweep fallback if linker returned nothing
    if bodhi_snomed_ids.is_empty() {
        tracing::warn!("[PromptBuilder] Linker returned nothing. Running hardened keyword sweep...");

        // ── Clinical stopwords — never query these against the drug dictionary ───
        const CLINICAL_STOPWORDS: &[&str] = &[
            // lab / report words
            "PROGNOSIS","VITALS","DECREASED","INCREASED","ABSENT","PRESENT","NORMAL",
            "ABNORMAL","TOTALWBC","TOTALRBC","EOSINOPHILS","BASOPHILS","MONOCYTES",
            "NEUTROPHILS","LYMPHOCYTES","PLATELETS","PROTEINS","SUGAR","COLOR","STOOL",
            "URINE","BLOOD","SERUM","PLASMA","PACKED","MATERIAL","AMORPHOUS","BODIES",
            "SPECIMEN","SPUTUM","CULTURE","SENSITIVITY","FINDINGS","RESULT","REPORT",
            // clinical verbs / generic words
            "SINCE","ABOVE","GIVEN","TAKEN","NOTED","STARTED","STOPPED","ADVISED",
            "PATIENT","HISTORY","COMPLAINT","DIAGNOSIS","TREATMENT","ADMISSION","DISCHARGE",
            "MEDICINE","ACCORDINGLY","SUBJECTED","ANALYSIS","PROGNOSIS","EXAMINATION",
            "SUBJECTED","INVESTIGATION","MANAGEMENT","FOLLOW","REVIEW","REFERRED",
            // OCR noise
            "MITCHATION","PATHOLOST","ACCORDINGLY",
        ];

        fn is_valid_sweep_term(term: &str) -> bool {
            let upper = term.to_uppercase();
            // Must be at least 6 chars
            if term.len() < 6 { return false; }
            // Must not be a stopword
            if CLINICAL_STOPWORDS.contains(&upper.as_str()) { return false; }
            // Must start with a letter (no purely numeric terms)
            if !term.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) { return false; }
            // Must not be all lowercase common English (heuristic: if all lowercase
            // and less than 8 chars, skip — real brand names are usually ALL CAPS or TitleCase)
            if term.chars().all(|c| c.is_lowercase()) && term.len() < 8 { return false; }
            true
        }

        // Build candidate terms from the FULL document text
        let candidate_terms: Vec<String> = case.document_text
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| is_valid_sweep_term(w))
            .collect::<std::collections::HashSet<_>>()   // deduplicate
            .into_iter()
            .collect();

        tracing::info!(
            "[PromptBuilder] Keyword sweep: {} candidate terms after filtering",
            candidate_terms.len()
        );

        let mut sweep_entities: Vec<crate::clients::postgres::ClinicalEntity> = Vec::new();
        for term in &candidate_terms {
            // Use PREFIX-only matching (term% not %term%) to prevent substring pollution
            if let Ok(Some(entity)) = crate::clients::postgres::search_dictionary_prefix(&pg_pool, term).await {
                tracing::info!(
                    "[PromptBuilder] Sweep hit: {:?} → name='{}' substance={:?}",
                    term, entity.name, entity.substance_name
                );
                sweep_entities.push(entity);
                if sweep_entities.len() >= 10 { break; }  // cap at 10 hits
            }
        }

        if !sweep_entities.is_empty() {
            // Bridge sweep results to BODHI IDs
            let mut bridge_terms: Vec<String> = Vec::new();
            for entity in &sweep_entities {
                if let Some(sub) = &entity.substance_name {
                    let base = sub.split_whitespace().next().unwrap_or(sub).to_string();
                    if base.len() >= 4 { bridge_terms.push(base); }
                }
                if let Some(gen) = &entity.generic_name {
                    let base = gen.split_whitespace().next().unwrap_or(gen).to_string();
                    if base.len() >= 4 { bridge_terms.push(base); }
                }
                id_to_name.insert(entity.concept_id.clone(), entity.name.clone());
                entity_names.push(entity.name.clone());
            }
            bridge_terms.sort();
            bridge_terms.dedup();

            if !bridge_terms.is_empty() {
                if let Ok(ids) = crate::clients::neo4j::resolve_bodhi_snomed_ids(&graph, &bridge_terms).await {
                    bodhi_snomed_ids.extend(ids);
                    bodhi_snomed_ids.sort();
                    bodhi_snomed_ids.dedup();
                }
            }
        }
    }

    let mut neo4j_context = String::new();

    if !bodhi_snomed_ids.is_empty() {
        tracing::info!(
            "[PromptBuilder] Firing Neo4j with {} BODHI snomed_ids: {:?}",
            bodhi_snomed_ids.len(),
            bodhi_snomed_ids
        );

        // 4. Deterministic pathways: direct edges between the case's own concepts
        match neo4j::fetch_deterministic_pathways(&graph, bodhi_snomed_ids.clone()).await {
            Ok(pathways) if !pathways.is_empty() => {
                neo4j_context.push_str("### DETERMINISTIC GRAPH PATHWAYS (Neo4j):\n");
                for (src, rel, tgt) in &pathways {
                    let src_name = id_to_name.get(src).unwrap_or(src);
                    let tgt_name = id_to_name.get(tgt).unwrap_or(tgt);
                    neo4j_context.push_str(&format!("- [{}] -[{}]-> [{}]\n", src_name, rel, tgt_name));
                }
                neo4j_context.push('\n');
            }
            Ok(_) => tracing::warn!("[PromptBuilder] Pathways query returned 0 rows for ids={:?}", bodhi_snomed_ids),
            Err(e) => tracing::error!("[Neo4j] fetch_deterministic_pathways failed: {}", e),
        }

        // 5. Textbook neighborhood: depth-1 neighbors for medical context
        match neo4j::fetch_entity_neighborhood(&graph, bodhi_snomed_ids.clone()).await {
            Ok(neighborhood) if !neighborhood.is_empty() => {
                neo4j_context.push_str("### TEXTBOOK MEDICAL KNOWLEDGE (Neo4j Neighborhood):\n");
                for (_, rel, _, src_name, tgt_name) in neighborhood.iter().take(15) {
                    neo4j_context.push_str(&format!("- [{}] --({})-- [{}]\n", src_name, rel, tgt_name));
                }
                neo4j_context.push('\n');
            }
            Ok(_) => tracing::warn!("[PromptBuilder] Neighborhood query returned 0 rows for ids={:?}", bodhi_snomed_ids),
            Err(e) => tracing::error!("[Neo4j] fetch_entity_neighborhood failed: {}", e),
        }
    } else {
        tracing::warn!("[PromptBuilder] No BODHI snomed_ids resolved. Skipping all Neo4j queries.");
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
