use anyhow::{Context, Result};
use sqlx::PgPool;

use clob_engine::order::Order;

pub struct OrderWriter {
    pool: PgPool,
}

impl OrderWriter {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert(&self, order: &Order) -> Result<()> {
        let side = format!("{:?}", order.side).to_lowercase();
        let status = format!("{:?}", order.status).to_lowercase();

        sqlx::query(
            r#"
            INSERT INTO orders
                (id, market_id, side, price, quantity, status, sequence, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, now(), now())
            ON CONFLICT (id)
            DO UPDATE SET status     = EXCLUDED.status,
                          updated_at = now()
            "#,
        )
        .bind(order.id)
        .bind(&order.market_id)
        .bind(&side)
        .bind(order.price)
        .bind(order.quantity)
        .bind(&status)
        .bind(order.sequence as i64)
        .execute(&self.pool)
        .await
        .context("upsert order")?;

        Ok(())
    }
}
