use anyhow::{Context, Result};
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;

use clob_engine::events::Fill;

pub const TOPIC_FILLS_OUT: &str = "fills-out";

pub struct FillPublisher {
    inner: FutureProducer,
}

impl FillPublisher {
    pub fn new(brokers: &str) -> Result<Self> {
        let inner: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .set("queue.buffering.max.messages", "100000")
            .set("queue.buffering.max.ms", "5")
            .set("batch.num.messages", "1000")
            .create()
            .context("failed to create fill publisher")?;

        Ok(Self { inner })
    }

    pub async fn send_fill(&self, fill: &Fill) -> Result<()> {
        let payload = postcard::to_allocvec(fill).context("failed to serialize fill")?;

        let key = fill.maker_order_id.to_string();

        let record = FutureRecord::to(TOPIC_FILLS_OUT)
            .key(key.as_str())
            .payload(payload.as_slice());

        self.inner
            .send(record, Timeout::Never)
            .await
            .map_err(|(e, _)| anyhow::anyhow!("kafka send failed: {}", e))?;

        Ok(())
    }
}
