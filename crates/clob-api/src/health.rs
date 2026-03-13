use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared health state updated by engine thread and leader elector.
pub struct HealthState {
    pub engine_alive: AtomicBool,
    pub snapshot_loaded: AtomicBool,
    pub lease_healthy: AtomicBool,
    pub kafka_ready: AtomicBool,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            engine_alive: AtomicBool::new(false),
            snapshot_loaded: AtomicBool::new(false),
            lease_healthy: AtomicBool::new(false),
            kafka_ready: AtomicBool::new(false),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.engine_alive.load(Ordering::Acquire)
            && self.lease_healthy.load(Ordering::Acquire)
            && self.kafka_ready.load(Ordering::Acquire)
    }

    pub fn is_live(&self) -> bool {
        self.engine_alive.load(Ordering::Acquire)
    }
}

pub type SharedHealth = Arc<HealthState>;

pub fn new_shared() -> SharedHealth {
    Arc::new(HealthState::new())
}
