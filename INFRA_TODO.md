# Infrastructure TODO

## Scope

This file tracks infrastructure work separately from local product/backend implementation.

Current intended deployment shape:

- Exchange backend: Rust service on EC2
- Exchange data layer: local PostgreSQL on the EC2 machine
- Client frontend: Next.js app on ECS
- Public ingress: ALB path routing in front of ECS + EC2
- Source of truth for deploys: GitHub `origin/main`, pulled onto the EC2 host
- Current internal test endpoint: `http://16.59.150.9:8080`
- Current internal test WebSocket endpoint: `ws://16.59.150.9:8080/ws`
- Public browser-safe endpoint: `https://exchange.jamesxu.dev`
- Public browser-safe WebSocket endpoint: `wss://exchange.jamesxu.dev/ws`

This infrastructure plan should stay aligned with the actual product target:

- internal competition only
- single-region / single-service deployment for now
- no RDS requirement
- no public-product hardening requirement

## 1. PostgreSQL

Purpose:

- Store exchange application data needed by the backend
- Provide durable transactional storage for position, order, fill, exchange-control, message, and audit state
- Run locally for the deployed EC2-based competition environment

Work items:

- [x] Define schema and table layout for:
  - users
  - api_keys
  - exchange_controls
  - markets
  - admin_messages
  - positions
  - pending_positions
  - orders
  - fills
  - settlement_journal
  - audit_logs
- [x] Define primary keys, foreign keys, and unique constraints
- [x] Define indexes required for exchange query paths
- [x] Define write patterns for:
  - order acceptance
  - order state transitions
  - fill recording
  - position updates
  - settlement events
  - exchange control / market config updates
  - admin message persistence
- [x] Add a dedicated persistence worker thread / task for PostgreSQL writes
- [x] Define batching strategy for the persistence worker:
  - max batch size
  - max flush interval
  - queue pressure thresholds
- [ ] Define idempotency strategy for repeated writes
- [ ] Define consistency requirements per query path:
  - transaction boundaries
  - lock strategy
  - isolation level
- [ ] Define local backup and restore plan
- [ ] Define migration strategy
- [ ] Define connection pooling strategy:
  - dedicated writer connection(s)
  - read/query pool
- [ ] Define read/write path for reconciliation and admin queries

Exchange implementation constraints:

- Design exchange storage access around explicit relational query paths, not ad hoc ORM sprawl
- Keep high-frequency matching state in memory, not in PostgreSQL
- Keep PostgreSQL writes off the main exchange thread
- Use batched writes so the local EC2 machine is not overwhelmed
- Keep account and order state queryable by user, market, and status with indexed reads
- Use transactions for account mutations that must remain consistent
- Keep append-only ledger semantics for orders, fills, and settlement events where appropriate

## 2. EC2 For Exchange

Purpose:

- Host the Rust exchange backend
- Host the local PostgreSQL instance used by the exchange

Work items:

- [ ] Define EC2 instance class and sizing targets for exchange + local PostgreSQL
- [ ] Define VPC, subnets, security groups, and internal exposure model
- [x] Define service bootstrapping:
  - systemd
  - binary deployment path
  - GitHub checkout path
  - environment file management
  - PostgreSQL service management
- [ ] Define secrets delivery approach
- [ ] Replace the temporary GitHub PAT on EC2 with a deploy key or machine-user SSH key
- [x] Define health checks and restart policy
- [ ] Define logging and metrics pipeline
- [x] Add TLS reverse proxy for `https://` and `wss://`
- [ ] Define deployment procedure:
  - [x] GitHub-backed EC2 checkout at `/home/ec2-user/exchange-v2`
  - [x] Fast-forward update flow via `git pull --ff-only`
  - [x] Release rebuild on-host via `cargo build --release`
  - [x] Rollout via `sudo systemctl restart exchange`
  - rollback
- [ ] Define SSH / SSM / operations access controls
- [ ] Define disk sizing and retention expectations for PostgreSQL data and backups
- [ ] Run a competition-like load test against the EC2 deployment

Exchange implementation constraints:

