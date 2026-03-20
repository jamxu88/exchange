# Client TODO

## Current Position

This repo already contains a Next.js client in `client/`, intended for ECS deployment.

What exists today:

- Next.js App Router app
- TypeScript
- Tailwind CSS
- ECS-ready Dockerfile / standalone build setup
- Login route scaffold
- Trade route scaffold
- Admin route scaffold
- API-key session flow with backend-validated cookie state
- Keybind provider scaffold
- Health endpoint
- Live exchange backend is available for integration testing at `http://16.59.150.9:8080` and `ws://16.59.150.9:8080/ws`
- The live exchange backend now runs from a GitHub-synced EC2 checkout, so client integration should assume `main` is the deployed source of truth
- Public Mintlify API docs now exist under `docs/`, and internal-only client/deployment notes now live under `internal-docs/`
- Public browser-safe exchange access is available at `https://exchange.jamesxu.dev` and `wss://exchange.jamesxu.dev/ws`
- ECS stack `exchange-client` is live behind the ALB and serving `https://exchange.jamesxu.dev`

What is still true:

- The app is intended for an internal competition, not a public trading product
- The client is no longer a starter template, but it is still not a production-ready trading system
- The trading UI and design system are implemented, but still need final polish and broader route coverage
- Real-time transport and trading UX are implemented client-side, but still depend on backend completeness and load validation
- Individual authentication is not implemented for production
- Admin workflows are now wired to the backend, but the operator UX still needs polish
- ECS deployment is implemented in AWS and serving the final public hostname

## Progress Snapshot

### Done

- Next.js app structure exists in `client/src/app`
- Route scaffolds exist for:
  - `/login`
  - `/trade`
  - `/admin`
- Backend-validated API-key login flow exists
- Role gating scaffold exists for trader/admin flows
- Keybind support exists for navigation:
  - `Ctrl/Cmd + K` -> `/trade`
  - `Ctrl/Cmd + G` -> `/admin`
- ECS deployment intent is already documented in [client/README.md](/Users/james/Desktop/Coding/exchange-v2/client/README.md)
- Live exchange endpoint/env configuration is documented in [client/README.md](/Users/james/Desktop/Coding/exchange-v2/client/README.md)
- Figma-based trading interface is implemented on `/trade`
- Core trading layout is in place:
  - market selector/header
  - connection status
  - user badge
  - positions panel
  - statistics card
  - orderbook
  - order ticket
  - messages panel
- Trade client architecture is defined:
  - runtime config
  - reducer/store
  - REST bootstrap client
  - WS client
  - controller hook
- Client-side order entry and state transitions are implemented for:
  - buy/sell mode
  - limit/market ticket mode
  - quantity stepping
  - pending vs active positions view
  - local message/event log
- Comprehensive frontend tests exist for the trading client:
  - reducer/store
  - REST client
  - WS client
  - controller hook
  - rendered trading view

### Partial

- Authentication:
  - API-key login form exists
  - login session is validated against the backend before cookie creation
  - admin vs trader role is resolved from backend auth
- Admin panel:
  - route exists
  - backend operator endpoints now exist for trading control, markets, config load, messages, settlement, and leaderboard
  - the page now reads live backend state and posts server actions to those endpoints
- Trader UI:
  - Figma shell exists
  - stateful workflows exist
  - still uses REST for ticket submit / amend / cancel even though the backend also supports WS trading messages
- Real-time integration:
  - REST bootstrap and WS connection logic exist
  - public health, WS auth, and L3 snapshot probes now succeed against `16.59.150.9`
  - the deployed exchange host is now updated from GitHub instead of manual file sync
  - client now consumes live `fill`, `order_state`, and `admin_message` events and refreshes account state from the backend
- Performance:
  - framework baseline is light
  - no production profiling, budgets, or load validation yet

### Not Started / Missing

- Full browser-session lifecycle hardening
- Low-latency client architecture work
- Memory budget enforcement
- End-to-end UX polish
- Broader validation of live WS/order flows under realistic competition traffic
- Performance/load testing under sustained market updates

## Product Requirements

- [x] Next.js app
- [x] Hosted on ECS
- [x] Clean UX/UI
- [ ] Low latency
- [ ] Low memory footprint
- [ ] Individual authentication
- [ ] Admin panel to run the event
- [x] Keybinds
- [x] Implement the referenced Figma design
- [x] Internal-competition API-key login flow
- [x] Admin messaging workflows for broadcast and user-specific communication

## Figma Source of Truth

Referenced design:

- `https://www.figma.com/design/1CXRX79fMCN1kILTRVUXzp/Untitled?node-id=40-8&m=dev`
- File key: `1CXRX79fMCN1kILTRVUXzp`
- Node id: `40:8`
- Node name: `MacBook Pro 14" - 4`

Observed structure from the referenced node:

- top navigation / market switcher
- connection status and user avatar
- left positions panel
- center orderbook / depth panel
- right order ticket
- bottom-left PnL summary card
- bottom-right messages panel

This should be treated as the source layout for the trading interface, not the current starter pages.

## Main Workstreams

## 1. Frontend Architecture

- [x] Keep the app in Next.js
- [x] Confirm App Router architecture
- [x] Define client data flow for low-latency trading UI
- [x] Decide where to keep live state:
  - WS event store
  - React state
  - external store if needed
- [x] Add runtime config for environment-specific endpoints
- [ ] Add logging and monitoring for client failures

