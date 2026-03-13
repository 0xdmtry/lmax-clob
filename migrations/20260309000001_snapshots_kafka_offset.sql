ALTER TABLE snapshots
    ADD COLUMN IF NOT EXISTS kafka_offset BIGINT NOT NULL DEFAULT 0;

CREATE UNIQUE INDEX IF NOT EXISTS idx_snapshots_market_id_unique
    ON snapshots (market_id);