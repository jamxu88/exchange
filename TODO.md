# Exchange TODO

## Current Position

This repo is an early exchange backend prototype in Rust, intended to run on EC2 for an internal competition.

What exists today:

- Rust backend service in `exchange/`
- Axum-based REST server
- WebSocket market-data endpoint with L3 snapshot + delta flow
- WebSocket API-key auth handshake for competition users
- In-memory matching engine and orderbook
- Local trading service for submit / cancel / amend
- Position-based risk model with per-market net limit enforcement
- Local auth system with operator-provisioned users, simple admin bearer-token auth, and assigned API-key auth for competition users
- Admin provisioning audit logging through the storage layer
- Persisted operator control plane for:
  - exchange-wide trading on/off
  - market create / delete / enable / disable / settle
  - bulk config load
  - admin broadcast / user-targeted messages
  - leaderboard generation
- Per-user in-memory rate limiting on authenticated REST account and trading routes
- PostgreSQL-oriented repository abstraction for positions, open orders, fills, identity lookups, and admin audit records
- In-memory repository backend plus a live PostgreSQL repository backend behind the same boundary
- Background PostgreSQL writer thread with batched flushes, retry/backpressure handling, and health telemetry behind the storage boundary
- EC2-hosted internal test deployment is live at `http://16.59.150.9:8080` with WS at `ws://16.59.150.9:8080/ws`
- Public browser-safe access is live at `https://exchange.jamesxu.dev` with WS at `wss://exchange.jamesxu.dev/ws`
- ECS client stack is live behind the ALB and serving `https://exchange.jamesxu.dev`
- GitHub is now the source of truth for deployment updates, and the EC2 host runs from a Git clone at `~/exchange-v2`
- Public Mintlify API docs now exist under `docs/`, and internal-only notes now live under `internal-docs/`
- OpenAPI generation and Swagger UI
- Tests and latency checks for core paths

Target constraints:

- This is for an internal competition, not a public exchange product
- Users do not self-register
- Operators provision competition accounts
- Each participant should receive a unique assigned API key that serves as both login identity and API access
- Matching remains in memory inside the exchange process
- Durable position, order, fill, and audit data should live in local PostgreSQL on the EC2 machine
- PostgreSQL writes must happen off the main exchange thread and be flushed in batches
- HA, multi-region failover, and public-internet hardening are out of scope for now

What is still true:

- Persistence and recovery are only partially implemented
- Settlement now persists a journal, but full replay-based recovery is still incomplete
- WS trading is implemented end-to-end, but restart reconciliation is still incomplete
- Startup recovery now rebuilds in-memory orderbooks from persisted open orders, while positions remain available from storage-backed reads
- The backend admin control plane is implemented, and the Next.js client now validates login keys against the backend and uses the live admin/trader surfaces

## Progress Snapshot

### Done

- Rust service skeleton is in place
- Health endpoint exists
- REST endpoints exist for:
  - `POST /api/v1/admin/users`
  - `GET /api/v1/admin/state`
  - `POST /api/v1/admin/trading/start`
  - `POST /api/v1/admin/trading/stop`
  - `GET|POST /api/v1/admin/markets`
  - `PATCH|DELETE /api/v1/admin/markets/{market_id}`
  - `POST /api/v1/admin/markets/{market_id}/settle`
  - `POST /api/v1/admin/config/load`
  - `GET|POST /api/v1/admin/messages`
  - `GET /api/v1/admin/leaderboard`
  - `GET /api/v1/markets`
  - `GET /api/v1/user`
  - `GET /api/v1/positions`
  - `GET /api/v1/portfolio`
  - `GET /api/v1/open-orders`
  - `GET /api/v1/fills`
  - `POST /api/v1/orders`
  - `PATCH /api/v1/orders/{order_id}`
  - `DELETE /api/v1/orders/{order_id}`
