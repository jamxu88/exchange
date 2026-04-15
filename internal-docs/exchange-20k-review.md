# Exchange Review And 20k POST/s Plan

## Goal

This document reviews the current exchange implementation and outlines the changes needed to make the service capable of sustaining roughly `20,000 POST /api/v1/orders` requests per second.

Important distinction:

- `20k POST/s` is not the same thing as `20k accepted orders/s`
- the current system can reject many requests cheaply, but the real target should be:
  - sustain `20k order-submit requests/s`
  - keep p95 latency controlled
  - avoid runaway backpressure
  - accept a meaningful fraction of those orders under a realistic market shape

## Current State Summary

The current branch and the EC2 deployment now share the same core refactor:

- Axum REST and WebSocket ingress
- one in-memory order book per market, now owned by a dedicated per-market engine thread
- position-based risk checks that now use engine-local exposure tracking plus storage-backed account state
- an account-state dispatcher that applies open-order, position, and fill read-model mutations off the engine thread
- read-your-writes response fencing so submits, cancels, and amends still return only after account read models have caught up
- bounded runtime, account, and persistence sidecar queues with health-visible depth and blocked-enqueue telemetry
- account-barrier wait telemetry exposed through `/health`
- in-memory authoritative trading state backed by asynchronous Postgres persistence with non-blocking ingress
- one runtime dispatcher thread for market/user/system fanout
- one runtime persistence sidecar for non-authoritative order-ledger writes on the Postgres backend
- aggregated market-data snapshots and level updates over WebSocket instead of raw per-order L3 snapshots

This is materially better than the earlier mutexed-book design, but it is still not shaped for `20k POST /orders/s` on one hot market.

Observed live behavior from the `2026-03-23` Go stress run against the pre-refactor deployment:

- target shape was `150 users * 100 POST /orders/s = 15,000 req/s`
- observed issue rate was roughly `2.9k-4.2k req/s`
- observed completion rate was roughly `2.0k-3.7k req/s`
- the service later recovered to `/health.status = "ok"`
- persistence telemetry showed clear write-path backpressure:
  - `total_blocked_enqueues = 8332`
  - `total_enqueue_block_time_ms = 267347`

That run proved the exchange bottleneck was inside the server, not only in the launcher.

Observed live behavior from the `2026-03-25` EC2 deployment after the engine/dispatcher/queue refactor:

- Live baseline over the public URL: `60 traders * 50 POST /orders/s` for `10s`
  - `30,000 / 30,000` accepted
  - `2.85k req/s`
  - p95 `111.14ms`, p99 `148.96ms`
  - persistence backlog peaked at about `40k` ops with `high_water_mark = 64,478`
  - runtime/account/persistence dispatch queues stayed empty by the time `/health` was sampled and barrier waits remained effectively zero
- Live hot pass over the public URL: `150 traders * 100 POST /orders/s` for `5s`
  - `75,000` attempted
  - `60,302` accepted
  - `14,698` rejected, almost entirely client timeouts rather than exchange-side validation rejects
  - `3.11k req/s`
  - p95 `5845.40ms`, p99 `7450.59ms`
  - persistence backlog peaked at about `81.9k` ops with `high_water_mark = 132,430`
  - runtime/account/persistence dispatch queues still stayed well below capacity, and submit barrier max stayed at only `6ms`
  - one public `/health` probe briefly failed to connect during unwind, but the service stayed up and both local and public health recovered to `status = "ok"` after the Postgres backlog drained

Those deployed results reinforce the same split as the local benchmarks:

- the refactor removed the old blocking and lock-contention pathologies
- the next real limit is still the hot-market engine cost plus downstream durability throughput
- queue saturation and account-barrier waits are not yet the first wall on the deployed stack

Observed local behavior on `2026-03-23` after the queue/barrier instrumentation and the Postgres fill-persistence ordering fix:

- In-memory baseline: `60 traders * 50 POST /orders/s` for `10s`
  - `30,000 / 30,000` accepted
  - `2.86k req/s`
  - p95 `0.62ms`, p99 `1.28ms`
  - runtime/account queues stayed effectively empty
- In-memory hot pass: `150 traders * 100 POST /orders/s` for `5s`
  - `74,859 / 75,000` accepted
  - `6.18k req/s`
  - p95 `2587.70ms`, p99 `3546.95ms`
  - runtime/account queues still stayed far below their configured thresholds
