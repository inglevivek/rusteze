use d3_graph_bench::clients::{postgres, qdrant};
use d3_graph_bench::config::Config;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    tracing::info!("Starting BioLORD-2023 Reindex Migration...");

    let config = Config::load();
    let pg_pool = postgres::establish_pool(&config.pg_url).await;
    let pg_pool = Arc::new(pg_pool);

    let vector_size = config.embedding_vector_size as u64;

    // 1. Flush both collections
    tracing::info!("Flushing clinical_cases collection...");
    qdrant::recreate_collection(&config.qdrant_url, "clinical_cases", vector_size).await?;

    tracing::info!("Flushing bodhi_global_knowledge collection...");
    qdrant::recreate_collection(&config.qdrant_url, "bodhi_global_knowledge", vector_size).await?;

    // 2. Fetch all cases
    tracing::info!("Fetching existing cases from Postgres...");
    // We need to fetch ALL cases, not just limited to 50.
    // list_cases has a LIMIT 50. Let's do a direct query here to get all cases.
    let client = pg_pool.get().await?;
    let stmt = client
        .prepare("SELECT case_id, document_text FROM cases")
        .await?;
    let rows = client.query(&stmt, &[]).await?;

    let mut success_count = 0;
    let mut skip_count = 0;

    for row in rows {
        let case_id: String = row.get(0);
        let doc_text: Option<String> = row.get(1);

        if let Some(text) = doc_text {
            if text.trim().is_empty() {
                tracing::warn!("Skipping case {}: empty document_text", case_id);
                skip_count += 1;
                continue;
            }

            tracing::info!("Re-embedding case {}...", case_id);
            if let Err(e) = qdrant::init_and_embed(&config.qdrant_url, &config.embedding_url, &text, &case_id, vector_size).await {
                tracing::error!("Failed to embed case {}: {}", case_id, e);
            } else {
                success_count += 1;
            }
        } else {
            tracing::warn!("Skipping case {}: null document_text", case_id);
            skip_count += 1;
        }
    }

    tracing::info!(
        "Reindex complete! Successfully re-embedded {} cases. Skipped {} cases.",
        success_count,
        skip_count
    );
    tracing::info!("NOTE: bodhi_global_knowledge has been flushed and is currently empty. It must be rebuilt separately if needed.");

    Ok(())
}
