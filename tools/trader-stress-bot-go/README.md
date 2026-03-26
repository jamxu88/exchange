# Go Trader Stress Bot

Purpose-built Go load generator for sustained REST order-rate tests against the exchange.

It provisions paired trader accounts through the admin API, then has each pair submit opposite-side limit orders at the same random price inside the configured band. That keeps the aggregate flow balanced while each user still submits at the target per-user rate.

## Run

```bash
cd tools/trader-stress-bot-go
go run . \
  --base-url https://exchange.jamesxu.dev \
  --admin-token admin \
  --market PEOPLE-USD \
  --traders 150 \
  --duration-seconds 120 \
  --ops-per-second 100 \
  --lower-price 90 \
  --upper-price 110 \
  --quantity 1 \
  --provision-concurrency 50 \
  --max-in-flight 50000
```

## Notes

- This tool is optimized for the sustained `POST /api/v1/orders` test shape that overwhelmed the Node launcher.
- Users are provisioned as normal `trader` accounts; there is no privileged maker account in this mode.
- Each pair shares a random price on every tick, with one user buying and the other selling.
- Submission is smoothed across the 1-second window so the full user set does not burst at the same millisecond.
- `--max-in-flight` is a safety valve for the launcher. If the exchange or network slows down enough to hit that cap, the generator backpressures instead of consuming unbounded memory.
- The tool reports target orders, actual attempts, accepts, rejects, latency, throughput, and aggregate ending positions.
