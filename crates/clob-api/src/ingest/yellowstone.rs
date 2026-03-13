use anyhow::{Context, Result};
use futures::StreamExt;
use tonic::transport::Channel;
use tracing::{info, warn};
use yellowstone_grpc_proto::geyser::{geyser_client::GeyserClient, SubscribeRequest};

use crate::ingest::processor::map_update_to_order;
use crate::kafka::producer::KafkaProducer;

pub async fn run_ingestor(endpoint: String, producer: KafkaProducer) -> Result<()> {
    let channel = Channel::from_shared(endpoint.clone())
        .context("invalid endpoint")?
        .connect()
        .await
        .context("failed to connect to yellowstone mock")?;

    let mut client = GeyserClient::new(channel);
    let request = futures::stream::once(async { SubscribeRequest::default() });

    let mut stream = client
        .subscribe(request)
        .await
        .context("subscribe RPC failed")?
        .into_inner();

    info!("yellowstone ingestor connected to {}", endpoint);

    while let Some(msg) = stream.next().await {
        match msg {
            Ok(update) => {
                let order = map_update_to_order(update);
                if let Err(e) = producer.send_order(&order).await {
                    warn!("kafka produce error: {}", e);
                }
            }
            Err(status) => {
                warn!("stream error: {}", status);
                break;
            }
        }
    }

    Ok(())
}
