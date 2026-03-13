use actix_web::{get, web, HttpResponse};
use redis::AsyncCommands;
use serde_json::json;

use crate::state::AppState;

#[get("/markets/{id}/book")]
pub async fn get_book(state: web::Data<AppState>, path: web::Path<String>) -> HttpResponse {
    let market_id = path.into_inner();

    let mut conn = match state
        .book_cache
        .client()
        .get_multiplexed_async_connection()
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("redis connection failed: {}", e);
            return HttpResponse::ServiceUnavailable().json(json!({"error": "cache unavailable"}));
        }
    };

    let bid: Option<String> = conn
        .get(format!("book:{}:best_bid", market_id))
        .await
        .unwrap_or(None);

    let ask: Option<String> = conn
        .get(format!("book:{}:best_ask", market_id))
        .await
        .unwrap_or(None);

    HttpResponse::Ok().json(json!({
        "market_id": market_id,
        "best_bid":  bid,
        "best_ask":  ask,
    }))
}

#[get("/markets/{id}/trades")]
pub async fn get_trades(state: web::Data<AppState>, path: web::Path<String>) -> HttpResponse {
    let market_id = path.into_inner();

    let rows: Vec<(
        uuid::Uuid,
        rust_decimal::Decimal,
        rust_decimal::Decimal,
        i64,
    )> = sqlx::query_as(
        "SELECT id, price, quantity, sequence \
             FROM fills \
             ORDER BY sequence DESC \
             LIMIT 50",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let trades: Vec<_> = rows
        .into_iter()
        .map(|(id, price, qty, seq)| {
            json!({
                "id":       id,
                "price":    price,
                "quantity": qty,
                "sequence": seq,
            })
        })
        .collect();

    HttpResponse::Ok().json(json!({
        "market_id": market_id,
        "trades":    trades,
    }))
}
