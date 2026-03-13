mod cache;
mod follower;
mod handlers;
mod health;
mod ingest;
mod kafka;
mod leader;
mod metrics;
mod middleware;
mod persistence;
mod state;
mod tracing_setup;
mod ws;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use actix_web::{web, App, HttpServer};
use anyhow::{Context, Result};
use metrics_exporter_prometheus::PrometheusBuilder;
use sqlx::postgres::PgPoolOptions;
use tracing::{info, warn};

use cache::fills::FillsCache;
use cache::orderbook::OrderBookCache;
use follower::start_follower_engine;
use ingest::processor::start_engine_thread;
use ingest::yellowstone::run_ingestor;
use kafka::consumer::KafkaConsumer;
use kafka::fill_publisher::FillPublisher;
use kafka::producer::KafkaProducer;
use leader::LeaderElector;
use middleware::metrics::MetricsMiddleware;
use persistence::fills::FillWriter;
use persistence::snapshot::SnapshotStore;
use state::AppState;
use ws::broadcast::FillBroadcast;

#[tokio::main]
async fn main() -> Result<()> {
    let otlp_endpoint =
        std::env::var("OTLP_ENDPOINT").unwrap_or_else(|_| "http://localhost:4317".into());

    tracing_setup::init(&otlp_endpoint, "clob-api").context("failed to init tracing")?;

    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], 9000))
        .install()
        .context("failed to install Prometheus exporter")?;

    metrics::register_all();

    let health_state = crate::health::new_shared();

    let endpoint =
        std::env::var("YELLOWSTONE_ENDPOINT").unwrap_or_else(|_| "http://localhost:10000".into());
    let brokers = std::env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:19092".into());
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://clob:clob@localhost:5432/clob".into());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
    let http_bind = std::env::var("HTTP_BIND").unwrap_or_else(|_| "0.0.0.0:8000".into());
    let namespace = std::env::var("K8S_NAMESPACE").unwrap_or_else(|_| "default".into());
    let pod_name = std::env::var("POD_NAME").unwrap_or_else(|_| "clob-local".into());

    info!("brokers: {}", brokers);

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .context("failed to connect to postgres")?;

    let store = Arc::new(SnapshotStore::new(pool.clone(), "SOL/USDC"));

    let (initial_book, start_offset) = match store.load().await? {
        Some((snap, offset)) => {
            info!(offset, "loaded snapshot, replaying from offset");
            (Some(snap), offset + 1)
        }
        None => {
            info!("no snapshot found, consuming from beginning");
            (None, 0i64)
        }
    };

    if initial_book.is_some() {
        health_state
            .snapshot_loaded
            .store(true, std::sync::atomic::Ordering::Release);
    }

    // ── Leader election ──────────────────────────────────────────────────────
    let elector = Arc::new(LeaderElector::new(
        namespace,
        pod_name,
        Arc::clone(&health_state),
    ));
    let is_leader = Arc::clone(&elector.is_leader);
    let elector_run = Arc::clone(&elector);

    tokio::spawn(async move {
        if let Err(e) = elector_run.run().await {
            warn!("leader elector exited: {}", e);
        }
    });

    // Give elector one cycle to resolve initial state before branching.
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;

    // ── Kafka consumer (shared between leader and follower) ──────────────────
    let consumer = KafkaConsumer::new(&brokers, "clob-engine")?;
    consumer.seek_to(start_offset)?;

    if is_leader.load(Ordering::Acquire) {
        info!("starting in LEADER mode");
        run_leader(
            pool,
            store,
            initial_book,
            consumer,
            brokers,
            redis_url,
            http_bind,
            endpoint,
            is_leader,
            Arc::clone(&health_state),
        )
        .await
    } else {
        info!("starting in FOLLOWER mode");
        run_follower(
            store,
            initial_book,
            consumer,
            brokers,
            redis_url,
            http_bind,
            endpoint,
            is_leader,
            Arc::clone(&health_state),
        )
        .await
    }
}

