use anyhow::{Context, Result};
use sqlx::PgPool;
use tracing::info;

use clob_engine::snapshot::{deserialize_book, BookSnapshot};

pub struct SnapshotStore {
    pool: PgPool,
    market_id: String,
}

impl SnapshotStore {
    pub fn new(pool: PgPool, market_id: impl Into<String>) -> Self {
        Self {
            pool,
            market_id: market_id.into(),
        }
    }

    /// Save raw postcard bytes (called from engine thread which already serialized).
    pub async fn save_raw(&self, payload: &[u8], kafka_offset: i64, sequence: u64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO snapshots (market_id, sequence, state, kafka_offset, created_at)
            VALUES ($1, $2, $3, $4, now())
            ON CONFLICT (market_id)
            DO UPDATE SET sequence     = EXCLUDED.sequence,
                          state        = EXCLUDED.state,
                          kafka_offset = EXCLUDED.kafka_offset,
                          created_at   = EXCLUDED.created_at
            "#,
        )
        .bind(&self.market_id)
        .bind(sequence as i64)
        .bind(payload)
        .bind(kafka_offset)
        .execute(&self.pool)
        .await
        .context("failed to upsert snapshot")?;

        info!(market_id = %self.market_id, kafka_offset, "snapshot saved");
        Ok(())
    }

    /// Load latest snapshot for this market.
    pub async fn load(&self) -> Result<Option<(BookSnapshot, i64)>> {
        let row: Option<(Vec<u8>, i64)> = sqlx::query_as(
            r#"
            SELECT state, kafka_offset
            FROM snapshots
            WHERE market_id = $1
            ORDER BY sequence DESC
            LIMIT 1
            "#,
        )
        .bind(&self.market_id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to query snapshot")?;

        match row {
            None => Ok(None),
            Some((bytes, offset)) => {
                let (_, snapshot) =
                    deserialize_book(&bytes).context("failed to decode snapshot")?;
                Ok(Some((snapshot, offset)))
            }
        }
    }
}
