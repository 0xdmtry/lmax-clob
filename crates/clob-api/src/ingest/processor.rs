use crate::metrics::ENGINE_CHANNEL_DEPTH;
use metrics::gauge;
use std::sync::Arc;
use std::time::Instant;

use crossbeam_channel::{bounded, Receiver, Sender};
use metrics::{counter, histogram};
use rust_decimal::Decimal;
use tracing::{info, info_span, warn};
use uuid::Uuid;
use yellowstone_grpc_proto::geyser::SubscribeUpdate;

use clob_engine::{
    events::Fill,
    matching::{MatchingEngine, ProcessResult},
    order::{Order, OrderStatus},
    snapshot::BookSnapshot,
    types::{OrderType, Price, Quantity, Side},
};

use crate::metrics::{FILLS_PER_SECOND, MATCHING_LATENCY_NS, SNAPSHOT_SIZE_BYTES};
use crate::persistence::snapshot::SnapshotStore;

const MARKET_ID: &str = "SOL/USDC";

#[derive(Debug, Clone)]
pub struct BookUpdate {
    pub market_id: String,
    pub best_bid: Option<Decimal>,
    pub best_ask: Option<Decimal>,
}

pub fn map_update_to_order(update: SubscribeUpdate) -> Order {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let slot = update.created_at.map(|t| t.nanos as u64).unwrap_or(seq);

    let (side, price) = if seq % 2 == 0 {
        (Side::Bid, Decimal::new(101, 0))
    } else {
        (Side::Ask, Decimal::new(100, 0))
    };

    Order {
        id: Uuid::new_v4(),
        market_id: MARKET_ID.to_string(),
        side,
        order_type: OrderType::Limit,
        price: Some(Price::from(price)),
        quantity: Quantity::from(Decimal::new(1, 0)),
        filled: Quantity::ZERO,
        status: OrderStatus::Open,
        sequence: slot,
    }
}

pub fn start_engine_thread(
    snapshot_store: Arc<SnapshotStore>,
    initial_snap: Option<BookSnapshot>,
    health: crate::health::SharedHealth,
) -> (Sender<Order>, Receiver<Fill>, Receiver<BookUpdate>) {
    let order_cap: usize = std::env::var("ENGINE_CHANNEL_DEPTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8_192);
    let snapshot_every: u64 = std::env::var("SNAPSHOT_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let (order_tx, order_rx): (Sender<Order>, Receiver<Order>) = bounded(order_cap);
    let (fill_tx, fill_rx): (Sender<Fill>, Receiver<Fill>) = bounded(8_192);
    let (book_tx, book_rx): (Sender<BookUpdate>, Receiver<BookUpdate>) = bounded(8_192);

    std::thread::Builder::new()
        .name("matching-engine".into())
        .spawn(move || {
            let mut engine = MatchingEngine::new(MARKET_ID);

            health
                .engine_alive
                .store(true, std::sync::atomic::Ordering::Release);

            if let Some(snap) = initial_snap {
                match postcard::to_allocvec(&snap) {
                    Ok(bytes) => match clob_engine::snapshot::deserialize_book(&bytes) {
                        Ok((book, _)) => {
                            engine.book = book;
                            info!("book restored from snapshot");
                        }
                        Err(e) => warn!("failed to restore book: {}", e),
                    },
                    Err(e) => warn!("failed to encode snapshot for restore: {}", e),
                }
            }

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("mini runtime");

            let mut processed: u64 = 0;
            let mut last_offset: i64 = 0;

            loop {
                match order_rx.recv() {
                    Ok(order) => {
                        gauge!(ENGINE_CHANNEL_DEPTH).set(order_rx.len() as f64);
                        last_offset = order.sequence as i64;
                        let mut had_fills = false;

                        let span = info_span!(
                            "match_order",
                            order_id  = %order.id,
                            market_id = %order.market_id,
                            sequence  = order.sequence,
                        );
                        let _enter = span.enter();

                        let t0 = Instant::now();
                        match engine.process_order(order) {
                            Ok(ProcessResult::Fills(fills)) => {
                                had_fills = true;
                                let n = fills.len();
                                counter!(FILLS_PER_SECOND).increment(n as u64);
                                for fill in fills {
                                    if fill_tx.send(fill).is_err() {
                                        health
                                            .engine_alive
                                            .store(false, std::sync::atomic::Ordering::Release);
                                        warn!("fill channel closed");
                                        return;
                                    }
                                }
                                info!("matched {} fills", n);
                            }
                            Ok(ProcessResult::Resting) => {}
                            Ok(ProcessResult::Cancelled) => {}
                            Err(e) => warn!("engine error: {}", e),
                        }
                        histogram!(MATCHING_LATENCY_NS).record(t0.elapsed().as_nanos() as f64);

                        if had_fills {
                            let update = BookUpdate {
                                market_id: MARKET_ID.to_string(),
                                best_bid: engine.book.best_bid(),
                                best_ask: engine.book.best_ask(),
                            };
                            book_tx.send(update).ok();
                        }

                        processed += 1;
                        if processed % snapshot_every == 0 {
                            let offset = last_offset;
                            let store = Arc::clone(&snapshot_store);
                            match clob_engine::snapshot::serialize_book(
                                &engine.book,
                                MARKET_ID,
                                processed,
                            ) {
                                Ok(bytes) => {
                                    histogram!(SNAPSHOT_SIZE_BYTES).record(bytes.len() as f64);
                                    let snap_t0 = std::time::Instant::now();
                                    rt.block_on(async move {
                                        if let Err(e) =
                                            store.save_raw(&bytes, offset, processed).await
                                        {
                                            warn!("snapshot save error: {}", e);
                                        }
                                    });
                                    histogram!(crate::metrics::SNAPSHOT_WRITE_LATENCY_NS)
                                        .record(snap_t0.elapsed().as_nanos() as f64);
                                }
                                Err(e) => warn!("serialize_book error: {}", e),
                            }
                        }
                    }
                    Err(_) => {
                        health
                            .engine_alive
                            .store(false, std::sync::atomic::Ordering::Release);
                        info!("order channel closed, engine thread exiting");
                        return;
                    }
                }
            }
        })
        .expect("failed to spawn matching-engine thread");

    (order_tx, fill_rx, book_rx)
}
