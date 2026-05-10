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
        MATCH (a)-[r]->(b)
        WHERE
          (a:Concept OR a:Condition OR a:Drug OR a:LabInvestigation OR a:Symptom)
          AND
          (b:Concept OR b:Condition OR b:Drug OR b:LabInvestigation OR b:Symptom OR b:Speciality)
          AND a.snomed_id IN $ids
          AND b.snomed_id IN $ids
        RETURN
          a.snomed_id AS source,
          type(r)     AS relation,
          b.snomed_id AS target
    ";

    tracing::info!(
        "┌── [Neo4j ▶ SEND] fetch_deterministic_pathways ─────────────\n│  Cypher: MATCH (a)-[r]->(b) WHERE both ends IN ids\n│  Params: ids={:?}\n└────────────────────────────────────────────────────────────",
        concept_ids
    );

    let mut result = graph
        .execute(query(q).param("ids", concept_ids.clone()))
        .await
        .map_err(|e| e.to_string())?;

    let mut pathways = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        let source: String = row.get("source").unwrap_or_default();
        let relation: String = row.get("relation").unwrap_or_default();
        let target: String = row.get("target").unwrap_or_default();
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
        MATCH (a)-[r]-(b)
        WHERE
          (a:Concept OR a:Condition OR a:Drug OR a:LabInvestigation OR a:Symptom)
          AND a.snomed_id IN $ids
        RETURN
          a.snomed_id                          AS source_id,
          coalesce(a.display_name, a.name)     AS source_name,
          type(r)                              AS relation,
          coalesce(b.snomed_id, b.id, '')    AS target_id,
          coalesce(b.display_name, b.name, b.id, '') AS target_name
        LIMIT 50
    ";

    tracing::info!(
        "┌── [Neo4j ▶ SEND] fetch_entity_neighborhood ────────────────\n│  Cypher: MATCH (a)-[r]-(b) WHERE a.snomed_id IN ids LIMIT 50\n│  Params: ids={:?}\n└────────────────────────────────────────────────────────────",
        concept_ids
    );

    let mut result = graph
        .execute(query(q).param("ids", concept_ids.clone()))
        .await
        .map_err(|e| e.to_string())?;

    let mut neighborhood = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        let source_id: String   = row.get("source_id").unwrap_or_default();
        let source_name: String = row.get("source_name").unwrap_or_default();
        let relation: String    = row.get("relation").unwrap_or_default();
        let target_id: String   = row.get("target_id").unwrap_or_default();
        let target_name: String = row.get("target_name").unwrap_or_default();
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