- Swagger docs are exposed at `/docs`
- WebSocket connection, heartbeat, and public aggregated data-stream snapshot + delta flow exist
- WebSocket `authenticate` handshake exists for assigned user API keys
- In-memory orderbook supports:
  - price priority
  - FIFO per price level
  - cancel by order id
  - amend remaining quantity
- Matching engine tests pass
- REST tests pass
- Admin provisioning endpoint is tested end-to-end
- Admin trading-control, market lifecycle, config-load, messaging, settlement, and leaderboard routes are tested end-to-end
- Admin reset-all-users route is tested end-to-end
- Local submit / cancel / amend flows are tested end-to-end
- Direct API-key trading flow is tested end-to-end
- Cross-account order isolation is tested end-to-end
- Register route removal is tested end-to-end
- Login route removal is tested end-to-end
- Per-user `100 ops/sec` REST rate limiting is tested end-to-end
- Position-limit admission and signed-position fill settlement are tested
- Admin provisioning is protected by a configured bearer token and emits audit records
- Storage access is routed through a repository layer instead of raw app-state maps
- Repository backend trait exists with in-memory and PostgreSQL backends
- PostgreSQL initial schema exists for users, api keys, balances, orders, fills, positions, pending positions, pnl snapshots, and admin audit logs
- PostgreSQL initial schema now also persists exchange controls, market definitions, and admin messages
- PostgreSQL repository writes are wired through a dedicated batched writer thread for identity, exchange controls, markets, admin messages, positions, orders, fills, and admin audit logs
- PostgreSQL writer health now exposes queue depth, flush latency, retry/failure counts, and degraded status through `/health`
- Startup recovery rebuilds in-memory orderbooks from persisted open orders
- Market configs now enforce trading-enabled state plus per-market enable/disable, tick size, and minimum order quantity
- Market settlement exists as an admin operation and flattens open net positions at a configured settlement price
- Public health and WebSocket auth/snapshot probes succeeded over Elastic IP `16.59.150.9`
- GitHub repo sync is in place for the EC2 host, and the deployed tree fast-forwards cleanly from `origin/main`
- ECS deployment assets now exist under `infra/client-ecs/`
- Matching hot path was improved:
  - lightweight execution records added
  - fewer hot-path lookups
  - benchmark added for matching on a prebuilt book

### Partial

- Authentication:
  - operators can already provision competition users
  - assigned API-key auth exists today for REST and WS
  - public self-registration is removed from the runtime
  - admin provisioning is protected by a configured bearer token
  - admin provisioning emits audit logs
  - no JWT verification, roles, or public-user auth features are required for the current competition scope
- Rate limiting:
  - authenticated REST account/trading routes enforce an in-memory per-user limiter
  - current policy is per-user at a maximum of `100 ops/sec`
  - no WS limiter yet
- WebSocket API:
  - connection lifecycle exists
  - heartbeat exists
  - API-key authentication handshake exists
  - public L3 snapshot + delta subscription exists for one market per connection
  - authenticated trading messages exist for submit / cancel / amend
  - `ack` / `reject` replies exist
  - user-scoped `fill` and `order_state` events exist
  - `resync_required` messages exist for market-data sequence gaps and lagged receivers
  - socket-level integration tests exist for auth, subscribe, trading, and user-event delivery
- Settlement and risk:
  - per-market `+/-1000` net position limit enforcement exists
  - signed-position fill application exists
  - settlement now realizes PnL and flattens open positions
  - full replay-based recovery is still not implemented
- Docs:
  - OpenAPI exists for the current REST surface
  - public Mintlify API docs now focus on the competitor workflow, market lifecycle, and per-endpoint REST/WS call pages
  - internal architecture, client, deployment, and runbook notes now live under `internal-docs/`
  - backup and restore guidance still needs deeper operational work
- Admin control plane:
  - backend operator endpoints now exist
  - client admin panel now reads live backend state and posts server actions to the operator endpoints
