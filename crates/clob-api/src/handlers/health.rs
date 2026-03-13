use actix_web::{get, web, HttpResponse};
use serde_json::json;

use crate::health::SharedHealth;
use crate::state::AppState;

#[get("/health")]
pub async fn health(state: web::Data<AppState>) -> HttpResponse {
    let db_ok = sqlx::query("SELECT 1").execute(&state.pool).await.is_ok();
    let status = if db_ok { "ok" } else { "degraded" };
    HttpResponse::Ok().json(json!({ "status": status, "db": db_ok}))
}

#[get("/healthz")]
pub async fn healthz(
    state: web::Data<AppState>,
    shared_health: web::Data<SharedHealth>,
) -> HttpResponse {
    let db_ok = sqlx::query("SELECT 1").execute(&state.pool).await.is_ok();
    let engine_alive = shared_health
        .engine_alive
        .load(std::sync::atomic::Ordering::Acquire);
    let lease_healthy = shared_health
        .lease_healthy
        .load(std::sync::atomic::Ordering::Acquire);
    let kafka_ready = shared_health
        .kafka_ready
        .load(std::sync::atomic::Ordering::Acquire);
    let snapshot_loaded = shared_health
        .snapshot_loaded
        .load(std::sync::atomic::Ordering::Acquire);

    let ready = db_ok && engine_alive && lease_healthy && kafka_ready;

    let body = json!({
        "ready":          ready,
        "db":             db_ok,
        "engine_alive":   engine_alive,
        "lease_healthy":  lease_healthy,
        "kafka_ready":    kafka_ready,
        "snapshot_loaded": snapshot_loaded,
    });

    if ready {
        HttpResponse::Ok().json(body)
    } else {
        HttpResponse::ServiceUnavailable().json(body)
    }
}
