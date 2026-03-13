use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use futures::StreamExt;
use metrics::gauge;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::Message;
use rdkafka::topic_partition_list::{Offset, TopicPartitionList};
use tracing::{info, warn};

use clob_engine::order::Order;

use crate::metrics::KAFKA_CONSUMER_LAG;

pub const TOPIC_ORDERS_IN: &str = "orders-in";

pub struct KafkaConsumer {
    inner: StreamConsumer,
}

impl KafkaConsumer {
    pub fn new(brokers: &str, group_id: &str) -> Result<Self> {
        let inner: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("group.id", group_id)
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "earliest")
            .set("fetch.min.bytes", "1")
            .set("fetch.wait.max.ms", "5")
            .create()
            .context("failed to create Kafka consumer")?;

        Ok(Self { inner })
    }

    pub fn seek_to(&self, start_offset: i64) -> Result<()> {
        let metadata = self
            .inner
            .fetch_metadata(Some(TOPIC_ORDERS_IN), std::time::Duration::from_secs(5))
            .context("failed to fetch metadata for seek")?;

        let topic = metadata
            .topics()
            .iter()
            .find(|t| t.name() == TOPIC_ORDERS_IN)
            .context("topic orders-in not found in metadata")?;

        let mut tpl = TopicPartitionList::new();
        for partition in topic.partitions() {
            tpl.add_partition_offset(
                TOPIC_ORDERS_IN,
                partition.id(),
                Offset::Offset(start_offset),
            )
            .context("failed to add partition offset")?;
        }

        self.inner
            .assign(&tpl)
            .context("failed to assign partitions with offset")?;
        info!(start_offset, "consumer seeked to offset");
        Ok(())
    }

    pub async fn run(
        self,
        order_tx: Sender<Order>,
        health: crate::health::SharedHealth,
    ) -> Result<()> {
        let mut stream = self.inner.stream();
        let mut count = 0u64;

        while let Some(result) = stream.next().await {
            match result {
                Err(e) => warn!("kafka receive error: {}", e),
                Ok(msg) => {
                    let offset = msg.offset();
                    let partition = msg.partition();

                    let payload = match msg.payload() {
                        Some(p) => p,
                        None => {
                            warn!("empty kafka message at offset {}", offset);
                            self.inner.commit_message(&msg, CommitMode::Async).ok();
                            continue;
                        }
                    };

                    match postcard::from_bytes::<Order>(payload) {
                        Ok(mut order) => {
                            order.sequence = offset as u64;
                            if order_tx.send(order).is_err() {
                                info!("engine channel closed, consumer exiting");
                                return Ok(());
                            }
                            count += 1;
                            if count % 10_000 == 0 {
                                info!("consumed {} orders from kafka", count);
                            }
                        }
                        Err(e) => warn!("deserialize error at offset {}: {}", offset, e),
                    }

                    self.inner.commit_message(&msg, CommitMode::Async).ok();

                    // Sample lag every 1000 messages
                    if count % 1_000 == 0 {
                        if let Ok((_, high)) = self.inner.fetch_watermarks(
                            TOPIC_ORDERS_IN,
                            partition,
                            std::time::Duration::from_millis(100),
                        ) {
                            let lag = (high - offset - 1).max(0);
                            gauge!(KAFKA_CONSUMER_LAG, "partition" => partition.to_string())
                                .set(lag as f64);
                        }
                    }

                    health.kafka_ready.store(true, std::sync::atomic::Ordering::Release);
                }
            }
        }

        Ok(())
    }
}
