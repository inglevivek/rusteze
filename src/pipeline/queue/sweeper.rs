use sqlx::{PgPool};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

pub struct ZombieSweeper;

impl ZombieSweeper {
    const STALE_TIMEOUT_MINUTES: i32 = 15;
    const SWEEP_INTERVAL_SECS: u64 = 300; // 5 minutes

    /// Spawns a background task to reclaim jobs stuck in 'Processing'.
    pub fn spawn(db_pool: Arc<PgPool>, cancel_token: CancellationToken) {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(Self::SWEEP_INTERVAL_SECS));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Using sqlx::query instead of query! to avoid compile-time DATABASE_URL requirement
                        let result = sqlx::query(
                            r#"
                            UPDATE pipeline_jobs 
                            SET status = 'Pending', 
                                error = 'Job reclaimed by ZombieSweeper due to worker crash',
                                started_at = NULL
                            WHERE status = 'Processing' 
                              AND started_at < NOW() - ($1 || ' minutes')::INTERVAL
                            "#
                        )
                        .bind(Self::STALE_TIMEOUT_MINUTES as f64)
                        .execute(&*db_pool)
                        .await;

                        match result {
                            Ok(res) if res.rows_affected() > 0 => {
                                println!("ZombieSweeper: Reclaimed {} stuck jobs.", res.rows_affected());
                            }
                            Err(e) => {
                                eprintln!("CRITICAL: ZombieSweeper failed to query database: {}", e);
                            }
                            _ => {} // No zombies found
                        }
                    }
                    _ = cancel_token.cancelled() => {
                        println!("ZombieSweeper shutting down gracefully...");
                        break;
                    }
                }
            }
        });
    }
}