- Storage:
  - in-memory backend is still the default local app mode
  - PostgreSQL backend can be selected behind the same storage boundary
  - PostgreSQL schema is defined and executed by the live repository implementation
  - dedicated off-thread persistence, batched write flushing, and retry/backpressure telemetry are implemented
  - startup recovery rebuilds in-memory orderbooks from persisted open orders
  - settlement journaling is implemented
  - replay-based recovery is not implemented yet

### Not Started / Missing

- Order recovery from snapshots or journal replay
- Formal EC2 sizing, rollback, and local PostgreSQL backup plan
- Local PostgreSQL backup / restore and restart-recovery playbooks
- Full replay-based settlement recovery from durable state
- Real EC2 load test and saturation validation against competition-like traffic
- Post-cutover monitoring and rollback drill for the ALB-hosted client

## Main Workstreams

## 1. Core Architecture

- [x] Keep backend in Rust
- [x] Keep the exchange deployable as a single process on EC2 for now
- [x] Run PostgreSQL locally for the deployed exchange data store
- [x] Add environment-specific config
- [x] Add structured logging and tracing
- [x] Add a dedicated persistence worker thread / task
- [x] Add a bounded queue between the main exchange loop and PostgreSQL writes
- [x] Define batch flush policy:
  - max batch size
  - max flush interval
  - shutdown drain behavior
- [x] Add metrics for queue depth, flush latency, and DB write failures

## 2. Matching Engine and Orderbook State

### Current

- In-memory orderbook exists
- Matching works for simple limit-order crossing
- Benchmarking exists
- Recent hot-path cleanup is done

### Next

- [ ] Split engine-internal order structs from API-facing `Order`
- [ ] Remove heavy fields from resting order nodes
- [ ] Decide on long-term side structure:
  - `HashMap + BTreeSet`
  - `BTreeMap`
  - specialized price ladder
- [ ] Add deterministic sequence numbers for all book events
- [ ] Add market-specific book lifecycle and recovery
- [ ] Add full cancel/amend semantics
- [ ] Add validation for:
  - tick size
  - lot size
  - price bands
  - self-trade prevention
- [ ] Add replay tests and determinism tests
- [ ] Add snapshot + journal recovery

## 3. WebSocket API

Trading should be via WS events.

### Current

- `/ws` exists
- heartbeat exists
- `authenticate` message exists for assigned API-key auth
- `l3` subscription returns current snapshot
- live orderbook/trade deltas are broadcast with per-market sequence numbers
- authenticated `submit_order`, `cancel_order`, and `amend_order` messages exist
- `ack` / `reject` replies exist
- user-scoped `fill` and `order_state` events exist
- user-scoped / broadcast `admin_message` events exist
- public market-data flow is implemented locally
- socket-level integration tests cover auth, subscribe, trading, and user-event delivery

### Required

- [ ] Versioned event schema
- [ ] Expand client subscription model beyond current single-market flow
- [ ] Expand market data channels using snapshot + delta only
- [x] Trading request channels
- [x] ACK / reject / error messages
- [x] Cancel / amend messages
- [x] Sequence numbers and gap detection
- [x] Reconnect + resync protocol
- [ ] Backpressure policy
- [ ] Define snapshot payload shape for initial subscribe / resubscribe
- [ ] Define delta payload shape with deterministic sequence numbers
- [ ] Reject API designs that depend on public registration or browser-style login sessions

### Trading via WS

- [x] Submit order event
- [x] Cancel order event
- [x] Amend order event
- [x] Order accepted event
- [x] Order rejected event
- [x] Fill event
- [x] Order state event
- [ ] Balance / margin update event if needed

## 4. Authentication

### Current

- Operators can provision competition users
- Assigned API-key auth exists for REST and WS
- Identity/account reads now go through a repository contract intended for PostgreSQL persistence
- API-key order entry is covered by integration tests
- Public self-registration is not exposed by the runtime

### Target

- Operators provision each competition participant directly
- Each participant gets a unique assigned API key
- The assigned API key is the participant login identity and API access credential
- No self-registration flow exists
- Auth stays intentionally simple for internal competition use

