CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

CREATE TABLE IF NOT EXISTS orders (
    id           UUID PRIMARY KEY,
    market_id    VARCHAR(32)  NOT NULL,
    side         VARCHAR(4)   NOT NULL,
    price        NUMERIC(20, 8),
    quantity     NUMERIC(20, 8) NOT NULL,
    status       VARCHAR(16)  NOT NULL,
    sequence     BIGINT       NOT NULL,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS fills (
    id              UUID PRIMARY KEY,
    maker_order_id  UUID          NOT NULL,
    taker_order_id  UUID          NOT NULL,
    price           NUMERIC(20, 8) NOT NULL,
    quantity        NUMERIC(20, 8) NOT NULL,
    sequence        BIGINT        NOT NULL,
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS snapshots (
    id          BIGSERIAL PRIMARY KEY,
    market_id   VARCHAR(32) NOT NULL,
    sequence    BIGINT      NOT NULL,
    state       BYTEA       NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_orders_market_status ON orders   (market_id, status);
CREATE INDEX IF NOT EXISTS idx_orders_sequence      ON orders   (sequence);
CREATE INDEX IF NOT EXISTS idx_fills_sequence       ON fills    (sequence);
CREATE INDEX IF NOT EXISTS idx_snapshots_market_seq ON snapshots(market_id, sequence DESC);