use tokio::sync::broadcast;

use clob_engine::events::Fill;

const CHANNEL_CAPACITY: usize = 1024;

#[derive(Clone)]
pub struct FillBroadcast {
    tx: broadcast::Sender<String>,
}

impl FillBroadcast {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx }
    }

    pub fn publish(&self, fill: &Fill) {
        let msg = serde_json::json!({
            "maker_order_id": fill.maker_order_id,
            "taker_order_id": fill.taker_order_id,
            "price":          fill.price.to_string(),
            "quantity":       fill.quantity.to_string(),
            "sequence":       fill.sequence,
        })
        .to_string();

        // broadcast::send errors only when there are no receivers — not an error
        let _ = self.tx.send(msg);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}
