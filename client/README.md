# Exchange Client Template

Next.js template for a low-latency exchange client UI, designed for ECS deployment.

## Current deployed exchange endpoint

- HTTP base: `http://16.59.150.9:8080`
- Health: `http://16.59.150.9:8080/health`
- Swagger docs: `http://16.59.150.9:8080/docs`
- WebSocket: `ws://16.59.150.9:8080/ws`
- This is currently plain HTTP/WS for internal testing. TLS is not configured yet.

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
NEXT_PUBLIC_EXCHANGE_HTTP_URL=http://16.59.150.9:8080
NEXT_PUBLIC_EXCHANGE_WS_URL=ws://16.59.150.9:8080/ws
```

## Environment

Copy `.env.example` to `.env.local` and update values.

For the current internal test deployment, use:

```bash
NEXT_PUBLIC_EXCHANGE_HTTP_URL=http://16.59.150.9:8080
NEXT_PUBLIC_EXCHANGE_WS_URL=ws://16.59.150.9:8080/ws
NEXT_PUBLIC_EXCHANGE_MARKETS=BTC-USD,ETH-USD,SOL-USD
```

## Production deployment notes (ECS)

- Build image using `Dockerfile`
- Set `NODE_ENV=production`
- Add load balancer health check to `/api/health`
- Keep auth secrets in AWS Secrets Manager / SSM Parameter Store

## What to implement next

- Replace mock login with the internal competition API-key flow
- Finish wiring live account/trading WS events against the deployed exchange endpoint
- Add operational actions in admin panel (pause market, risk thresholds)
- Add end-to-end auth and authorization tests