### Required

- [x] Define trader identity model
- [x] Add API key support for programmatic trading
- [x] Remove public self-registration from the exchange flow
- [x] Add operator/admin provisioning API or tooling for competition users
- [x] Make assigned API key the only competition-user auth mechanism
- [x] Remove username/password login from the runtime
- [x] Remove bearer-session dependency from REST and WS user auth
- [x] Simplify REST auth to direct assigned API-key auth
- [ ] Keep admin/operator auth simple and internal-only
- [x] Keep only the audit logging that is operationally useful for provisioning and access changes
- [ ] Add API-key rotation / revocation support if operators need it

## 5. Rate Limiting

### Current

- Authenticated REST account/trading routes use an in-memory per-user sliding-window limiter
- Current limit is `100 ops/sec` per authenticated user

### Required

- [x] Per-user rate limit at `100 ops/sec`
- [x] Apply the per-user limit across authenticated REST account and trading operations
- [ ] Decide whether market-data WS messages are counted separately from trading/account ops
- [ ] Separate REST and WS limits if needed
- [ ] Separate market-data and trading limits if needed
- [ ] Burst handling
- [ ] Add simple operator controls for suspending abusive keys if needed

## 6. Settlement

### Current

- Signed-position settlement exists for local trading flows
- Full replay-based recovery is still incomplete

### Required

- [x] Enforce per-market net position limit before accepting orders
- [x] Update signed positions and realized PnL on fill
- [x] Flatten positions on market settlement
- [x] Keep persisted positions queryable after restart
- [x] Handle persistence queue failures and backpressure safely
- [ ] Define idempotent settlement flow
- [ ] Add invariants and reconciliation jobs

## 7. Trading API

We need a trading API that allows competition users to programmatically interact with the exchange.

### Required

- [x] Define initial local trader-facing REST API surface
- [x] Support API-key trading locally
- [ ] Prefer WS for trading actions and state updates
- [ ] Define REST for account/admin/reference data only
- [ ] Do not expose historical market-data query endpoints
- [ ] Align account model to competition operations:
  - operator assigns unique API key
  - no user self-registration
  - no username/password login flow
- [ ] Decide whether raw API-key auth is enough or whether a lightweight secret/signature is still worth keeping
- [ ] Publish internal schemas and examples

## 8. REST API Attached to the Exchange

### Current

- Basic account visibility endpoints exist

### Required

- [ ] Keep health and account reads on REST
- [ ] Expand REST coverage for:
  - positions
  - portfolio
  - open orders
  - fills
  - market metadata
  - system status
- [ ] Decide whether any order-entry fallback should exist on REST
- [ ] Add proper pagination and filtering
- [ ] Add internal auth and rate-limit docs per endpoint

## 9. GET Balance, Portfolio, Open Orders, Fills

### Current

- Endpoints exist, backed by the storage repository
- In-memory storage is the default local mode
- PostgreSQL-backed reads/writes are available behind the repository boundary

### Required

- [x] Make local PostgreSQL the default source of truth for deployed account/query state
- [x] Move durable writes for orders, fills, position snapshots, and audit events onto the dedicated persistence worker
- [x] Batch durable writes with bounded size and latency thresholds
- [ ] Add market filtering for open orders
- [ ] Add pagination for fills and orders
- [ ] Add timestamps and cursors
- [ ] Add richer portfolio semantics if needed
- [ ] Ensure returned data matches WS event stream state
- [x] Rebuild in-memory orderbooks from persisted open orders after restart
- [x] Rebuild recovered orderbooks against persisted signed-position state after restart
- [ ] Add full replay-based reconciliation behavior from local PostgreSQL after restart

## 10. L3 Data

Trading will be via WS events, and L3 data needs to be first-class.

### Required

- [ ] Define L3 schema:
  - add
  - amend
  - cancel
  - execute
  - snapshot
