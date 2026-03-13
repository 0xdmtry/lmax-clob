mod config;
mod generator;
mod server;

use std::net::SocketAddr;
use std::sync::Arc;

use tonic::transport::Server;
use tracing::info;
use yellowstone_grpc_proto::geyser::geyser_server::GeyserServer;

use crate::config::Config;
use crate::server::MockGeyserService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("yellowstone-mock starting");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("RUST_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Arc::new(Config::from_env());
    info!(
        rate = config.stream_rate_per_second,
        payload = config.payload_size_bytes,
        disconnects = config.simulate_disconnects,
        port = config.port,
        "yellowstone-mock starting"
    );

    let addr: SocketAddr = format!("0.0.0.0:{}", config.port).parse()?;
    let service = MockGeyserService::new(Arc::clone(&config));

    info!("listening on {}", addr);

    Server::builder()
        .add_service(GeyserServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
