BEGIN;

CREATE TABLE IF NOT EXISTS users (
    trader_id UUID PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'TRADER',
    created_at TIMESTAMPTZ NOT NULL
);

ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'TRADER';

CREATE TABLE IF NOT EXISTS api_keys (
    api_key TEXT PRIMARY KEY,
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    UNIQUE (trader_id, api_key)
);

CREATE INDEX IF NOT EXISTS api_keys_trader_id_idx ON api_keys (trader_id);

CREATE TABLE IF NOT EXISTS exchange_controls (
    control_key TEXT PRIMARY KEY,
    trading_enabled BOOLEAN NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS markets (
    market_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    base_asset TEXT NOT NULL,
    quote_asset TEXT NOT NULL,
    tick_size BIGINT NOT NULL,
    min_order_quantity BIGINT NOT NULL,
    reference_price BIGINT,
    settlement_price BIGINT,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS markets_status_idx ON markets (status, market_id);

CREATE TABLE IF NOT EXISTS admin_audit_logs (
    audit_id UUID PRIMARY KEY,
    actor_username TEXT NOT NULL,
    action TEXT NOT NULL,
    target_username TEXT,
    target_trader_id UUID,
    details TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS admin_audit_logs_occurred_at_idx ON admin_audit_logs (occurred_at DESC);

CREATE TABLE IF NOT EXISTS admin_messages (
    message_id UUID PRIMARY KEY,
    target_username TEXT,
    target_trader_id UUID REFERENCES users(trader_id) ON DELETE SET NULL,
    market_id TEXT,
    level TEXT NOT NULL,
    title TEXT,
    body TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS admin_messages_created_at_idx ON admin_messages (created_at DESC);

CREATE TABLE IF NOT EXISTS balances (
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    asset TEXT NOT NULL,
    free BIGINT NOT NULL,
    locked BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (trader_id, asset)
);

CREATE TABLE IF NOT EXISTS settlement_journal (
    journal_id UUID PRIMARY KEY,
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    asset TEXT NOT NULL,
    free_delta BIGINT NOT NULL,
    locked_delta BIGINT NOT NULL,
    reason TEXT NOT NULL,
    order_id UUID,
    fill_id UUID,
    occurred_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS settlement_journal_trader_id_occurred_at_idx
    ON settlement_journal (trader_id, occurred_at DESC);

CREATE TABLE IF NOT EXISTS positions (
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    market TEXT NOT NULL,
    net_quantity BIGINT NOT NULL,
    average_entry_price BIGINT,
    realized_pnl BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (trader_id, market)
);

CREATE TABLE IF NOT EXISTS pending_positions (
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    market TEXT NOT NULL,
    side TEXT NOT NULL,
    quantity BIGINT NOT NULL,
    reserved_quote BIGINT,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (trader_id, market, side)
);

CREATE TABLE IF NOT EXISTS orders (
    order_id UUID PRIMARY KEY,
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    market TEXT NOT NULL,
    side TEXT NOT NULL,
    price BIGINT NOT NULL,
    quantity BIGINT NOT NULL,
    remaining BIGINT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS orders_trader_id_created_at_idx ON orders (trader_id, created_at DESC);
CREATE INDEX IF NOT EXISTS orders_trader_id_market_status_idx ON orders (trader_id, market, status);

CREATE TABLE IF NOT EXISTS fills (
    fill_id UUID PRIMARY KEY,
    market TEXT NOT NULL,
    maker_order_id UUID NOT NULL REFERENCES orders(order_id) ON DELETE CASCADE,
    taker_order_id UUID NOT NULL REFERENCES orders(order_id) ON DELETE CASCADE,
    price BIGINT NOT NULL,
    quantity BIGINT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS fills_maker_order_id_idx ON fills (maker_order_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS fills_taker_order_id_idx ON fills (taker_order_id, occurred_at DESC);

CREATE TABLE IF NOT EXISTS pnl_snapshots (
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    market TEXT NOT NULL,
    realized_pnl BIGINT NOT NULL,
    unrealized_pnl BIGINT NOT NULL,
    captured_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (trader_id, market, captured_at)
);

CREATE INDEX IF NOT EXISTS pnl_snapshots_trader_id_market_idx ON pnl_snapshots (trader_id, market, captured_at DESC);

CREATE TABLE IF NOT EXISTS competition_leaderboard_snapshots (
    snapshot_id UUID PRIMARY KEY,
    competition_id TEXT NOT NULL,
    label TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS competition_leaderboard_snapshots_competition_id_created_at_idx
    ON competition_leaderboard_snapshots (competition_id, created_at DESC, snapshot_id DESC);

CREATE TABLE IF NOT EXISTS competition_leaderboard_snapshot_rows (
    snapshot_id UUID NOT NULL REFERENCES competition_leaderboard_snapshots(snapshot_id) ON DELETE CASCADE,
    rank BIGINT NOT NULL,
    trader_id UUID NOT NULL REFERENCES users(trader_id) ON DELETE CASCADE,
    username TEXT NOT NULL,
    net_pnl BIGINT NOT NULL,
    realized_pnl BIGINT NOT NULL,
    unrealized_pnl BIGINT NOT NULL,
    gross_exposure BIGINT NOT NULL,
    PRIMARY KEY (snapshot_id, rank),
    UNIQUE (snapshot_id, trader_id)
);

CREATE INDEX IF NOT EXISTS competition_leaderboard_snapshot_rows_snapshot_id_idx
    ON competition_leaderboard_snapshot_rows (snapshot_id, rank);

COMMIT;
