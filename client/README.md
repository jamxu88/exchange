# Exchange Client Template

Next.js template for a low-latency exchange client UI, designed for ECS deployment.

Canonical internal docs now live in `docs/` as a Mintlify site.

## Current deployed exchange endpoint

- HTTP base: `https://exchange.jamesxu.dev`
- Health: `https://exchange.jamesxu.dev/health`
- Swagger docs: `https://exchange.jamesxu.dev/docs`
- WebSocket: `wss://exchange.jamesxu.dev/ws`
- Public port `80` is not currently redirecting, so use the HTTPS URL directly.

## Internal docs

- Mintlify docs root: `docs/`
- See the client pages there for integration, deployment, Figma mapping, and keybind reference.
- Preview locally with `cd docs && npx mintlify dev`

## Current integration target

- The live exchange backend on `https://exchange.jamesxu.dev` is the current integration target for the client.
- That backend now runs from the GitHub-synced EC2 checkout, so client integration testing against the live host should track the latest deployed `main`.
- Keep client endpoint configuration externalized with `EXCHANGE_HTTP_URL`, `NEXT_PUBLIC_EXCHANGE_HTTP_URL`, and `NEXT_PUBLIC_EXCHANGE_WS_URL`; do not hardcode the domain in app logic.

## Included template features

- App Router + TypeScript + Tailwind
- ECS-ready production container (`output: "standalone"` + `Dockerfile`)
- Individual authentication scaffold (`/login`, session cookie, route protection)
- Admin panel route (`/admin`) with role-gated middleware
- Trader console route (`/trade`)
- Keybind support:
  - `Ctrl/Cmd + K` => Trader Console
  - `Ctrl/Cmd + G` => Admin Panel
- Health endpoint (`/api/health`)

## Run locally

```bash
npm install
npm run dev
```

The local client defaults still target `localhost:8080`. To point a local client at the deployed exchange instead, set:

```bash
EXCHANGE_HTTP_URL=https://exchange.jamesxu.dev
NEXT_PUBLIC_EXCHANGE_HTTP_URL=https://exchange.jamesxu.dev
NEXT_PUBLIC_EXCHANGE_WS_URL=wss://exchange.jamesxu.dev/ws
```

## Environment

Copy `.env.example` to `.env.local` and update values.

For the current internal test deployment, use:

```bash
EXCHANGE_HTTP_URL=https://exchange.jamesxu.dev
NEXT_PUBLIC_EXCHANGE_HTTP_URL=https://exchange.jamesxu.dev
NEXT_PUBLIC_EXCHANGE_WS_URL=wss://exchange.jamesxu.dev/ws
NEXT_PUBLIC_EXCHANGE_MARKETS=BTC-USD,ETH-USD,SOL-USD
```

`EXCHANGE_HTTP_URL` is used by server-rendered routes and server actions such as login and the admin page. If it is unset, those paths fall back to `NEXT_PUBLIC_EXCHANGE_HTTP_URL`, then to `http://localhost:8080`.

When validating the client against the live EC2 exchange, re-check the exchange health endpoint first:

```bash
curl https://exchange.jamesxu.dev/health
```

## Production deployment notes (ECS)

- Build image using `Dockerfile`
- Set `NODE_ENV=production`
- Add load balancer health check to `/api/health`
- Keep auth secrets in AWS Secrets Manager / SSM Parameter Store

## What to implement next

- Add trader-facing balance and buying-power presentation where it improves decision making
- Decide whether the browser client should move order submit/cancel/amend from REST onto the existing WS trading protocol
- Add operational actions in admin panel (pause market, risk thresholds)
- Add end-to-end auth and authorization tests
