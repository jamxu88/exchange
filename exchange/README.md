# Exchange Service Template (Rust)

Rust template for an exchange core service, targeted for a single EC2 deployment for an internal competition.

Canonical public API docs now live in `docs/` as a Mintlify site. Internal-only docs now live in `internal-docs/`.

## Included template features

- Axum HTTP server (REST + WS)
- Operator-provisioned competition users via `POST /api/v1/admin/users`
- Provisioned user roster export via `GET /api/v1/admin/users` and `GET /api/v1/admin/users/export.csv`
- Simple user auth via assigned `x-api-key`
- Simple admin auth via `Authorization: Bearer $ADMIN_API_TOKEN`
- Operator control-plane REST endpoints for:
  - exchange-wide start / stop trading
  - market create / patch / delete
  - market enable / disable / settle
  - bulk config load
  - admin messages
  - leaderboard queries
- Per-user token-bucket rate limiting on authenticated competitor operations, defaulting to `500 ops per 10s` and shared across REST account/trading routes and authenticated WebSocket trading messages
- Matching engine + in-memory orderbook skeleton
- PostgreSQL-oriented repository abstraction for user/position/order/fill state
- Background PostgreSQL writer thread with bounded queue, batch flushing, and retry/backpressure telemetry
- Canonical per-market order-event stream inside the exchange core
- Derived browser market-data feed built from that canonical stream
- Optional external market-data service over a local Unix socket, with the core WebSocket API bridged onto it
- Dedicated market-data broadcast worker threads with per-market delta batching and fanout separate from the main exchange path
- OpenAPI docs + Swagger UI at `/docs`
- REST endpoints for trader visibility:
  - `GET /api/v1/markets`
  - `GET /api/v1/user`
  - `GET /api/v1/positions`
  - `GET /api/v1/portfolio`
  - `GET /api/v1/leaderboard`
  - `GET /api/v1/open-orders`
  - `GET /api/v1/fills`
- REST endpoints for order entry:
  - `POST /api/v1/orders`
  - `PATCH /api/v1/orders/{order_id}`
  - `DELETE /api/v1/orders/{order_id}`
- REST endpoints for operator workflows:
  - `GET /api/v1/admin/state`
  - `GET|POST /api/v1/admin/users`
  - `GET /api/v1/admin/users/export.csv`
  - `POST /api/v1/admin/trading/start`
  - `POST /api/v1/admin/trading/stop`
  - `GET|POST /api/v1/admin/markets`
  - `PATCH|DELETE /api/v1/admin/markets/{market_id}`
  - `POST /api/v1/admin/markets/{market_id}/settle`
  - `POST /api/v1/admin/config/load`
  - `GET|POST /api/v1/admin/messages`
  - `POST /api/v1/admin/users/reset`
  - `GET /api/v1/admin/leaderboard`
- WebSocket endpoint for market data and trading events:
  - `GET /ws`
  - public market-data flow:
    - send `{"op":"subscribe","channel":"l2","market":"BTC-USD"}`
    - receive one aggregated `snapshot`
    - then receive live sequenced `delta` batches with `start_sequence` and `sequence`, usually flushed around `100ms`
    - if the server detects a gap or receiver lag, it sends `resync_required`
    - client should resubscribe to get a fresh snapshot; no replay endpoint is provided
  - authenticated raw market-data flow:
    - send `{"op":"authenticate","api_key":"..."}`
    - send `{"op":"subscribe","channel":"l3","market":"BTC-USD"}`
    - receive one raw per-order `l3_snapshot`
    - then receive live sequenced `l3_delta` messages carrying canonical public order events
    - if the server detects a gap or receiver lag, it sends `resync_required`
    - client should resubscribe to get a fresh snapshot; no replay endpoint is provided
  - authenticated socket flow:
    - send `{"op":"authenticate","api_key":"..."}`
    - receive `authenticated` acknowledgement for the competition user
    - send `submit_order`, `cancel_order`, and `amend_order` messages
    - receive `ack` / `reject` replies plus user-scoped `fill`, `order_state`, and `admin_message` events
- Socket-level integration tests cover auth, subscribe, submit/amend/cancel, and crossing-trade user-event delivery.

## Authentication model

- Users do not self-register.
- Operators provision competition users directly.
- Each provisioned user receives a unique API key.
- Operators can export provisioned user credentials as JSON or CSV, with optional `username_prefix`, `role`, and `limit` filters.
- That API key is both the user identity and the API access credential.
- User-facing REST routes authenticate with `x-api-key`.
- WebSocket authentication uses the same assigned API key.
- Admin routes use a separate configured bearer token.

## Storage direction

