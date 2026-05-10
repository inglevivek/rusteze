use crate::clients::encoder::BioLordEncoder;
use qdrant_client::{
    qdrant::{
        value::Kind, Condition, CreateCollectionBuilder, Distance, Filter, PointStruct,
        SearchParams, SearchPointsBuilder, UpsertPointsBuilder, VectorParams,
    },
    Qdrant,
};
use std::collections::HashMap;
use std::error::Error;
use uuid::Uuid;

const COLLECTION_NAME: &str = "clinical_cases";

pub async fn recreate_collection(
    qdrant_url: &str,
    collection_name: &str,
    vector_size: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client = Qdrant::from_url(qdrant_url).build()?;

    // Attempt DELETE collection -> ignore "not found" error
    if let Err(e) = client.delete_collection(collection_name).await {
        tracing::warn!("Failed to delete collection {} (may not exist): {}", collection_name, e);
    }

    client
        .create_collection(
            CreateCollectionBuilder::new(collection_name).vectors_config(VectorParams {
                size: vector_size,
                distance: Distance::Cosine.into(),
                ..Default::default()
            }),
        )
        .await?;

    tracing::info!("[Qdrant] Recreated collection: {} with size {}", collection_name, vector_size);
    Ok(())
}

pub async fn init_and_embed(
    qdrant_url: &str,
    embedding_url: &str,
    raw_text: &str,
    case_id: &str,
    vector_size: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("[Qdrant] Initializing Vector Store for Case: {}", case_id);

    let client = Qdrant::from_url(qdrant_url).build()?;

    if !client.collection_exists(COLLECTION_NAME).await? {
        client
            .create_collection(
                CreateCollectionBuilder::new(COLLECTION_NAME).vectors_config(VectorParams {
                    size: vector_size,
                    distance: Distance::Cosine.into(),
                    ..Default::default()
                }),
            )
            .await?;
        tracing::info!("[Qdrant] Created new collection: {}", COLLECTION_NAME);
    }

    let model = BioLordEncoder::new(embedding_url);

    // Naive Chunking
    let chunks: Vec<String> = raw_text
        .split("\n\n")
        .filter(|c| !c.trim().is_empty())
        .map(|c| c.to_string())
        .collect();

    if chunks.is_empty() {
        return Ok(());
    }

    let embeddings = model.embed(chunks.clone()).await
        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?;

    let mut points = Vec::new();
    for (i, embedding) in embeddings.into_iter().enumerate() {
        let payload = serde_json::json!({
            "text": chunks[i],
            "case_id": case_id
        })
        .as_object()
        .unwrap()
        .clone();

        // FIX 2: Explicitly define the HashMap type to resolve compiler ambiguity
        let payload_map: HashMap<String, serde_json::Value> = payload.into_iter().collect();

        let point = PointStruct::new(Uuid::new_v4().to_string(), embedding, payload_map);
        points.push(point);
    }

    client
        .upsert_points(UpsertPointsBuilder::new(COLLECTION_NAME, points))
        .await?;
    tracing::info!(
        "[Qdrant] Memorized {} vectors for Case: {}",
        chunks.len(),
        case_id
    );

    Ok(())
}

pub async fn search_case_context(
    qdrant_url: &str,
    embedding_url: &str,
    query: &str,
    case_id: &str,
    limit: u64,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let client = Qdrant::from_url(qdrant_url).build()?;

    let model = BioLordEncoder::new(embedding_url);

    let query_embedding = model.embed(vec![query.to_string()]).await
        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?.pop().unwrap();

    // The Metadata Barrier: STRICT case_id filtering
    let case_filter = Filter::all([Condition::matches("case_id", case_id.to_string())]);

    let search_result = client
        .search_points(
            SearchPointsBuilder::new(COLLECTION_NAME, query_embedding, limit)
                .filter(case_filter)
                .with_payload(true)
                .params(SearchParams {
                    exact: Some(false),
                    hnsw_ef: Some(128),
                    ..Default::default()
                }),
        )
        .await?;

    let mut context = String::new();
    for hit in search_result.result {
        if let Some(payload_val) = hit.payload.get("text") {
            if let Some(Kind::StringValue(text)) = &payload_val.kind {
                context.push_str(text);
                context.push_str("\n---\n");
            }
        }
    }

    Ok(context)
}
pub async fn search_global_knowledge(
    qdrant_url: &str,
    embedding_url: &str,
    query: &str,
    limit: u64,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let client = Qdrant::from_url(qdrant_url).build()?;
    let model = BioLordEncoder::new(embedding_url);

    let query_embedding = model.embed(vec![query.to_string()]).await
        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?.pop().unwrap();

    let search_result = client
        .search_points(
            SearchPointsBuilder::new("bodhi_global_knowledge", query_embedding, limit)
                .with_payload(true),
        )
        .await?;

    let mut context = String::new();
    for hit in search_result.result {
        if let Some(payload_val) = hit.payload.get("fact") {
            if let Some(Kind::StringValue(text)) = &payload_val.kind {
                context.push_str("- ");
                context.push_str(text);
                context.push_str("\n");
            }
        }
    }

    Ok(context)
}
