# API Load Bench Go

Benchmark runner for the exchange API across three traffic shapes:

- `rest`: all order flow over `POST /api/v1/orders`
- `ws`: all order flow over `/ws` `submit_order`
- `mixed`: paired flow split across REST and websocket

The runner can also attach observer sockets that validate live market-data quality while load is running:

- `l2` observers track the batched aggregated book feed
- `l3` observers track the authenticated raw per-order feed
- at the end of each trial, observers compare their reconstructed state against a fresh snapshot

The tool provisions paired trader accounts through the admin API, then ramps the global target ops/sec upward until the run stops meeting the configured efficiency thresholds.

## What “max load” means

A trial is considered passing when all of these hold:

- reject rate is at or below `--max-reject-rate`
- p95 latency is at or below `--max-p95-ms`
- completed operations are at or above `--min-achieved-ratio`

The highest passing target per mode is reported as the practical ceiling.

## Run

```bash
cd tools/api-load-bench-go
go run . \
  --base-url https://exchange.jamesxu.dev \
  --admin-token admin \
  --mode all \
  --market PEOPLE-USD \
  --traders 50 \
  --duration-seconds 20 \
  --start-ops-per-second 100 \
  --step-ops-per-second 100 \
  --max-ops-per-second 2000 \
  --lower-price 95 \
  --upper-price 105 \
  --lower-quantity 1 \
  --upper-quantity 100 \
  --price-pattern random-walk \
  --min-spread-ticks 1 \
  --max-spread-ticks 3 \
  --max-depth-ticks 6 \
  --cross-interval-ticks 8
```

To make the feed checks explicit, include observer settings in the run:

```bash
cd tools/api-load-bench-go
go run . \
  --base-url https://exchange.jamesxu.dev \
  --admin-token admin \
  --mode all \
  --market PEOPLE-USD \
  --traders 50 \
  --observe-l2-clients 1 \
  --observe-l3-clients 1 \
  --require-feed-accuracy true
```

## Notes

- `--admin-token` defaults to `admin` so it matches the current operator login convention.
- Ops/sec is global, not per-user. It must be even because each scheduled event submits one buy and one sell.
- The scheduler uses a random-walk center price and configurable spread/depth so the book does not collapse into pure same-price self-crossing.
- `--observe-l2-clients` and `--observe-l3-clients` provision observer sockets that watch sequence continuity, resyncs, and final snapshot parity.
- `--require-feed-accuracy` causes any observer gap, resync, or final book mismatch to fail the trial.
- `--reset-before-suite` and `--reset-between-trials` call `POST /api/v1/admin/users/reset`. Use those only against disposable environments.
