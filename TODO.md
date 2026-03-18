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
- In-memory settlement and balance locking for spot-style markets
- Local auth system with operator-provisioned users, simple admin bearer-token auth, and assigned API-key auth for competition users
- Admin provisioning audit logging through the storage layer
- Per-user in-memory rate limiting on authenticated REST account and trading routes
- PostgreSQL-oriented repository abstraction for balances, open orders, fills, identity lookups, and admin audit records
- In-memory repository backend plus a live PostgreSQL repository backend behind the same boundary
- Background PostgreSQL writer thread with batched flushes, retry/backpressure handling, and health telemetry behind the storage boundary
- EC2-hosted internal test deployment is live at `http://16.59.150.9:8080` with WS at `ws://16.59.150.9:8080/ws`
- OpenAPI generation and Swagger UI
- Tests and latency checks for core paths

Target constraints:

- This is for an internal competition, not a public exchange product
- Users do not self-register
- Operators provision competition accounts
- Each participant should receive a unique assigned API key that serves as both login identity and API access
- Matching remains in memory inside the exchange process
- Durable account, order, fill, and audit data should live in local PostgreSQL on the EC2 machine
- PostgreSQL writes must happen off the main exchange thread and be flushed in batches
- HA, multi-region failover, and public-internet hardening are out of scope for now

What is still true:

- Persistence and recovery are only partially implemented
- Settlement is still in-memory only
- WS trading is implemented end-to-end, but restart reconciliation is still incomplete
- Startup recovery now rebuilds in-memory orderbooks from persisted open orders, but full reconciliation is still missing

## Progress Snapshot

### Done

- Rust service skeleton is in place
- Health endpoint exists
- REST endpoints exist for:
  - `POST /api/v1/admin/users`
  - `GET /api/v1/user`
  - `GET /api/v1/balance`
  - `GET /api/v1/portfolio`
  - `GET /api/v1/open-orders`
  - `GET /api/v1/fills`
  - `POST /api/v1/orders`
  - `PATCH /api/v1/orders/{order_id}`
  - `DELETE /api/v1/orders/{order_id}`
- Swagger docs are exposed at `/docs`
- WebSocket connection, heartbeat, and public L3 snapshot + delta broadcast flow exist
- WebSocket `authenticate` handshake exists for assigned user API keys
- In-memory orderbook supports:
  - price priority
  - FIFO per price level
  - cancel by order id
  - amend remaining quantity
- Matching engine tests pass
- REST tests pass
- Admin provisioning endpoint is tested end-to-end
- Local submit / cancel / amend flows are tested end-to-end
- Direct API-key trading flow is tested end-to-end
- Cross-account order isolation is tested end-to-end
- Register route removal is tested end-to-end
- Login route removal is tested end-to-end
- Per-user `100 ops/sec` REST rate limiting is tested end-to-end
- Balance locking and in-memory fill settlement are tested
- Admin provisioning is protected by a configured bearer token and emits audit records
- Storage access is routed through a repository layer instead of raw app-state maps
- Repository backend trait exists with in-memory and PostgreSQL backends
- PostgreSQL initial schema exists for users, api keys, balances, orders, fills, positions, pending positions, pnl snapshots, and admin audit logs
- PostgreSQL repository writes are wired through a dedicated batched writer thread for identity, balances, orders, fills, and admin audit logs
- PostgreSQL writer health now exposes queue depth, flush latency, retry/failure counts, and degraded status through `/health`
- Startup recovery rebuilds in-memory orderbooks from persisted open orders
- Public health and WebSocket auth/snapshot probes succeeded over Elastic IP `16.59.150.9`
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
- Settlement:
  - in-memory balance locking / release / fill application exists
  - no persistence or reconciliation yet
- Docs:
  - OpenAPI exists for the current REST surface
  - internal competition docs still need to be written
- Storage:
  - in-memory backend is still the default local app mode
  - PostgreSQL backend can be selected behind the same storage boundary
  - PostgreSQL schema is defined and executed by the live repository implementation
  - dedicated off-thread persistence, batched write flushing, and retry/backpressure telemetry are implemented
  - startup recovery rebuilds in-memory orderbooks from persisted open orders
  - settlement journaling and full reconciliation are not implemented yet

### Not Started / Missing

- Persistent settlement journal
- Order recovery from snapshots or journal replay
- EC2 deployment topology and local PostgreSQL operations plan
- Local PostgreSQL backup / restore and restart-recovery playbooks
- Internal operator and competition-user documentation

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
- [ ] Keep only the audit logging that is operationally useful for provisioning and access changes
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

- In-memory settlement exists for local trading flows
- No persistence or restart reconciliation

### Required

- [x] Lock funds before accepting orders
- [x] Release funds on cancel
- [x] Apply debits / credits on fill
- [ ] Persist settlement journal through the background PostgreSQL writer
- [ ] Reconcile balances after restart
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
  - balances
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
- [x] Move durable writes for orders, fills, balances, and audit events onto the dedicated persistence worker
- [x] Batch durable writes with bounded size and latency thresholds
- [ ] Add market filtering for open orders
- [ ] Add pagination for fills and orders
- [ ] Add timestamps and cursors
- [ ] Add richer portfolio semantics if needed
- [ ] Ensure returned data matches WS event stream state
- [x] Rebuild in-memory orderbooks from persisted open orders after restart
- [ ] Add full reconciliation behavior from local PostgreSQL after restart

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
  - balances
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
  - balances / settlement events
  - audit records
- [x] Define flush triggers:
  - max batch size
  - max flush interval
  - queue pressure thresholds
- [x] Add backpressure, retry, and failure-handling policy for the persistence queue
- [x] Rebuild in-memory orderbooks from persisted open orders on startup
- [ ] Add snapshots and full recovery procedures
- [ ] Define local backup retention and restore drills
- [ ] Add restart reconciliation playbooks

## 12. Documentation

We need internal docs, not public-product polish.

### Required

- [ ] System architecture doc
- [ ] Internal auth and provisioning doc
- [ ] REST API doc
- [ ] WS trading protocol doc
- [ ] L3 market data doc
- [ ] Error codes and rejection reasons
- [ ] Sequence / replay / recovery semantics
- [ ] EC2 + local PostgreSQL deployment and operations doc
- [ ] Backup / restore doc
- [ ] Internal competition trader quickstart
- [ ] Example clients

## Suggested Implementation Order

1. Simplify competition-user auth to the assigned API-key-only model
2. Tighten the WS trading protocol and L3 schema
3. Move PostgreSQL persistence off the main exchange thread and batch writes
4. Extend startup recovery into full reconciliation on top of persisted data
5. Finish settlement durability and correctness
6. Extend restart reconciliation and settlement journaling
7. Write internal docs and runbooks alongside the implementation

## Immediate Next Tasks

- [x] Collapse competition-user auth to assigned API-key-only
- [x] Move WS auth onto the assigned API-key identity model
- [x] Build the dedicated batched PostgreSQL writer path off the main exchange thread
- [x] Rebuild in-memory orderbooks from persisted open orders on startup
- [x] Add WS submit / cancel / amend trading messages with ack/reject replies
- [x] Add full socket-level WS integration tests
- [x] Add `resync_required` handling for market-data gaps and lag
- [x] Add queue depth, flush latency, and DB failure metrics
- [ ] Add full reconciliation and settlement recovery using local PostgreSQL
- [ ] Persist a settlement journal through the background PostgreSQL writer
- [x] Make PostgreSQL the default deployed backend and validate it on the EC2 box
- [ ] Refactor engine-internal order representation
- [ ] Document the internal architecture and operator runbook
