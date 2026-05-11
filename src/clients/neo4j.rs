use neo4rs::query;
use neo4rs::Graph;
use std::error::Error;
use std::sync::Arc;

pub async fn establish_connection(uri: &str, user: &str, pass: &str) -> Arc<Graph> {
    let formatted_uri = if uri.starts_with("bolt://") || uri.starts_with("neo4j://") {
        uri.to_string()
    } else {
        format!("bolt://{}", uri)
    };

    tracing::info!(
        "╔══ [Neo4j] Connecting ════════════════════════════════════════╗\n  uri={}\n╚══════════════════════════════════════════════════════════════╝",
        formatted_uri
    );

    let graph = Graph::new(&formatted_uri, user, pass)
        .await
        .expect("Failed to connect to Neo4j. Is the Docker container running?");

    tracing::info!(
        "╔══ [Neo4j] Connection established ════════════════════════════╗\n  uri={}\n╚══════════════════════════════════════════════════════════════╝",
        formatted_uri
    );

    Arc::new(graph)
}

/// Given substance/generic names from NRCES, find the matching snomed_ids
/// that actually exist inside the BODHI graph (bodhi-m Drug/Concept nodes).
/// Returns deduplicated snomed_id strings ready for pathway queries.
pub async fn resolve_bodhi_snomed_ids(
    graph: &Graph,
    terms: &[String],          // substance_names + generic_names + raw diagnoses
) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
    if terms.is_empty() {
        return Ok(vec![]);
    }

    // Each term is searched independently; we collect all matches.
    // BODHI-m Drug and Concept nodes both carry `snomed_id` and `synonyms`.
    // BODHI-s Condition/Symptom nodes also carry `snomed_id` and `name`.
    let cypher = "
        UNWIND $terms AS term
        OPTIONAL MATCH (d:Drug)
            WHERE toLower(d.name) CONTAINS toLower(term)
               OR d.synonyms CONTAINS term
        OPTIONAL MATCH (c:Concept)
            WHERE toLower(c.name) CONTAINS toLower(term)
               OR toLower(c.display_name) CONTAINS toLower(term)
               OR c.synonyms CONTAINS term
        OPTIONAL MATCH (cond:Condition)
            WHERE toLower(cond.name) CONTAINS toLower(term)
        OPTIONAL MATCH (s:Symptom)
            WHERE toLower(s.name) CONTAINS toLower(term)
        WITH
            collect(d.snomed_id) + collect(c.snomed_id) +
            collect(cond.snomed_id) + collect(s.snomed_id) AS all_ids
        UNWIND all_ids AS sid
        WITH sid WHERE sid IS NOT NULL
        RETURN DISTINCT sid
        LIMIT 30
    ";

    tracing::info!(
        "┌── [Neo4j ▶ SEND] resolve_bodhi_snomed_ids ─────────────────\n│  Resolving {} term(s) to BODHI snomed_ids\n│  terms={:?}\n└────────────────────────────────────────────────────────────",
        terms.len(), terms
    );

    let mut result = graph.execute(
        neo4rs::query(cypher).param("terms", terms.to_vec())
    ).await?;

    let mut ids: Vec<String> = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        if let Ok(sid) = row.get::<String>("sid") {
            ids.push(sid);
        }
    }

    tracing::info!(
        "└── [Neo4j ◀ RECV] resolve_bodhi_snomed_ids ─────────────────\n│  {} BODHI snomed_id(s) resolved: {:?}\n└────────────────────────────────────────────────────────────",
        ids.len(), ids
    );

    Ok(ids)
}

