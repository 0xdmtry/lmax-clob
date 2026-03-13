use anyhow::{Context, Result};
use metrics::histogram;
use sqlx::PgPool;
use std::time::Instant;
use tokio::time::{interval, Duration};
use tracing::{info, instrument, warn};
use uuid::Uuid;

use clob_engine::events::Fill;

use crate::metrics::{DB_WRITE_BATCH_SIZE, DB_WRITE_LATENCY_NS};

pub struct FillWriter {
    pool: PgPool,
    batch_size: usize,
    flush_ms: u64,
}

impl FillWriter {
    pub fn new(pool: PgPool) -> Self {
        let batch_size = std::env::var("DB_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(500);
        let flush_ms = std::env::var("DB_FLUSH_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);
        Self {
            pool,
            batch_size,
            flush_ms,
        }
    }

    pub async fn run(self, mut rx: tokio::sync::mpsc::Receiver<Fill>) -> Result<()> {
        let mut buf: Vec<Fill> = Vec::with_capacity(self.batch_size);
        let mut tick = interval(Duration::from_millis(self.flush_ms));

        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    match maybe {
                        Some(fill) => {
                            buf.push(fill);
                            if buf.len() >= self.batch_size {
                                self.flush(&mut buf).await;
                            }
                        }
                        None => {
                            if !buf.is_empty() {
                                self.flush(&mut buf).await;
                            }
                            info!("fill writer channel closed, exiting");
                            return Ok(());
                        }
                    }
                }
                _ = tick.tick() => {
                    if !buf.is_empty() {
                        self.flush(&mut buf).await;
                    }
                }
            }
        }
    }

    async fn flush(&self, buf: &mut Vec<Fill>) {
        let batch: Vec<Fill> = buf.drain(..).collect();
        let n = batch.len();
        histogram!(DB_WRITE_BATCH_SIZE).record(n as f64);
        let t0 = Instant::now();
        if let Err(e) = self.insert_batch(&batch).await {
            warn!("fill batch insert failed ({} rows): {}", n, e);
        } else {
            histogram!(DB_WRITE_LATENCY_NS).record(t0.elapsed().as_nanos() as f64);
            info!("persisted {} fills", n);
        }
    }

    #[instrument(skip(self, batch), fields(batch_size = batch.len()))]
    async fn insert_batch(&self, batch: &[Fill]) -> Result<()> {
        let mut ids: Vec<Uuid> = Vec::with_capacity(batch.len());
        let mut maker_order_ids: Vec<Uuid> = Vec::with_capacity(batch.len());
        let mut taker_order_ids: Vec<Uuid> = Vec::with_capacity(batch.len());
        let mut prices: Vec<rust_decimal::Decimal> = Vec::with_capacity(batch.len());
        let mut quantities: Vec<rust_decimal::Decimal> = Vec::with_capacity(batch.len());
        let mut sequences: Vec<i64> = Vec::with_capacity(batch.len());

        for fill in batch {
            ids.push(Uuid::new_v4());
            maker_order_ids.push(fill.maker_order_id);
            taker_order_ids.push(fill.taker_order_id);
            prices.push(fill.price);
            quantities.push(fill.quantity);
            sequences.push(fill.sequence as i64);
        }

        sqlx::query(
            r#"
            INSERT INTO fills
                (id, maker_order_id, taker_order_id, price, quantity, sequence, created_at)
            SELECT
                UNNEST($1::uuid[]),
                UNNEST($2::uuid[]),
                UNNEST($3::uuid[]),
                UNNEST($4::numeric[]),
                UNNEST($5::numeric[]),
                UNNEST($6::bigint[]),
                now()
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(&ids)
        .bind(&maker_order_ids)
        .bind(&taker_order_ids)
        .bind(&prices)
        .bind(&quantities)
        .bind(&sequences)
        .execute(&self.pool)
        .await
        .context("batch insert fills")?;

        Ok(())
    }
}
