use sqlx::PgPool;

use crate::cache::fills::FillsCache;
use crate::cache::orderbook::OrderBookCache;
use crate::kafka::producer::KafkaProducer;

pub struct AppState {
    pub pool: PgPool,
    pub producer: KafkaProducer,
    pub book_cache: OrderBookCache,
    pub fills_cache: FillsCache,
}