- Postgres baseline: `60 traders * 50 POST /orders/s` for `10s`
  - `30,000 / 30,000` accepted
  - `2.86k req/s`
  - p95 `0.58ms`, p99 `1.89ms`
  - request path stayed fast, but the Postgres writer backlog peaked above `71k` ops and drained after the run
- Postgres hot pass: `150 traders * 100 POST /orders/s` for `5s`
  - `70,799 / 75,000` accepted
  - `4.46k req/s`
  - p95 hit the client timeout at about `5s`
  - Postgres writer backlog peaked above `222k` ops and was still above `167k` ops `10s` after the run ended

Those local results matter because they separate two ceilings:

- the in-memory hot-market ceiling, where matching/request latency degrades before the new sidecar queues saturate
- the Postgres durability ceiling, where the request path can still run but the write backlog grows much faster than it drains

## Review Findings

### High Severity

1. WebSocket trading bypasses the per-user REST rate limiter.

- REST account/trading routes are rate-limited in `build_app`, but WebSocket `submit_order`, `cancel_order`, and `amend_order` call directly into `TradingService`.
- Files:
  - `exchange/src/lib.rs`
  - `exchange/src/ws.rs`
- Risk:
  - one user can bypass the documented per-user request cap by switching transports
  - fairness and protection guarantees are inconsistent across APIs

2. `/api/v1/balance` returns positions instead of balances.

- File:
  - `exchange/src/rest.rs`
- Risk:
  - incorrect API behavior
  - likely hidden because tests only check auth behavior, not payload correctness

### Medium Severity

3. Dispatcher backpressure is now visible and bounded, but it can still stall producers under overload.

- Files:
  - `exchange/src/storage.rs`
  - `exchange/src/state.rs`
- The branch now uses bounded runtime/account/persistence queues plus health telemetry, so queue growth is explicit instead of silent.
- That is the right tradeoff for an accuracy-first exchange, but full queues now push back on producers rather than letting memory grow without bound.
- Risk:
  - producer latency can spike sharply when queues saturate
  - durability lag can widen under sustained overload
  - response latency can move from engine time to account-dispatch barrier wait time

4. Single-market matching is still single-owner by design.

- Files:
  - `exchange/src/trading.rs`
  - `exchange/src/state.rs`
- The old mutex bottleneck is gone locally, but one hot market is still capped by one engine thread.
- Risk:
  - throughput for one symbol still scales mostly with one core
  - `20k POST /orders/s` still requires an aggressively lean engine path

5. Engine-local exposure state now duplicates part of the account model and needs stronger invariants.

- Files:
  - `exchange/src/trading.rs`
  - `exchange/src/storage.rs`
- The submit path no longer scans open orders, but the market engine now owns a second view of pending exposure and market position for each trader on that market.
- Risk:
  - any divergence between engine exposure state and storage-backed read models can produce incorrect accepts or rejects
  - the fast path now depends on runtime-state recovery being exact

6. The submit path still has too much total downstream work per accepted order.

- Files:
  - `exchange/src/trading.rs`
  - `exchange/src/settlement.rs`
  - `exchange/src/storage.rs`
- Each accepted order can still trigger:
  - open-order read-model updates
  - per-fill position upserts or deletes for both counterparties
  - fill-history appends for both counterparties
  - order-ledger close/update persistence
- This no longer blocks matching directly, but it still drives queue depth, cache churn, and response-fence latency.

7. Postgres durability now behaves like a sustained-lag bottleneck rather than an immediate request-path stall.

- Files:
  - `exchange/src/storage.rs`
  - `exchange/src/state.rs`
  - `exchange/src/trading.rs`
- The Postgres writer no longer blocks the engine thread directly, but local benchmarks show it can fall tens or hundreds of thousands of ops behind while the request path still appears healthy.
- Risk:
  - durable history can lag far behind in-memory truth under sustained load
  - recovery cost rises with backlog size
  - "service is up" can mask "durability is not keeping up"

8. Settlement and other bulk workflows still rewrite full position snapshots.

- File:
  - `exchange/src/settlement.rs`
- `apply_fill()` is now optimized, but `settle_market()` still rewrites full position sets.
- Risk:
  - administrative actions are still heavier than necessary
  - the slow path remains more expensive than it should be

9. The new bounded queues still need final overload policy and tuning.

- Files:
  - `exchange/src/state.rs`
  - `exchange/src/storage.rs`
