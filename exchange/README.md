# Exchange Service Template (Rust)

Rust template for an exchange core service, targeted for a single EC2 deployment for an internal competition.

## Included template features

- Axum HTTP server (REST + WS)
- Operator-provisioned competition users via `POST /api/v1/admin/users`
- Simple user auth via assigned `x-api-key`
- Simple admin auth via `Authorization: Bearer $ADMIN_API_TOKEN`
- Per-user `100 ops/sec` rate limiting on authenticated REST account/trading routes
- Matching engine + in-memory orderbook skeleton
- PostgreSQL-oriented repository abstraction for user/account/order/fill state
- Background PostgreSQL writer thread with bounded queue, batch flushing, and retry/backpressure telemetry
- OpenAPI docs + Swagger UI at `/docs`
- REST endpoints for trader visibility:
  - `GET /api/v1/user`
  - `GET /api/v1/balance`
  - `GET /api/v1/portfolio`
  - `GET /api/v1/open-orders`
  - `GET /api/v1/fills`
- REST endpoints for order entry:
  - `POST /api/v1/orders`
  - `PATCH /api/v1/orders/{order_id}`
  - `DELETE /api/v1/orders/{order_id}`
- WebSocket endpoint for market data and trading events:
  - `GET /ws`
  - market-data flow:
    - send `{"op":"subscribe","channel":"l3","market":"BTC-USD"}`
    - receive one snapshot
    - then receive live sequenced deltas only
    - if the server detects a gap or receiver lag, it sends `resync_required`
    - client should resubscribe to get a fresh snapshot; no replay endpoint is provided
  - authenticated socket flow:
    - send `{"op":"authenticate","api_key":"..."}`
    - receive `authenticated` acknowledgement for the competition user
    - send `submit_order`, `cancel_order`, and `amend_order` messages
    - receive `ack` / `reject` replies plus user-scoped `fill` and `order_state` events
- Socket-level integration tests cover auth, subscribe, submit/amend/cancel, and crossing-trade user-event delivery.

## Authentication model

- Users do not self-register.
- Operators provision competition users directly.
- Each provisioned user receives a unique API key.
- That API key is both the user identity and the API access credential.
- User-facing REST routes authenticate with `x-api-key`.
- WebSocket authentication uses the same assigned API key.
- Admin routes use a separate configured bearer token.

## Storage direction

- Matching remains in memory.
- Durable account, order, fill, and audit data can be routed to local PostgreSQL.
- `STORAGE_BACKEND=postgres` enables the PostgreSQL-backed repository.
- The PostgreSQL backend keeps an in-memory cache for reads and pushes writes to a dedicated background writer thread.
- The background writer uses a bounded queue plus transaction batches so the exchange path does not perform direct database writes.
- The writer retries failed batches in order, applies backpressure by blocking enqueue when the queue is saturated, and reports queue/flush health through `/health`.
- Startup recovery rebuilds in-memory orderbooks from persisted open orders before the exchange begins serving traffic.
- The initial schema lives at `sql/migrations/001_initial.sql`.

## Current deployed test endpoint

- HTTP base: `http://16.59.150.9:8080`
- Health: `http://16.59.150.9:8080/health`
- Swagger docs: `http://16.59.150.9:8080/docs`
- WebSocket: `ws://16.59.150.9:8080/ws`
- This is currently plain HTTP/WS for internal testing. TLS is not configured yet.

## Run locally

```bash
cargo run
```

Then open:

- `http://localhost:8080/health`
- `http://localhost:8080/docs`

Key environment variables:

- `ADMIN_API_TOKEN`
- `STORAGE_BACKEND=in_memory|postgres`
- `DATABASE_URL`
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

1. Extend startup recovery into full reconciliation and settlement recovery on top of persisted local PostgreSQL data
2. Add deterministic matching tests and replay tests
3. Extend restart recovery into full balance, fill, and settlement reconciliation
4. Add explicit account-refresh guidance for user-event resync cases
5. Write the internal operator and competition-user runbook
