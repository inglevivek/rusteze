use neo4rs::query;
use neo4rs::Graph;
use std::error::Error;
use std::sync::Arc;

// We return an Arc<Graph> so it can be safely shared across Axum's thread pool
pub async fn establish_connection(uri: &str, user: &str, pass: &str) -> Arc<Graph> {
    // neo4rs requires the uri to have the bolt:// prefix.
    // If it doesn't have it, we append it.
    let formatted_uri = if uri.starts_with("bolt://") || uri.starts_with("neo4j://") {
        uri.to_string()
    } else {
        format!("bolt://{}", uri)
    };

    tracing::info!("Connecting to Neo4j at {}", formatted_uri);
    let graph = Graph::new(&formatted_uri, user, pass)
        .await
        .expect("Failed to connect to Neo4j. Is the Docker container running?");
    Arc::new(graph)
}

pub async fn check_medical_necessity(
    graph: &Graph,
    diag_id: &str,
    med_id: &str,
) -> Result<bool, Box<dyn Error>> {
    // The Blueprint Query: MATCH (d:Diagnosis {id: $diag_id})-[:INDICATED_FOR]->(m:Medication {id: $med_id}) RETURN m
    // We parameterize this to prevent Cypher injection attacks and improve query caching.
    let cypher = "MATCH (d:ClinicalEntity {id: $diag_id})-[r:CLINICAL_RELATION]->(m:ClinicalEntity {id: $med_id}) WHERE r.type = 'INDICATED_FOR' RETURN m LIMIT 1";
    tracing::info!("[Neo4j] Executing query: {} with diag_id: {}, med_id: {}", cypher, diag_id, med_id);

    let mut result = graph
        .execute(
            query(cypher)
                .param("diag_id", diag_id.to_string())
                .param("med_id", med_id.to_string()),
        )
        .await?;

    // If we get back at least one row, the relationship exists. It is medically necessary.
    match result.next().await {
        Ok(Some(_row)) => {
            tracing::info!("[Graph] VALIDATED: {} is indicated for {}", med_id, diag_id);
            Ok(true)
        }
        Ok(None) => {
            tracing::warn!(
                "[Graph] CONFLICT: {} is NOT indicated for {}",
                med_id,
                diag_id
            );
            Ok(false)
        }
        Err(e) => Err(Box::new(e)),
    }
}

pub async fn fetch_entity_neighborhood(
    graph: &neo4rs::Graph,
    concept_ids: Vec<String>,
) -> Result<Vec<(String, String, String)>, String> {
    if concept_ids.is_empty() {
        return Ok(vec![]);
    }

    // This query pulls the 'Textbook Knowledge' for each entity.
    // It looks for any immediate neighbors (depth 1) of the extracted concepts.
    let q = "
        MATCH (a:ClinicalEntity)-[r:CLINICAL_RELATION]-(b:ClinicalEntity)
        WHERE a.id IN $ids OR b.id IN $ids
        RETURN a.id AS source, r.type AS relation, b.id AS target
        LIMIT 50
    ";
    tracing::info!("[Neo4j] Executing query: {} with ids: {:?}", q, concept_ids);

    let mut result = graph
        .execute(query(q).param("ids", concept_ids))
        .await
        .map_err(|e| e.to_string())?;

    let mut pathways = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        let source: String = row.get("source").unwrap_or_default();
        let relation: String = row.get("relation").unwrap_or_default();
        let target: String = row.get("target").unwrap_or_default();
        pathways.push((source, relation, target));
    }

    Ok(pathways)
}

pub async fn fetch_deterministic_pathways(
    graph: &neo4rs::Graph,
    concept_ids: Vec<String>,
) -> Result<Vec<(String, String, String)>, String> {
    if concept_ids.is_empty() {
        return Ok(vec![]);
    }

    // This query is 'Hybrid-Aware': It resolves nodes from both the Bodhi triples (using .id)
    // and the Drug Code graph (using .identifier).
    let q = "
        MATCH (a:ClinicalEntity)-[r:CLINICAL_RELATION]->(b:ClinicalEntity)
        WHERE a.id IN $ids AND b.id IN $ids
        RETURN a.id AS source, r.type AS relation, b.id AS target
    ";
    tracing::info!("[Neo4j] Executing query: {} with ids: {:?}", q, concept_ids);

    let mut result = graph
        .execute(query(q).param("ids", concept_ids))
        .await
        .map_err(|e| e.to_string())?;

    let mut pathways = Vec::new();
    while let Ok(Some(row)) = result.next().await {
        let source: String = row.get("source").unwrap_or_default();
        let relation: String = row.get("relation").unwrap_or_default();
        let target: String = row.get("target").unwrap_or_default();
        pathways.push((source, relation, target));
    }

    Ok(pathways)
}