- The runtime dispatcher, account dispatcher, and persistence sidecar are now bounded and exposed through `/health`.
- Risk:
  - queue capacity choices are still defaults, not tuned from benchmark data
  - we still need explicit product policy for what should degrade first under overload

### Low Severity

10. Global permissive CORS applies to admin routes too.

- File:
  - `exchange/src/lib.rs`
- This is not the main throughput problem, but it is looser than necessary for admin surfaces.

11. The codebase lacks production-oriented performance instrumentation.

- There is some persistence telemetry and a couple of ignored latency smoke tests.
- What is missing:
  - request latency breakdown by stage
  - per-market engine queue depth and service time
  - accepted/rejected counters by reason
  - per-market throughput metrics

## Primary Throughput Bottlenecks

### 1. Single-Market Engine Cost

For a hot market, matching is now explicitly single-owner:

- REST/WS ingress validates and hands off to the market engine
- one engine thread owns one book
- the engine matches, settles, updates open orders, and emits side effects in sequence

That is the correct shape for price-time-priority matching, and it is better than contended locking. It is still a hard ceiling for one symbol if the engine path is too heavy.

### 2. Request-Path Account Fence And Read-Model Throughput

The most expensive remaining work is no longer engine-side account mutation. The engine now enqueues account-state updates and keeps matching.

What remains is:

- account-dispatch throughput for open orders, positions, and fills
- response fence wait time so `submit/cancel/amend` preserve read-your-writes behavior
- request-thread storage lookups such as `find_order_market()`
- control-plane validation reads like market status and user-role checks

### 3. Read-Model Synchronization And Runtime-State Duplication

The submit path is now fast because the market engine owns exposure state locally.

That also means there are now two views of trader state that must stay aligned:

- engine-local market exposure
- storage-backed open-order read models
- storage-backed position read models

That is the right tradeoff for throughput, but it raises the bar for invariants, recovery correctness, and targeted regression tests.

### 4. Write Amplification And Sidecar Backpressure

Market/user/system fanout is now queued off the engine thread, and Postgres persistence ingress is now non-blocking.

That is the right direction, but the current sidecars still need:

- explicit overload behavior
- lag visibility for subscribers and persistence workers
- barrier-wait visibility for account read-model catch-up

## Changes Already Made In This Branch

These changes are in the codebase now, and the exchange-side changes were deployed to EC2 on `2026-03-25`:

1. Removed a duplicate order-ledger write during open-order sync.

- File:
  - `exchange/src/trading.rs`

2. Raised Postgres writer defaults:

- `POSTGRES_WRITE_BATCH_SIZE = 512`
- `POSTGRES_WRITE_FLUSH_INTERVAL_MS = 10`
- `POSTGRES_WRITE_QUEUE_CAPACITY = 65536`

- File:
  - `exchange/src/config.rs`

3. Replaced mutex-protected per-market order books with dedicated per-market engine threads.

- Files:
  - `exchange/src/trading.rs`
  - `exchange/src/state.rs`
- Result:
  - lock contention is removed from the hot market path
  - sequencing is now explicit and deterministic

4. Moved market/user/system fanout onto a dedicated runtime dispatcher thread.

- Files:
  - `exchange/src/state.rs`
  - `exchange/src/trading.rs`
  - `exchange/src/admin.rs`
- Result:
  - websocket broadcast work is no longer done inline on the market engine thread

5. Moved order-ledger persistence off the market engine thread for the Postgres backend.

- Files:
  - `exchange/src/state.rs`
  - `exchange/src/trading.rs`
- Result:
  - order-history writes no longer directly block the engine

6. Moved open-order, position, and fill read-model mutation off the market engine thread behind a dedicated account dispatcher.

- Files:
  - `exchange/src/state.rs`
  - `exchange/src/trading.rs`
  - `exchange/src/settlement.rs`
- Result:
  - the engine no longer mutates account read models inline
  - submits, cancels, and amends still preserve read-your-writes behavior by waiting on an account-dispatch barrier before returning
  - account-state latency is now paid on the request side rather than inside the single-market engine

7. Made Postgres write ingress non-blocking for all storage operations.

- File:
  - `exchange/src/storage.rs`
- Result:
  - matching and settlement no longer block on the writer queue
  - `POSTGRES_WRITE_QUEUE_CAPACITY` now behaves as a soft backpressure threshold instead of a hard blocking limit

8. Replaced per-fill full position snapshot rewrites with single-position get/upsert/delete operations.

