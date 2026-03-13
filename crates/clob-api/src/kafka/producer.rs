use anyhow::{Context, Result};
use metrics::histogram;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;
use std::time::Instant;
use tracing::instrument;

use clob_engine::order::Order;

pub const TOPIC_ORDERS_IN: &str = "orders-in";
const KAFKA_PRODUCE_LATENCY_NS: &str = "kafka_produce_latency_ns";

pub struct KafkaProducer {
    inner: FutureProducer,
}

impl KafkaProducer {
    pub fn new(brokers: &str) -> Result<Self> {
        let linger_ms = std::env::var("KAFKA_LINGER_MS").unwrap_or_else(|_| "50".into());
        let batch_num = std::env::var("KAFKA_BATCH_NUM_MESSAGES").unwrap_or_else(|_| "5000".into());
        let batch_size = std::env::var("KAFKA_BATCH_SIZE").unwrap_or_else(|_| "1048576".into());

        let inner: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .set("queue.buffering.max.messages", "100000")
            .set("queue.buffering.max.ms", &linger_ms)
            .set("batch.num.messages", &batch_num)
            .set("batch.size", &batch_size)
            .create()
            .context("failed to create Kafka producer")?;

        Ok(Self { inner })
    }

    #[instrument(skip(self, order), fields(order_id = %order.id, market_id = %order.market_id))]
    pub async fn send_order(&self, order: &Order) -> Result<()> {
        let payload = postcard::to_allocvec(order).context("failed to serialize order")?;
        let key = order.id.to_string();
        let record = FutureRecord::to(TOPIC_ORDERS_IN)
            .key(key.as_str())
            .payload(payload.as_slice());

        let t0 = Instant::now();
        self.inner
            .send(record, Timeout::Never)
            .await
            .map_err(|(e, _)| anyhow::anyhow!("kafka send failed: {}", e))?;

        histogram!(KAFKA_PRODUCE_LATENCY_NS).record(t0.elapsed().as_nanos() as f64);

        Ok(())
    }
}
