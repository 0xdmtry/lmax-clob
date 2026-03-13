use actix_web::{delete, get, post, web, HttpResponse};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use uuid::Uuid;

use clob_engine::order::{Order, OrderStatus};
use clob_engine::types::{OrderType, Side};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PlaceOrderRequest {
    pub market_id: String,
    pub side: String,
    pub order_type: String,
    pub price: Option<Decimal>,
    pub quantity: Decimal,
}

#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub id: Uuid,
}

#[post("/orders")]
#[instrument(skip(state, body), fields(market_id, order_id, side))]
pub async fn place_order(
    state: web::Data<AppState>,
    body: web::Json<PlaceOrderRequest>,
) -> HttpResponse {
    let side = match body.side.to_lowercase().as_str() {
        "bid" | "buy" => Side::Bid,
        "ask" | "sell" => Side::Ask,
        _ => return HttpResponse::BadRequest().json(serde_json::json!({"error": "invalid side"})),
    };

    let order_type = match body.order_type.to_lowercase().as_str() {
        "limit" => OrderType::Limit,
        "market" => OrderType::Market,
        _ => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({"error": "invalid order_type"}))
        }
    };

    if order_type == OrderType::Limit && body.price.is_none() {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "price required for limit orders"}));
    }

    if body.quantity <= Decimal::ZERO {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({"error": "quantity must be positive"}));
    }

    let order = Order {
        id: Uuid::new_v4(),
        market_id: body.market_id.clone(),
        side,
        order_type,
        price: body.price,
        quantity: body.quantity,
        filled: Decimal::ZERO,
        status: OrderStatus::Open,
        sequence: 0,
    };

    let id = order.id;

    tracing::Span::current().record("order_id", id.to_string().as_str());
    tracing::Span::current().record("market_id", body.market_id.as_str());
    tracing::Span::current().record("side", body.side.as_str());

    match state.producer.send_order(&order).await {
        Ok(_) => HttpResponse::Ok().json(OrderResponse { id }),
        Err(e) => {
            tracing::warn!("failed to publish order: {}", e);
            HttpResponse::InternalServerError()
                .json(serde_json::json!({"error": "failed to publish order"}))
        }
    }
}

#[get("/orders/{id}")]
pub async fn get_order(state: web::Data<AppState>, path: web::Path<Uuid>) -> HttpResponse {
    let id = path.into_inner();

    let row: Option<(Uuid, String, String, Option<Decimal>, Decimal, String, i64)> =
        sqlx::query_as(
            "SELECT id, market_id, side, price, quantity, status, sequence \
             FROM orders WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    match row {
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "order not found"})),
        Some((id, market_id, side, price, quantity, status, sequence)) => {
            HttpResponse::Ok().json(serde_json::json!({
                "id":        id,
                "market_id": market_id,
                "side":      side,
                "price":     price,
                "quantity":  quantity,
                "status":    status,
                "sequence":  sequence,
            }))
        }
    }
}

#[delete("/orders/{id}")]
pub async fn cancel_order(_state: web::Data<AppState>, path: web::Path<Uuid>) -> HttpResponse {
    let id = path.into_inner();
    HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "cancel not yet implemented",
        "id":    id,
    }))
}