- Keep the exchange service runnable as a single deployable binary
- Keep config externalized through environment/config files
- Separate infrastructure wiring from core exchange logic
- Keep startup deterministic so EC2 instance replacement is straightforward
- Keep persistence work isolated from the main exchange loop

## 3. ECS For Client

Purpose:

- Host the Next.js client application
- Serve the competition UI separately from exchange compute

Work items:

- [x] Define ECS service topology
- [x] Define Docker build and runtime image
- [x] Define task sizing targets
- [x] Define ALB / domain / TLS setup
- [x] Define environment and secrets injection
- [x] Define deployment strategy
- [ ] Define static asset and caching strategy
- [x] Define client-to-exchange network path and allowed origins
- [ ] Define observability for frontend runtime and API errors
- [x] Complete ACM DNS validation for `exchange.jamesxu.dev`
- [x] Attach the HTTPS listener to the ALB after the cert is issued
- [x] Cut `exchange.jamesxu.dev` over to the ALB hostname at the external DNS provider
- [x] Remove direct public `8080` access after ALB cutover is verified

Client implementation constraints:

- Keep the client configurable by environment variables
- Keep API/WS endpoints externally configurable
- Avoid hardcoding localhost-specific assumptions into app code

## 4. Cross-Cutting Data Concerns

These need to be considered while building the exchange locally.

- [x] Introduce a repository/storage abstraction before wiring persistence
- [ ] Separate in-memory engine state from durable account/order records
- [ ] Define canonical source of truth for:
  - positions
  - pending positions
  - orders
  - fills
  - pnl
- [ ] Define write ordering between matching, settlement, and persistence
- [x] Define the persistence queue and worker model between the main exchange thread and PostgreSQL
- [ ] Define how WS/REST reads should source data:
  - in-memory state
  - PostgreSQL
  - hybrid strategy
- [ ] Define recovery/bootstrap path for EC2 process restart
- [ ] Define reconciliation jobs between engine state and PostgreSQL state

Current local status:

- The exchange now uses a repository layer for identity, positions, open orders, and fills.
- A PostgreSQL schema has been defined in `exchange/sql/migrations/001_initial.sql`.
- A live PostgreSQL backend exists behind the same repository boundary as the in-memory backend.
- The live matching orderbook still stays in-memory in the process, separate from durable account/query state.
- Dedicated persistence-thread batching is implemented and deployed.
- Startup recovery now rebuilds in-memory orderbooks from persisted open orders while positions remain available from the storage-backed cache.
- The exchange is running on EC2 with Elastic IP `16.59.150.9`.
- The EC2 host now runs from a GitHub clone at `/home/ec2-user/exchange-v2`.
- The exchange env file lives at `/home/ec2-user/exchange-v2/exchange.env`.
- The deployed update flow is `git pull --ff-only`, `cargo build --release`, and `sudo systemctl restart exchange`.
- GitHub access on the EC2 host is temporarily PAT-based and should be replaced with a deploy key.
- Public internal test endpoints are `http://16.59.150.9:8080`, `http://16.59.150.9:8080/health`, and `ws://16.59.150.9:8080/ws`.
- ECS stack `exchange-client` now exists in AWS.
- The ALB hostname is `exchange-client-alb-1466111370.us-east-2.elb.amazonaws.com`.
- The ALB already routes ECS client traffic for `/`, `/login`, `/trade`, `/admin`, `/api/auth/*`, and `/api/health`.
- The ALB already routes EC2 backend traffic for `/api/v1/*`, `/ws`, `/health`, `/docs*`, and `/api-doc/*`.
- ACM certificate `arn:aws:acm:us-east-2:490004617163:certificate/0ef4c31a-90b2-437c-bed8-8f23b74fc0f7` is issued for `exchange.jamesxu.dev` and attached to the ALB `443` listener.
- Public Mintlify API docs now exist under `docs/`, and internal-only deployment notes now live under `internal-docs/`.

## 5. Suggested Order

1. Finalize local PostgreSQL schema and indexed query paths.
2. Add the dedicated persistence queue / worker so database writes are off the main exchange thread.
3. Keep the exchange deployable as one EC2 service until the local product surface is complete.
4. Add restart recovery and reconciliation using local PostgreSQL.
5. Keep the client packaged simply unless internal competition requirements force a split.
