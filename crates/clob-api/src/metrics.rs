use metrics::{describe_counter, describe_gauge, describe_histogram, Unit};

pub const MATCHING_LATENCY_NS: &str = "matching_latency_ns";
pub const KAFKA_CONSUMER_LAG: &str = "kafka_consumer_lag";
pub const FILLS_PER_SECOND: &str = "fills_total";
pub const SNAPSHOT_SIZE_BYTES: &str = "snapshot_size_bytes";
pub const DB_WRITE_BATCH_SIZE: &str = "db_write_batch_size";
pub const WS_CLIENT_COUNT: &str = "ws_client_count";
pub const HTTP_REQUEST_LATENCY: &str = "http_request_duration_ns";
pub const ENGINE_CHANNEL_DEPTH: &str = "engine_channel_depth";
pub const KAFKA_PRODUCE_LATENCY_NS: &str = "kafka_produce_latency_ns";
pub const DB_WRITE_LATENCY_NS: &str = "db_write_latency_ns";
pub const SNAPSHOT_WRITE_LATENCY_NS: &str = "snapshot_write_latency_ns";

pub fn register_all() {
    describe_histogram!(
        MATCHING_LATENCY_NS,
        Unit::Nanoseconds,
        "Time to process one order through the matching engine"
    );
    describe_gauge!(
        KAFKA_CONSUMER_LAG,
        Unit::Count,
        "Kafka consumer lag on orders-in topic"
    );
    describe_counter!(
        FILLS_PER_SECOND,
        Unit::Count,
        "Total fills produced by the matching engine"
    );
    describe_histogram!(
        SNAPSHOT_SIZE_BYTES,
        Unit::Bytes,
        "Size of serialized book snapshot"
    );
    describe_histogram!(
        DB_WRITE_BATCH_SIZE,
        Unit::Count,
        "Number of fills in each DB batch insert"
    );
    describe_gauge!(
        WS_CLIENT_COUNT,
        Unit::Count,
        "Number of connected WebSocket clients"
    );
    describe_histogram!(
        HTTP_REQUEST_LATENCY,
        Unit::Nanoseconds,
        "HTTP request duration per endpoint"
    );
    describe_gauge!(
        ENGINE_CHANNEL_DEPTH,
        Unit::Count,
        "Current depth of the order channel into the matching engine"
    );
    describe_histogram!(
        KAFKA_PRODUCE_LATENCY_NS,
        Unit::Nanoseconds,
        "Kafka producer send latency per message"
    );
    describe_histogram!(
        DB_WRITE_LATENCY_NS,
        Unit::Nanoseconds,
        "Time to execute one fill batch insert into Postgres"
    );
    describe_histogram!(
        SNAPSHOT_WRITE_LATENCY_NS,
        Unit::Nanoseconds,
        "Time to serialize and persist one book snapshot to Postgres"
    );
}