- Files:
  - `exchange/src/settlement.rs`
  - `exchange/src/storage.rs`
- Result:
  - fills now touch exactly one market position per trader
  - flat zero-PnL positions are deleted instead of rebuilding the entire trader position set

9. Replaced submit-side open-order scans with engine-local exposure tracking.

- File:
  - `exchange/src/trading.rs`
- Result:
  - per-submit limit checks are now constant-time with respect to the trader's resting order count
  - exposure updates happen in the same serialized order as matching, fills, amend, and cancel
  - targeted regression coverage now exercises fill and cancel transitions against the limit logic

10. Added bounded queue telemetry and account-barrier wait instrumentation, exposed through `/health`.

- Files:
  - `exchange/src/state.rs`
  - `exchange/src/rest.rs`
  - `exchange/src/trading.rs`
- Result:
  - runtime, account, and persistence sidecar queues now report depth, high-water mark, blocked enqueue counts, and blocked enqueue time
  - submit/cancel/amend barrier waits are now visible per operation
  - overload can be measured directly before the next stress run

11. Fixed Postgres fill persistence so durable fill writes are ordered behind order-ledger writes.

- Files:
  - `exchange/src/storage.rs`
  - `exchange/src/state.rs`
  - `exchange/src/trading.rs`
- Result:
  - fill history is now persisted through the same ordered persistence queue as the order ledger
  - the Postgres writer no longer hits `fills -> orders` foreign-key races under load
  - Postgres benchmarks now expose throughput limits instead of a broken write path

12. Hardened the Go stress launcher transport for live HTTPS runs.

- File:
  - `tools/trader-stress-bot-go/main.go`
- Result:
  - disabled forced HTTP/2 for the launcher
  - removed the oversized per-host connection cap that was triggering a Go `net/http` transport panic
  - capped the launcher's idle connection pool to a sane range so live-url tests finish with a usable summary

13. Added explicit competition finalization and frozen leaderboard export.

- Files:
  - `exchange/src/admin.rs`
  - `exchange/src/rest.rs`
  - `exchange/src/storage.rs`
- Result:
  - operators can now stop trading, settle explicit markets, freeze filtered final standings, and export a CSV snapshot
  - this lives on the admin/reporting path and does not change the trading hot path

14. Replaced raw per-order market-data snapshots with aggregated price-level snapshots and level deltas.

- Files:
  - `exchange/src/orderbook.rs`
  - `exchange/src/marketdata.rs`
  - `exchange/src/trading.rs`
  - `exchange/src/ws.rs`
  - `client/src/components/trade/trade-ws-client.ts`
  - `client/src/components/trade/trade-store.ts`
- Result:
  - WebSocket snapshot size now scales with visible price levels instead of total resting order count
  - the browser no longer needs to hydrate and maintain a full L3 order map just to render the top of book
  - initial subscribe fanout bandwidth is dramatically lower for large books, which directly matters for `150+` concurrent client connects

These changes are worth deploying, but they are still not enough by themselves to make the exchange handle `20k POST/s`.

## Recommended Roadmap

### Phase 0: Correctness And Safety Fixes

Do these first because they are small and high value.

1. Fix `/api/v1/balance` to return balances.
2. Apply rate limiting consistently to WebSocket trading.
3. Restrict CORS for admin routes.

### Phase 1: Cheap Throughput Wins

These are the fastest improvements with good risk/reward.

1. Deploy the already-made persistence tuning, per-market engines, and sidecar dispatchers.
2. Tune queue capacities and define explicit overload policy for dispatcher, account, and persistence queues.
3. Add invariant tests and recovery checks around engine-local exposure tracking.
4. Audit the request path for remaining synchronous storage/cache work and peel off anything that is not authoritative for trading decisions.
5. Separate "authoritative in-memory trading state" from "eventual durable snapshots".

Expected result:

- materially lower latency spikes
- fewer persistence-induced stalls
- better behavior under overload

### Phase 2: Reshape Persistence Around Append-Only Journals

The current persistence model is too state-rewrite heavy.

Move toward:

- append-only order events
- append-only fill events
- async derived-state materialization for:
  - open orders
  - positions
  - balances
  - leaderboard

Recommended structure:

1. Trading path writes compact append-only events.
2. Background consumers build read models.
3. Recovery replays journal segments or checkpoints.

Why this matters:

- append-only writes are cheaper than repeated upserts/replaces
- batching becomes much more efficient
- read-model rebuilds are asynchronous and can be parallelized