## 2. ECS Deployment

- [x] Finalize ECS deployment strategy
- [x] Use container image from `client/Dockerfile`
- [x] Add environment management for production
- [x] Add health checks to `/api/health`
- [ ] Add CDN / caching strategy for static assets
- [ ] Add rollout / rollback plan
- [ ] Add monitoring for memory and response times
- [x] Complete ACM validation and ALB HTTPS listener attachment
- [x] Cut `exchange.jamesxu.dev` over to the ALB and verify browser flows on the final host

## 3. UX/UI

- [x] Replace the current starter UI with the Figma-based interface
- [x] Build a clean visual system from the design
- [x] Match layout, spacing, hierarchy, and states
- [x] Make the app feel intentional, not template-like
- [x] Ensure strong desktop trading ergonomics
- [x] Ensure responsive behavior on smaller screens
- [x] Add loading, empty, disconnected, and error states

## 4. Performance

Low latency and low memory are explicit requirements.

- [ ] Minimize rerenders in trading surfaces
- [ ] Avoid large component trees rerendering on each book update
- [ ] Use efficient event diffing for L3 updates
- [ ] Virtualize long lists where needed
- [ ] Define performance budgets:
  - initial load
  - memory
  - update latency
- [ ] Profile memory usage in production builds
- [ ] Profile WS update handling under load
- [ ] Avoid unnecessary client-side libraries

## 5. Individual Authentication

### Current

- Backend-validated API-key login with HTTP-only session cookie

### Required

- [x] Replace mock login with API-key login for the internal competition
- [ ] Support individual trader accounts keyed by assigned API keys
- [ ] Align auth model with backend auth/API key strategy
- [x] Validate API key on login and establish client session from it
- [x] Persist authenticated trader identity for REST and WS requests
- [x] Add protected route handling
- [ ] Add session expiry / refresh behavior
- [x] Add logout flow
- [x] Add admin-vs-trader authorization rules

## 6. Admin Panel

The client needs an admin panel to run the event.

### Current

- `/admin` route scaffold exists
- backend already exposes:
  - start / stop trading
  - market create / patch / delete
  - market enable / disable / settle
  - config load
  - admin messages
  - leaderboard

### Required

- [ ] Define event-ops workflows
- [x] Add admin dashboard layout
- [ ] Add operational controls, for example:
  - add markets
  - market pause/resume
  - start trading
  - enable/disable markets visible to users
  - settle markets
  - event messaging / announcements
  - send broadcast messages to all users
  - send unique messages to individual users
  - load config payloads into the backend
  - render leaderboard
  - monitoring views
- [ ] Add audit visibility for admin actions
- [x] Restrict access to admin users only

## 7. Keybinds

### Current

- navigation keybinds exist

### Required

- [ ] Preserve existing navigation keybinds
- [ ] Add in-trade keybind map
- [ ] Define safe trading keybinds carefully
- [ ] Add discoverability / shortcut overlay
- [ ] Avoid accidental destructive actions
- [ ] Ensure accessibility and focus handling

## 8. Trading UI Implementation

Using the referenced Figma node as the target:

- [x] Market selector/header
- [x] Connection state indicator
- [x] User/account badge
- [x] Positions panel
- [x] PnL summary card
- [x] Central orderbook / L3 panel
- [x] Order ticket with buy/sell mode
- [x] Quantity and price steppers
- [x] Cost summary
- [x] Messages / event log panel
- [ ] Bottom action / mode controls if required by design

## 9. Real-Time Integration

- [x] Connect to backend WS streams
- [x] Handle auth on WS connection
- [x] Subscribe to market data
- [x] Subscribe to account/trading events
- [x] Consume backend `admin_message` events in the message panel
- [x] Render L3 updates efficiently
- [x] Handle reconnect and resync
- [x] Surface connection degradation clearly

## 10. Trading Workflow

- [x] Submit order from the order ticket
- [x] Show order ACK / reject state
- [x] Show fills and order lifecycle updates
- [x] Show current positions
- [ ] Show net position limit / exposure cues where needed
- [x] Keep UI state consistent with server events

## 11. Documentation

- [x] Client architecture doc
- [x] Auth flow doc for API-key login
- [x] ECS deployment doc
- [x] Figma-to-implementation mapping doc
- [x] Keybind reference
- [ ] Admin workflow doc for market controls and user messaging
- [ ] Performance budget doc

## Suggested Implementation Order

1. Lock the client architecture and data flow for WS-driven trading
2. Translate the Figma design into a component/layout plan
3. Implement real authentication and protected routing
4. Build the Figma-based trading shell
5. Connect WS market data and trading events
6. Build the admin panel workflows
7. Optimize latency and memory
8. Finish client docs and ECS deployment hardening

## Immediate Next Tasks

- [x] Validate API keys against the backend during login instead of only after session bootstrap
- [x] Point deployed client environments at `https://exchange.jamesxu.dev` and `wss://exchange.jamesxu.dev/ws`
- [x] Document the exact client deploy env values to use against the current EC2 exchange endpoint
- [x] Finish wiring live account/trading WS events end-to-end against the backend
- [ ] Define admin panel actions and permissions for market controls and messaging
- [ ] Define the file format and ingestion flow for per-user message uploads
- [ ] Add performance budgets for memory and UI update latency
- [ ] Add client logging/monitoring for REST and WS failures
- [x] Document the implemented client architecture and Figma mapping
