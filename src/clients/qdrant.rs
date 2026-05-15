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

    tracing::info!(
        "┌── [Qdrant ▶ SEND] recreate_collection ─────────────────────\n│  collection='{}' vector_size={}\n└────────────────────────────────────────────────────────────",
        collection_name,
        vector_size
    );

    // Attempt DELETE collection -> ignore "not found" error
    if let Err(e) = client.delete_collection(collection_name).await {
        tracing::warn!("[Qdrant] Failed to delete collection {} (may not exist): {}", collection_name, e);
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

    tracing::info!(
        "└── [Qdrant ◀ RECV] recreate_collection ─────────────────────\n│  ✅ collection='{}' created (dim={})\n└────────────────────────────────────────────────────────────",
        collection_name,
        vector_size
    );
    Ok(())
}

pub async fn init_and_embed(
    qdrant_url: &str,
    embedding_url: &str,
    raw_text: &str,
    case_id: &str,
    vector_size: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!(
        "┌── [Qdrant ▶ SEND] init_and_embed ──────────────────────────\n│  collection='{}' case_id='{}' text_len={} chars\n│  embed_url: {}\n└────────────────────────────────────────────────────────────",
        COLLECTION_NAME,
        case_id,
        raw_text.len(),
        embedding_url
    );

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
        tracing::warn!("[Qdrant] No chunks to embed for case_id='{}'", case_id);
        return Ok(());
    }

    tracing::info!(
        "┌── [Qdrant ▶ SEND] embed request ───────────────────────────\n│  Embedding {} chunks via {}\n└────────────────────────────────────────────────────────────",
        chunks.len(),
        embedding_url
    );

    let embeddings = model.embed(chunks.clone()).await
        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?;

    tracing::info!(
        "└── [Qdrant ◀ RECV] embed response ──────────────────────────\n│  ✅ {} embedding vectors returned (dim={})\n└────────────────────────────────────────────────────────────",
        embeddings.len(),
        embeddings.first().map(|v| v.len()).unwrap_or(0)
    );

    let mut points = Vec::new();
    for (i, embedding) in embeddings.into_iter().enumerate() {
        let payload = serde_json::json!({
            "text": chunks[i],
            "case_id": case_id
        })
        .as_object()
        .unwrap()
        .clone();

        let payload_map: HashMap<String, serde_json::Value> = payload.into_iter().collect();
        let point = PointStruct::new(Uuid::new_v4().to_string(), embedding, payload_map);
        points.push(point);
    }

    tracing::info!(
        "┌── [Qdrant ▶ SEND] upsert_points ──────────────────────────\n│  Upserting {} points into '{}' for case_id='{}'\n└────────────────────────────────────────────────────────────",
        points.len(),
        COLLECTION_NAME,
        case_id
    );

    client
        .upsert_points(UpsertPointsBuilder::new(COLLECTION_NAME, points))
        .await?;

    tracing::info!(
        "└── [Qdrant ◀ RECV] upsert_points ──────────────────────────\n│  ✅ {} vectors memorized for case_id='{}'\n└────────────────────────────────────────────────────────────",
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
    tracing::info!(
        "┌── [Qdrant ▶ SEND] search_case_context ─────────────────────\n\
         │  collection='{}' case_id='{}' limit={}\n\
         │  query: \"{}{}\"\n\
         └────────────────────────────────────────────────────────────",
        COLLECTION_NAME,
        case_id,
        limit,
        &query[..query.len().min(120)],
        if query.len() > 120 { "…" } else { "" }
    );

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
    let mut hit_count = 0usize;
    for hit in search_result.result {
        if let Some(payload_val) = hit.payload.get("text") {
            if let Some(Kind::StringValue(text)) = &payload_val.kind {
                context.push_str(text);
                context.push_str("\n---\n");
                hit_count += 1;
            }
        }
    }

    tracing::info!(
        "└── [Qdrant ◀ RECV] search_case_context ─────────────────────\n│  ✅ {} chunk(s) returned for case_id='{}'\n└────────────────────────────────────────────────────────────",
        hit_count,
        case_id
    );

    Ok(context)
}

pub async fn search_global_knowledge(
    qdrant_url: &str,
    embedding_url: &str,
    query: &str,
    limit: u64,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    tracing::info!(
        "┌── [Qdrant ▶ SEND] search_global_knowledge ─────────────────\n\
         │  collection='bodhi_global_knowledge' limit={}\n\
         │  query: \"{}{}\"\n\
         └────────────────────────────────────────────────────────────",
        limit,
        &query[..query.len().min(120)],
        if query.len() > 120 { "…" } else { "" }
    );

    let client = Qdrant::from_url(qdrant_url).build()?;
    let model = BioLordEncoder::new(embedding_url);

    let query_embedding = model.embed(vec![query.to_string()]).await
        .map_err(|e| Box::<dyn std::error::Error + Send + Sync>::from(e))?.pop().unwrap();

    // Diagnostic: log how many points exist in the collection before searching
    // This catches the "collection exists but is empty" case silently returning 0.
    tracing::info!(
        "[Qdrant] search_global_knowledge called — query='{}' limit={}",
        query, limit
    );

    let search_result = client
        .search_points(
            SearchPointsBuilder::new("bodhi_global_knowledge", query_embedding, limit)
                .with_payload(true),
        )
        .await?;

    let hits = &search_result.result;
    tracing::info!(
        "[Qdrant] search_global_knowledge raw hit count: {}",
        hits.len()
    );
    if hits.is_empty() {
        tracing::warn!(
            "[Qdrant] ⚠️  bodhi_global_knowledge returned 0 hits for query='{}'. \
             Verify: (1) collection exists, (2) collection has points, \
             (3) embedding dimension matches the indexed vectors.",
            query
        );
    }

    let mut context = String::new();
    let mut fact_count = 0usize;
    for hit in hits {
        let payload_val = hit.payload.get("fact").or_else(|| hit.payload.get("text"));
        if let Some(payload_val) = payload_val {
            if let Some(Kind::StringValue(text)) = &payload_val.kind {
                context.push_str("- ");
                context.push_str(text);
                context.push('\n');
                fact_count += 1;
            }
        }
    }

    if fact_count > 0 {
        tracing::info!(
            "└── [Qdrant ◀ RECV] search_global_knowledge ─────────────────\n│  ✅ {} fact(s) returned from BODHI global knowledge\n└────────────────────────────────────────────────────────────",
            fact_count
        );
    } else {
        tracing::error!(
            "└── [Qdrant ◀ RECV] search_global_knowledge ─────────────────\n\
             │  ❌ 0 facts returned — bodhi_global_knowledge is EMPTY or payload key mismatch.\n\
             │  Run: data_scripts/ingest.py to populate. Check payload key is \"fact\".\n\
             └────────────────────────────────────────────────────────────"
        );
    }

    Ok(context)
}

/// Returns the number of points currently indexed in a Qdrant collection.
/// Call from a startup check or debug endpoint to verify bodhi_global_knowledge is populated.
pub async fn count_collection_points(
    qdrant_url: &str,
    collection_name: &str,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let client = Qdrant::from_url(qdrant_url).build()?;
    let resp = client.collection_info(collection_name).await?;
    
    let count = resp.result
        .and_then(|r| r.points_count)
        .unwrap_or(0);

    tracing::info!(
        "[Qdrant] collection='{}' has {} points indexed",
        collection_name, count
    );
    Ok(count)
}