### Phase 3: Remove The Single Hot-Market Lock As The Ceiling

This structural step is now complete locally, but not deployed.

Options:

1. Per-market dedicated worker task

- one thread/task owns one market book
- requests are sent over a channel
- no shared mutex in the hot path

This is the lowest-risk architectural improvement.

2. Shard one hot market across multiple books

- only if the product model allows it
- harder because a central price-time-priority book does not shard cleanly

3. Use a specialized single-threaded engine process per hot market

- REST/WS ingress hands off to a colocated market engine
- persistence and fanout happen off the critical path

For this exchange, option 1 was the right next step and is already in the branch.

### Phase 4: Decouple Fanout From The Submit Path

The first part of this is now complete locally. The remaining work is production hardening.

Recommended changes:

1. Keep market/user/system events queued instead of broadcasting inline.
2. Track dropped or lagged consumer counts explicitly.
3. Add queue-depth metrics and overload behavior for the dispatcher.
4. Batch deltas when clients are behind.
5. Consider separating:
   - trading ingress
   - market-data fanout
   - user-event fanout

### Phase 5: Transport And Ingress Tuning

REST JSON is convenient, but `20k POST/s` is an aggressive target.

Recommended:

1. Keep REST for control-plane simplicity.
2. Add a lower-overhead trading ingress for load and production:
   - WebSocket trading with explicit flow control, or
   - a compact binary TCP/QUIC protocol for order entry
3. Add connection reuse and keepalive tuning explicitly in deployment.
4. Test with colocated load generators, not laptops over the public internet.

## What I Would Build Next

If the real target is a credible path to `20k POST /orders/s`, I would do this in order:

1. Deploy the existing structural changes in this branch.
2. Fix the correctness/security findings above.
3. Add stronger invariant tests and recovery checks around engine-local exposure state.
4. Move the remaining authoritative account state out of generic storage calls and into dedicated in-memory runtime state.
5. Replace remaining snapshot-style persistence with compact deltas or append-only journal writes.
6. Add stage-level metrics so we can prove where time is going.
7. Rerun synthetic benchmarks:
   - in-memory backend only
   - Postgres with current non-blocking writer ingress
   - Postgres with current sidecar architecture
   - Postgres with append-only journal path
   - one hot market
   - many markets

## Benchmark Plan

To know whether the stack can actually handle `20k POST/s`, run these in order:

1. Single host, in-memory backend, one hot market

- goal: isolate matching/risk/fanout cost

2. Single host, Postgres backend, one hot market

- goal: quantify persistence tax

3. Same build, same host, persistence queue intentionally bypassed or no-op persisted

- goal: prove whether writes are the main limit

4. Same build, colocated load generator in the same region/AZ

- goal: remove public internet and local laptop effects

5. Full success criteria

- sustained `20k POST /orders/s`
- bounded p95 and p99 latency
- no queue saturation spiral
- no event-loop starvation
- stable memory growth

## Metrics To Add Before The Next Load Test

1. REST submit latency by stage:

- auth
- risk
- engine queue wait
- matching
- settlement
- account-dispatch barrier wait
- sidecar enqueue
- total

2. Per-market counters:

- submit rate
- accept rate
- reject rate by reason
- fills per second

3. Persistence metrics:

- queue depth over time
- dispatcher, account, and persistence queue depth histograms
- flush batch size histogram
- flush latency histogram

4. WebSocket metrics:

- lagged receivers
- dropped events
- snapshot rebuild count

## Bottom Line

The current exchange is not close to `20k POST /orders/s` on one hot market.

The branch is in a better place than the live deployment because:

1. market ownership is now explicit and lock-free
2. websocket fanout is no longer inline on the engine thread
3. persistence ingress is no longer inline-blocking on the engine thread
4. per-fill position updates no longer rewrite the trader's entire position set
5. account read-model updates no longer run inline on the engine thread
6. queue saturation and barrier wait are now visible through `/health`

But the biggest remaining blockers are:

1. one hot market is still limited by one engine core
2. the runtime exposure/read-model split now needs stronger invariants and recovery checks
3. dispatcher, account, and persistence queues still need tuned capacities and a final overload policy
4. request-path barrier latency can still become the next ceiling under heavy accepted-order load

Deploying the work in this branch was worth doing and materially improved the live stack, but the real path to `20k POST/s` still requires a deeper account-state and persistence redesign.