- [ ] Add sequence numbers to every L3 event
- [ ] Add per-market snapshots
- [ ] Add replay/resync flow after disconnect
- [ ] Guarantee deterministic ordering
- [ ] Add tests for book reconstruction from event stream

## 11. Local PostgreSQL Persistence

Local PostgreSQL is the durable store for the internal competition deployment. It must not sit on the main matching thread.

### Required

- [x] Add PostgreSQL schema for:
  - traders
  - position snapshots
  - orders
  - fills
  - settlement journal
  - audit logs
- [x] Persist order and fill events in the PostgreSQL repository path
- [x] Run PostgreSQL locally on the EC2 machine for the deployed environment
- [x] Make the PostgreSQL-backed repository the default deployed mode
- [x] Add a dedicated persistence thread / task and bounded queue between exchange core and database writer
- [x] Batch writes for:
  - order acceptance
  - order state transitions
  - fills
  - positions / settlement events
  - audit records
- [x] Define flush triggers:
  - max batch size
  - max flush interval
  - queue pressure thresholds
- [x] Add backpressure, retry, and failure-handling policy for the persistence queue
- [x] Rebuild in-memory orderbooks from persisted open orders on startup
- [x] Persist settlement journal entries alongside position and realized-PnL mutations
- [x] Persist exchange controls, market definitions, and admin messages through the same writer path
- [ ] Add replayable snapshots and full recovery procedures
- [ ] Define local backup retention and restore drills
- [ ] Add restart reconciliation playbooks

## 12. Documentation

We need internal docs, not public-product polish.

### Required

- [x] System architecture doc
- [x] Internal auth and provisioning doc
- [x] REST API doc
- [x] WS trading protocol doc
- [x] L3 market data doc
- [x] Error codes and rejection reasons
- [x] Sequence / replay / recovery semantics
- [x] EC2 + local PostgreSQL deployment and operations doc
- [x] Document the current GitHub-based EC2 deploy/update workflow and live endpoint
- [ ] Backup / restore doc
- [x] Internal competition trader quickstart
- [x] Example clients

## Suggested Implementation Order

1. Simplify competition-user auth to the assigned API-key-only model
2. Tighten the WS trading protocol and L3 schema
3. Move PostgreSQL persistence off the main exchange thread and batch writes
4. Build the operator control plane needed to run the competition
5. Extend startup recovery into full reconciliation on top of persisted data
6. Wire the client login/admin/trader flows to the live backend surface
7. Add TLS and run production-like load tests

## Immediate Next Tasks

- [x] Collapse competition-user auth to assigned API-key-only
- [x] Move WS auth onto the assigned API-key identity model
- [x] Build the dedicated batched PostgreSQL writer path off the main exchange thread
- [x] Rebuild in-memory orderbooks from persisted open orders on startup
- [x] Add WS submit / cancel / amend trading messages with ack/reject replies
- [x] Add full socket-level WS integration tests
- [x] Add `resync_required` handling for market-data gaps and lag
- [x] Add queue depth, flush latency, and DB failure metrics
- [x] Persist positions through the background PostgreSQL writer
- [x] Keep positions available after startup recovery of open orders
- [x] Add backend endpoints for start / stop trading, market lifecycle, config load, settlement, admin messages, and leaderboard
- [x] Add admin reset-all-users control path
- [ ] Add full replay-based settlement recovery using local PostgreSQL
- [x] Make PostgreSQL the default deployed backend and validate it on the EC2 box
- [x] Validate API keys against the backend during client login
- [x] Wire the admin page and trade client to the live market / leaderboard / admin-message endpoints
- [x] Put TLS in front of the EC2 deployment and switch browser clients to `https://` + `wss://`
- [ ] Run a competition-like load test against the EC2 deployment
- [ ] Replace the temporary GitHub PAT on EC2 with a deploy key or machine-user SSH key
- [ ] Refactor engine-internal order representation
- [x] Document the internal architecture and operator runbook
