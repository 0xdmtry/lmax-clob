use anyhow::Result;
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

use clob_engine::{
    matching::{MatchingEngine, ProcessResult},
    order::Order,
    snapshot::BookSnapshot,
};

use crate::persistence::snapshot::SnapshotStore;

const MARKET_ID: &str = "SOL/USDC";

/// Follower engine: consumes orders from Kafka (via order_tx re-use pattern),
/// replays into in-memory book only — no DB, no Redis, no WS, no fills-out.
pub fn start_follower_engine(
    snapshot_store: Arc<SnapshotStore>,
    initial_snap: Option<BookSnapshot>,
    is_leader: Arc<AtomicBool>,
    health: crate::health::SharedHealth,
) -> Sender<Order> {
    let (order_tx, order_rx) = crossbeam_channel::bounded::<Order>(8_192);

    std::thread::Builder::new()
        .name("follower-engine".into())
        .spawn(move || {
            let mut engine = MatchingEngine::new(MARKET_ID);

            health.engine_alive.store(true, std::sync::atomic::Ordering::Release);

            if let Some(snap) = initial_snap {
                match postcard::to_allocvec(&snap) {
                    Ok(bytes) => match clob_engine::snapshot::deserialize_book(&bytes) {
                        Ok((book, _)) => {
                            engine.book = book;
                            info!("follower: book restored from snapshot");
                        }
                        Err(e) => warn!("follower: failed to restore book: {}", e),
                    },
                    Err(e) => warn!("follower: failed to encode snapshot: {}", e),
                }
            }

            let mut processed: u64 = 0;

            loop {
                // If we've become leader mid-run, exit so main promotes to leader mode.
                if is_leader.load(Ordering::Acquire) {
                    info!("follower engine detected promotion to leader, exiting thread");
                    return;
                }

                match order_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                    Ok(order) => {
                        match engine.process_order(order) {
                            Ok(ProcessResult::Fills(_))
                            | Ok(ProcessResult::Resting)
                            | Ok(ProcessResult::Cancelled) => {}
                            Err(e) => warn!("follower engine error: {}", e),
                        }
                        processed += 1;
                        if processed % 10_000 == 0 {
                            info!("follower replayed {} orders", processed);
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        health.engine_alive.store(false, std::sync::atomic::Ordering::Release);
                        info!("follower order channel closed, exiting");
                        return;
                    }
                }
            }
        })
        .expect("failed to spawn follower-engine thread");

    order_tx
}