async fn run_leader(
    pool: sqlx::PgPool,
    store: Arc<SnapshotStore>,
    initial_book: Option<clob_engine::snapshot::BookSnapshot>,
    consumer: KafkaConsumer,
    brokers: String,
    redis_url: String,
    http_bind: String,
    endpoint: String,
    _is_leader: Arc<std::sync::atomic::AtomicBool>,
    shared_health: crate::health::SharedHealth,
) -> Result<()> {
    let (order_tx, fill_rx, book_rx) =
        start_engine_thread(Arc::clone(&store), initial_book, Arc::clone(&shared_health));

    let health_for_consumer = Arc::clone(&shared_health);
    tokio::spawn(async move {
        if let Err(e) = consumer.run(order_tx, health_for_consumer).await {
            warn!("kafka consumer error: {}", e);
        }
    });

    let book_cache_bg = OrderBookCache::new(&redis_url)?;
    tokio::spawn(async move {
        loop {
            match book_rx.try_recv() {
                Ok(update) => {
                    if let Err(e) = book_cache_bg
                        .set_best_bid_ask(&update.market_id, update.best_bid, update.best_ask)
                        .await
                    {
                        warn!("book cache write error: {}", e);
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    warn!("book_rx disconnected");
                    break;
                }
            }
        }
    });

    let fill_publisher = FillPublisher::new(&brokers)?;
    let fill_writer = FillWriter::new(pool.clone());
    let fills_cache_bg = FillsCache::new(&redis_url)?;
    let fill_broadcast = FillBroadcast::new();
    let fill_broadcast2 = fill_broadcast.clone();
    let (db_tx, db_rx) = tokio::sync::mpsc::channel(8_192);

    tokio::spawn(async move {
        if let Err(e) = fill_writer.run(db_rx).await {
            warn!("fill writer error: {}", e);
        }
    });

    tokio::spawn(async move {
        loop {
            match fill_rx.try_recv() {
                Ok(fill) => {
                    fill_broadcast2.publish(&fill);
                    if let Err(e) = fill_publisher.send_fill(&fill).await {
                        warn!("fill publish error: {}", e);
                    }
                    if let Err(e) = fills_cache_bg.push(&fill).await {
                        warn!("fills cache error: {}", e);
                    }
                    if db_tx.send(fill).await.is_err() {
                        warn!("db fill channel closed");
                        break;
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    warn!("fill channel disconnected");
                    break;
                }
            }
        }
    });

    let app_producer = KafkaProducer::new(&brokers)?;
    let app_book_cache = OrderBookCache::new(&redis_url)?;
    let app_fills_cache = FillsCache::new(&redis_url)?;
    let broadcast_data = web::Data::new(fill_broadcast);
    let app_state = web::Data::new(AppState {
        pool: pool.clone(),
        producer: app_producer,
        book_cache: app_book_cache,
        fills_cache: app_fills_cache,
    });

    let health_for_http = Arc::clone(&shared_health);
    let http_bind_clone = http_bind.clone();
    tokio::spawn(async move {
        info!("HTTP listening on {}", http_bind_clone);
        let _ = HttpServer::new(move || {
            App::new()
                .wrap(MetricsMiddleware)
                .app_data(app_state.clone())
                .app_data(broadcast_data.clone())
                .app_data(web::Data::new(Arc::clone(&health_for_http)))
                .service(handlers::health::health)
                .service(handlers::orders::place_order)
                .service(handlers::orders::get_order)
                .service(handlers::orders::cancel_order)
                .service(handlers::markets::get_book)
                .service(handlers::markets::get_trades)
                .service(handlers::ws::ws_market)
                .service(handlers::health::healthz)
        })
        .bind(&http_bind_clone)
        .expect("failed to bind HTTP")
        .run()
        .await;
    });

    let result = run_ingestor(endpoint, KafkaProducer::new(&brokers)?).await;
    tracing_setup::shutdown();
    result
}

async fn run_follower(
    store: Arc<SnapshotStore>,
    initial_book: Option<clob_engine::snapshot::BookSnapshot>,
    consumer: KafkaConsumer,
    brokers: String,
    redis_url: String,
    http_bind: String,
    endpoint: String,
    is_leader: Arc<std::sync::atomic::AtomicBool>,
    shared_health: crate::health::SharedHealth,
) -> Result<()> {
    let order_tx = start_follower_engine(
        Arc::clone(&store),
        initial_book,
        Arc::clone(&is_leader),
        Arc::clone(&shared_health),
    );

    let health_for_consumer = Arc::clone(&shared_health);
    tokio::spawn(async move {
        if let Err(e) = consumer.run(order_tx, health_for_consumer).await {
            warn!("follower kafka consumer error: {}", e);
        }
    });

    // Follower exposes health + read-only book/trades endpoints only.
    let app_producer = KafkaProducer::new(&brokers)?;
    let app_book_cache = OrderBookCache::new(&redis_url)?;
    let app_fills_cache = FillsCache::new(&redis_url)?;
    let fill_broadcast = FillBroadcast::new();
    let broadcast_data = web::Data::new(fill_broadcast);
    let app_state = web::Data::new(AppState {
        pool: sqlx::PgPool::connect(
            &std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://clob:clob@localhost:5432/clob".into()),
        )
        .await
        .context("follower db pool")?,
        producer: app_producer,
        book_cache: app_book_cache,
        fills_cache: app_fills_cache,
    });

    let health_for_http = Arc::clone(&shared_health);
    let http_bind_clone = http_bind.clone();
    tokio::spawn(async move {
        info!("follower HTTP listening on {}", http_bind_clone);
        let _ = HttpServer::new(move || {
            App::new()
                .wrap(MetricsMiddleware)
                .app_data(app_state.clone())
                .app_data(broadcast_data.clone())
                .app_data(web::Data::new(Arc::clone(&health_for_http)))
                .service(handlers::health::health)
                .service(handlers::markets::get_book)
                .service(handlers::markets::get_trades)
                .service(handlers::health::healthz)
        })
        .bind(&http_bind_clone)
        .expect("failed to bind follower HTTP")
        .run()
        .await;
    });

    // Watch for promotion — when elected leader, restart as leader.
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if is_leader.load(Ordering::Acquire) {
            info!("follower promoted to leader — process restart required for full leader mode");
            // In K8s the pod will be restarted; in local mode log and exit.
            tracing_setup::shutdown();
            std::process::exit(0);
        }
    }
}
