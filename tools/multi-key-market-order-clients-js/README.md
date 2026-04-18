# Multi-Key Market Order Clients (JS)

Runs one JS client per API key and submits random `market` orders at the per-key rate limit speed (`500 ops / 10s` by default).

## Run

From repo root:

```bash
node tools/multi-key-market-order-clients-js/run.mjs \
  --base-url http://localhost:8080 \
  --market BTC-USD \
  --duration-seconds 10
```

## Notes

- Input keys file defaults to `allocated-api-keys-batch-1.txt`.
- API keys are read from TSV columns: `identifier<TAB>api_key`.
- Every key gets its own concurrent client loop.
- Orders sent are:
  - `order_type: "market"`
  - random `side` (`BUY` or `SELL`)
  - `price: 0`
  - configurable `quantity` (default `1`)
