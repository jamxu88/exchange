# Exchange Client Template

Next.js template for a low-latency exchange client UI, designed for ECS deployment.

Canonical public API docs now live in `docs/` as a Mintlify site. Internal-only client and deployment notes now live in `internal-docs/`.

## Current deployed exchange endpoint

- HTTP base: `https://exchange.jamesxu.dev`
- Health: `https://exchange.jamesxu.dev/health`
- Swagger docs: `https://exchange.jamesxu.dev/docs`
- WebSocket: `wss://exchange.jamesxu.dev/ws`
- ALB hostname: `exchange-client-alb-1466111370.us-east-2.elb.amazonaws.com`

## Docs

- Public Mintlify API docs root: `docs/`
- Internal-only client and deployment notes root: `internal-docs/`
- Preview the public docs locally with `cd docs && npx mint validate` or `cd docs && npx mintlify dev`

## Current integration target

- The public hostname is `https://exchange.jamesxu.dev`.
- The client is deployed on ECS/Fargate behind an ALB, while backend exchange paths continue to route to the EC2 host.
- The ALB hostname remains available as `exchange-client-alb-1466111370.us-east-2.elb.amazonaws.com` for direct AWS-side debugging.
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

- Build and push with `infra/client-ecs/build_and_push.sh`
- Request ACM DNS validation with `infra/client-ecs/request_certificate.sh exchange.jamesxu.dev`
- Deploy/update the ECS stack with `infra/client-ecs/deploy_stack.sh`
- The ALB path split is:
  - ECS client: `/`, `/login`, `/trade`, `/admin`, `/api/auth/*`, `/api/health`
  - EC2 backend: `/api/v1/*`, `/ws`, `/health`, `/docs*`, `/api-doc/*`
- Keep auth secrets in AWS Secrets Manager / SSM Parameter Store

## What to implement next

- Add trader-facing position-limit and exposure presentation where it improves decision making
- Decide whether the browser client should move order submit/cancel/amend from REST onto the existing WS trading protocol
- Add operational actions in admin panel (pause market, risk thresholds)
- Add end-to-end auth and authorization tests
