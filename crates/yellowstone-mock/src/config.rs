#[derive(Debug, Clone)]
pub struct Config {
    pub stream_rate_per_second: u64,
    pub payload_size_bytes: usize,
    pub simulate_disconnects: bool,
    pub disconnect_interval_s: u64,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            stream_rate_per_second: env_u64("STREAM_RATE_PER_SECOND", 1000),
            payload_size_bytes: env_usize("PAYLOAD_SIZE_BYTES", 512),
            simulate_disconnects: env_bool("SIMULATE_DISCONNECTS", false),
            disconnect_interval_s: env_u64("DISCONNECT_INTERVAL_S", 30),
            port: env_u16("GRPC_PORT", 10000),
        }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"))
        .unwrap_or(default)
}

fn env_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
