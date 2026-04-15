# Trader Stress Bot

Zero-dependency Node.js load generator for the exchange REST API.

It uses the admin API to:

- optionally reset trading state
- provision one privileged `admin` quote account with unlimited position power
- provision many standard trader accounts
- seed a large resting bid and ask from the quote account
- drive concurrent taker flow from the simulated traders
- report throughput, latency, fill, and reject metrics

## Run

```bash
node tools/trader-stress-bot/index.mjs \
  --base-url http://localhost:8080 \
  --admin-token Quant2024! \
  --market BTC-USD \
  --traders 100 \
  --iterations 200 \
  --lower-price 95 \
  --upper-price 105 \
  --quantity 1 \
  --reset-before-start
```

## Notes

- The bot uses only public trading routes plus admin provisioning/reset routes.
- The seeded quote account is created with `role: "admin"`, so its per-market position limit is unlimited.
- Standard simulated traders still use the normal fixed per-market limit.
- With `--lower-price` and `--upper-price`, the bot seeds a full admin quote ladder across the band and each trader picks a random side plus a random price inside that band.
- If you omit the band flags, the bot falls back to the simpler single bid/single ask mode around `--center-price` and `--spread`.
- By default the bot cancels the quote account's open orders at the end. Pass `--no-cleanup-quotes` to leave them resting.
- `--reset-before-start` and `--reset-after-run` call `POST /api/v1/admin/users/reset`, so do not use them against an environment you care about.