/// Checks if a Drug (snomed_id = med_id) has an IMPACTS edge to a Concept/Condition (snomed_id = diag_id).
/// BODHI-M: (Drug)-[:IMPACTS]->(Concept)
/// Returns true if the graph confirms medical necessity.
pub async fn check_medical_necessity(
    graph: &Graph,
    diag_id: &str,
    med_id: &str,
) -> Result<bool, Box<dyn Error>> {
    let cypher = "
        MATCH (m:Drug {snomed_id: $med_id})-[:IMPACTS]->(d:Concept {snomed_id: $diag_id})
        RETURN m.snomed_id AS med LIMIT 1
    ";

    tracing::info!(
        "┌── [Neo4j ▶ SEND] check_medical_necessity ──────────────────\n│  Cypher: MATCH (m:Drug {{snomed_id:$med_id}})-[:IMPACTS]->(d:Concept {{snomed_id:$diag_id}})\n│  Params: med_id='{}' diag_id='{}'\n└────────────────────────────────────────────────────────────",
        med_id,
        diag_id
    );

    let mut result = graph
        .execute(
            query(cypher)
                .param("diag_id", diag_id.to_string())
                .param("med_id", med_id.to_string()),
        )
        .await?;

    match result.next().await {
        Ok(Some(_)) => {
            tracing::info!(
                "└── [Neo4j ◀ RECV] check_medical_necessity ──────────────────\n│  ✅ VALIDATED: Drug({}) -[:IMPACTS]-> Concept({})\n└────────────────────────────────────────────────────────────",
                med_id,
                diag_id
            );
            Ok(true)
        }
        Ok(None) => {
            tracing::warn!(
                "└── [Neo4j ◀ RECV] check_medical_necessity ──────────────────\n│  ❌ NOT FOUND: no IMPACTS edge between Drug({}) and Concept({})\n└────────────────────────────────────────────────────────────",
                med_id,
                diag_id
            );
            Ok(false)
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Fetches all direct relationships where BOTH endpoints are in the resolved concept set.
/// Covers BODHI-M (Concept/Drug/LabInvestigation) and BODHI-S (Condition/Symptom/Speciality).
/// Returns Vec of (source_snomed_id, relationship_type, target_snomed_id).
pub async fn fetch_deterministic_pathways(
    graph: &neo4rs::Graph,
    concept_ids: Vec<String>,
) -> Result<Vec<(String, String, String)>, String> {
    if concept_ids.is_empty() {
        return Ok(vec![]);
    }

    let q = "
        UNWIND $ids AS id
        MATCH p = (a)-[*1..8]->(b)
        WHERE (a:Drug OR a:Concept OR a:Condition OR a:Symptom OR a:LabInvestigation)
          AND (b:Drug OR b:Concept OR b:Condition OR b:Symptom OR b:LabInvestigation)
          AND a.snomed_id IN $ids
          AND b.snomed_id IN $ids
          AND a <> b
        UNWIND relationships(p) AS r
        WITH startNode(r) AS src, r, endNode(r) AS tgt
        RETURN src.name AS from_name, type(r) AS rel, tgt.name AS to_name,
               src.snomed_id AS from_id, tgt.snomed_id AS to_id
        LIMIT 100
    ";

    tracing::info!(
        "┌── [Neo4j ▶ SEND] fetch_deterministic_pathways ─────────────\n│  Cypher: MATCH (a)-[*1..8]->(b) WHERE both ends IN ids\n│  Params: ids={:?}\n└────────────────────────────────────────────────────────────",
        concept_ids
    );

    let mut result = graph
        .execute(query(q).param("ids", concept_ids.clone()))
        .await
        .map_err(|e| e.to_string())?;

    let mut pathways = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        let source: String = row.get("from_id").unwrap_or_default();
        let relation: String = row.get("rel").unwrap_or_default();
        let target: String = row.get("to_id").unwrap_or_default();
        pathways.push((source, relation, target));
    }

    if pathways.is_empty() {
        tracing::warn!(
            "└── [Neo4j ◀ RECV] fetch_deterministic_pathways ─────────────\n│  ⚠️  0 pathways found for ids={:?}\n└────────────────────────────────────────────────────────────",
            concept_ids
        );
    } else {
        let preview: Vec<String> = pathways
            .iter()
            .take(5)
            .map(|(s, r, t)| format!("({}) -[{}]-> ({})", s, r, t))
            .collect();
        tracing::info!(
            "└── [Neo4j ◀ RECV] fetch_deterministic_pathways ─────────────\n│  ✅ {} pathway(s) found\n│  Sample: {}\n└────────────────────────────────────────────────────────────",
            pathways.len(),
            preview.join("\n│         ")
        );
    }

    Ok(pathways)
}

/// Fetches depth-1 neighbors of the resolved concepts.
/// Returns Vec of (source_snomed_id, rel_type, target_snomed_id, source_display_name, target_display_name).
/// Capped at 50 rows to keep prompt size manageable.
pub async fn fetch_entity_neighborhood(
    graph: &neo4rs::Graph,
    concept_ids: Vec<String>,
) -> Result<Vec<(String, String, String, String, String)>, String> {
    if concept_ids.is_empty() {
        return Ok(vec![]);
    }

    let q = "
        UNWIND $ids AS id
        MATCH p = (a)-[r*1..8]-(b)
        WHERE (a:Drug OR a:Concept OR a:Condition OR a:Symptom OR a:LabInvestigation)
          AND a.snomed_id = id
        UNWIND relationships(p) AS edge
        WITH startNode(edge) AS src, edge, endNode(edge) AS tgt
        RETURN src.name AS center_name, src.snomed_id AS center_id,
               type(edge) AS rel, tgt.name AS neighbor_name,
               tgt.snomed_id AS neighbor_id
        LIMIT 50
    ";

    tracing::info!(
        "┌── [Neo4j ▶ SEND] fetch_entity_neighborhood ────────────────\n│  Cypher: MATCH (a)-[*1..8]-(b) WHERE a.snomed_id IN ids LIMIT 50\n│  Params: ids={:?}\n└────────────────────────────────────────────────────────────",
        concept_ids
    );

    let mut result = graph
        .execute(query(q).param("ids", concept_ids.clone()))
        .await
        .map_err(|e| e.to_string())?;

    let mut neighborhood = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        let source_id: String   = row.get("center_id").unwrap_or_default();
        let source_name: String = row.get("center_name").unwrap_or_default();
        let relation: String    = row.get("rel").unwrap_or_default();
        let target_id: String   = row.get("neighbor_id").unwrap_or_default();
        let target_name: String = row.get("neighbor_name").unwrap_or_default();
        neighborhood.push((source_id, relation, target_id, source_name, target_name));
    }

    if neighborhood.is_empty() {
        tracing::warn!(
            "└── [Neo4j ◀ RECV] fetch_entity_neighborhood ────────────────\n│  ⚠️  0 neighbors found for ids={:?}\n└────────────────────────────────────────────────────────────",
            concept_ids
        );
    } else {
        let preview: Vec<String> = neighborhood
            .iter()
            .take(5)
            .map(|(_, rel, _, src, tgt)| format!("[{}] --{}-- [{}]", src, rel, tgt))
            .collect();
        tracing::info!(
            "└── [Neo4j ◀ RECV] fetch_entity_neighborhood ────────────────\n│  ✅ {} neighbor edge(s) found\n│  Sample: {}\n└────────────────────────────────────────────────────────────",
            neighborhood.len(),
            preview.join("\n│         ")
        );
    }

    Ok(neighborhood)
}