- Matching remains in memory.
- Durable position, order, fill, and audit data can be routed to local PostgreSQL.
- Exchange controls, market definitions, and admin messages are persisted through the same storage boundary.
- `STORAGE_BACKEND=postgres` enables the PostgreSQL-backed repository.
- The PostgreSQL backend keeps an in-memory cache for reads and pushes writes to a dedicated background writer thread.
- The background writer uses a bounded queue plus transaction batches so the exchange path does not perform direct database writes.
- The writer retries failed batches in order, applies backpressure by blocking enqueue when the queue is saturated, and reports queue/flush health through `/health`.
- User risk is position-based, with a fixed per-market net position limit of `+/-1000`.
- Traders can buy from flat, sell from flat, go long, and go short; there is no inventory pre-seeding requirement to place a sell order.
- Realized PnL accumulates as positions are reduced, flipped, or settled.
- Startup recovery rebuilds in-memory orderbooks from persisted open orders before the exchange begins serving traffic.
- The initial schema lives at `sql/migrations/001_initial.sql`.
- Provisioned users can now carry `role=admin`, which removes the fixed per-market net position cap for that API key.
- A zero-dependency multi-trader stress harness lives at `tools/trader-stress-bot/`.

## Current deployed test endpoint

- HTTP base: `https://exchange.jamesxu.dev`
- Health: `https://exchange.jamesxu.dev/health`
- Swagger docs: `https://exchange.jamesxu.dev/docs`
- WebSocket: `wss://exchange.jamesxu.dev/ws`
- ALB hostname: `exchange-client-alb-1466111370.us-east-2.elb.amazonaws.com`
- The public edge is the ALB path split, with backend exchange routes forwarded to the EC2 service.

## Docs

- Public Mintlify API docs root: `docs/`
- Internal-only notes root: `internal-docs/`
- Preview the public docs locally with `cd docs && npx mint validate` or `cd docs && npx mintlify dev`

## Current EC2 deployment

- Host: `ec2-user@16.59.150.9`
- Repo path: `/home/ec2-user/exchange-v2`
- Exchange binary: `/home/ec2-user/exchange-v2/exchange/target/release/exchange`
- Exchange env file: `/home/ec2-user/exchange-v2/exchange.env`
- Service names: `exchange`, `market-data-service`
- Data store: local PostgreSQL on the same EC2 machine
- Source of truth for code updates: GitHub `origin/main`

## Update the deployed EC2 host from GitHub

Push locally first:

```bash
git push origin main
```

Then update the EC2 host:

```bash
ssh -i "quant-exchange.pem" ec2-user@16.59.150.9 '
  cd ~/exchange-v2 &&
  git pull --ff-only &&
  cd exchange &&
  source "$HOME/.cargo/env" &&
  cargo build --release --bin exchange --bin market-data-service &&
  sudo systemctl restart market-data-service &&
  sudo systemctl restart exchange &&
  sudo systemctl status market-data-service --no-pager &&
  sudo systemctl status exchange --no-pager
'
```

Verify the live service:

```bash
curl https://exchange.jamesxu.dev/health
```

Current note: GitHub access on the EC2 host is temporarily configured with a stored PAT. Replace that with a GitHub deploy key or machine-user SSH key, then revoke the PAT.

## Run locally

```bash
cargo run --bin exchange
```

To run the first split-process setup locally:

```bash
export MARKET_DATA_SERVICE_SOCKET=/tmp/exchange-marketdata.sock
cargo run --bin market-data-service &
cargo run --bin exchange
```

Then open:

- `http://localhost:8080/health`
- `http://localhost:8080/docs`

Key environment variables:

- `ADMIN_API_TOKEN`
- `STORAGE_BACKEND=in_memory|postgres`
- `DATABASE_URL`
- `MARKET_DATA_SERVICE_SOCKET`
  Optional Unix socket path for the external market-data service bridge. If unset, the exchange keeps using the in-process derived feed.
- `MARKET_DATA_SERVICE_RETRY_BACKOFF_MS`
- `WS_MARKET_DELTA_BATCH_INTERVAL_MS`
  Browser-market-data flush interval in milliseconds. Defaults to `100`.
- `WS_MARKET_BROADCAST_WORKERS`
- `PER_USER_RATE_LIMIT_BURST_CAPACITY`
- `PER_USER_RATE_LIMIT_BURST_WINDOW_SECONDS`
- `POSTGRES_WRITE_BATCH_SIZE`
- `POSTGRES_WRITE_FLUSH_INTERVAL_MS`
- `POSTGRES_WRITE_QUEUE_CAPACITY`
- `POSTGRES_WRITE_RETRY_BACKOFF_MS`

## Testing and latency checks

- Run the standard test suite:
  - `cargo test`
- Run runtime-sensitive smoke checks (ignored by default):
  - `cargo test --test latency_smoke -- --ignored`
- Run micro-benchmarks for matching and REST path latency:
  - `cargo bench --bench latency`

## Next implementation priorities

1. Extend startup recovery from lock reconciliation into full replay-based settlement recovery on top of persisted local PostgreSQL data
2. Decide whether the browser client should move order submit / cancel / amend onto the existing WS trading protocol or keep REST for ticket actions
3. Open public port `80` if automatic `http` to `https` redirects are required
4. Load test the deployed EC2 stack under competition-like traffic
5. Replace the temporary GitHub PAT on the EC2 host with a deploy key or machine-user SSH key
