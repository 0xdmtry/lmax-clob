use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use yellowstone_grpc_proto::geyser::{
    geyser_server::Geyser, GetBlockHeightRequest, GetBlockHeightResponse,
    GetLatestBlockhashRequest, GetLatestBlockhashResponse, GetSlotRequest, GetSlotResponse,
    GetVersionRequest, GetVersionResponse, IsBlockhashValidRequest, IsBlockhashValidResponse,
    PingRequest, PongResponse, SubscribeRequest, SubscribeUpdate,
};

use yellowstone_grpc_proto::geyser::{SubscribeDeshredRequest, SubscribeUpdateDeshred};

use crate::config::Config;
use crate::generator::generate_update;

pub struct MockGeyserService {
    config: Arc<Config>,
}

impl MockGeyserService {
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

type SubscribeStream = Pin<Box<dyn Stream<Item = Result<SubscribeUpdate, Status>> + Send>>;

#[tonic::async_trait]
impl Geyser for MockGeyserService {
    type SubscribeDeshredStream =
        Pin<Box<dyn Stream<Item = Result<SubscribeUpdateDeshred, Status>> + Send>>;

    type SubscribeStream = SubscribeStream;

    async fn subscribe(
        &self,
        _request: Request<tonic::Streaming<SubscribeRequest>>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let config = Arc::clone(&self.config);
        let (tx, rx) = mpsc::channel::<Result<SubscribeUpdate, Status>>(256);

        tokio::spawn(async move {
            let interval_ns = 1_000_000_000u64
                .checked_div(config.stream_rate_per_second)
                .unwrap_or(1_000_000);
            let tick = Duration::from_nanos(interval_ns);

            let disconnect_after = if config.simulate_disconnects {
                Some(Duration::from_secs(config.disconnect_interval_s))
            } else {
                None
            };

            let started = Instant::now();
            let mut count = 0u64;

            loop {
                if let Some(limit) = disconnect_after {
                    if started.elapsed() >= limit {
                        warn!("simulating disconnect after {} events", count);
                        break;
                    }
                }

                let update = generate_update(config.payload_size_bytes);
                if tx.send(Ok(update)).await.is_err() {
                    break;
                }

                count += 1;
                if count.is_multiple_of(10_000) {
                    info!("streamed {} events", count);
                }

                tokio::time::sleep(tick).await;
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn ping(&self, request: Request<PingRequest>) -> Result<Response<PongResponse>, Status> {
        Ok(Response::new(PongResponse {
            count: request.into_inner().count,
        }))
    }

    async fn get_latest_blockhash(
        &self,
        _: Request<GetLatestBlockhashRequest>,
    ) -> Result<Response<GetLatestBlockhashResponse>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn get_block_height(
        &self,
        _: Request<GetBlockHeightRequest>,
    ) -> Result<Response<GetBlockHeightResponse>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn get_slot(
        &self,
        _: Request<GetSlotRequest>,
    ) -> Result<Response<GetSlotResponse>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn is_blockhash_valid(
        &self,
        _: Request<IsBlockhashValidRequest>,
    ) -> Result<Response<IsBlockhashValidResponse>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn get_version(
        &self,
        _: Request<GetVersionRequest>,
    ) -> Result<Response<GetVersionResponse>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn subscribe_replay_info(
        &self,
        _: Request<yellowstone_grpc_proto::geyser::SubscribeReplayInfoRequest>,
    ) -> Result<Response<yellowstone_grpc_proto::geyser::SubscribeReplayInfoResponse>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn subscribe_deshred(
        &self,
        _request: Request<tonic::Streaming<SubscribeDeshredRequest>>,
    ) -> Result<Response<Self::SubscribeDeshredStream>, Status> {
        Err(Status::unimplemented("not implemented"))
    }
}
