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
- Mock session-based auth flow
- Keybind provider scaffold
- Health endpoint
- Live exchange backend is available for integration testing at `http://16.59.150.9:8080` and `ws://16.59.150.9:8080/ws`

What is still true:

- The app is intended for an internal competition, not a public trading product
- The client is no longer a starter template, but it is still not a production-ready trading system
- The trading UI and design system are implemented, but still need final polish and broader route coverage
- Real-time transport and trading UX are implemented client-side, but still depend on backend completeness and load validation
- The currently deployed backend endpoint is plain HTTP/WS only; TLS is not configured yet
- Individual authentication is not implemented for production
- Admin workflows are not implemented

## Progress Snapshot

### Done

- Next.js app structure exists in `client/src/app`
- Route scaffolds exist for:
  - `/login`
  - `/trade`
  - `/admin`
- Mock auth flow exists
- Role gating scaffold exists for trader/admin flows
- Keybind support exists for navigation:
  - `Ctrl/Cmd + K` -> `/trade`
  - `Ctrl/Cmd + G` -> `/admin`
- ECS deployment intent is already documented in [client/README.md](/Users/james/Desktop/Coding/exchange-v2/client/README.md)
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
  - mock login only
  - not tied to backend auth model
- Admin panel:
  - route exists
  - no real controls yet
- Trader UI:
  - Figma shell exists
  - stateful workflows exist
  - still using a mix of mock/synthetic behavior until backend capabilities are finalized
- Real-time integration:
  - REST bootstrap and WS connection logic exist
  - public health, WS auth, and L3 snapshot probes now succeed against `16.59.150.9`
  - depends on live backend endpoints and final event model
- Performance:
  - framework baseline is light
  - no production profiling, budgets, or load validation yet

### Not Started / Missing

- Real individual authentication
- Low-latency client architecture work
- Memory budget enforcement
- End-to-end UX polish
- Production admin workflows
- Broader validation of live WS/order flows under realistic competition traffic
- Performance/load testing under sustained market updates

## Product Requirements

- [x] Next.js app
- [ ] Hosted on ECS
- [x] Clean UX/UI
- [ ] Low latency
- [ ] Low memory footprint
- [ ] Individual authentication
- [ ] Admin panel to run the event
- [x] Keybinds
- [x] Implement the referenced Figma design
- [ ] Internal-competition API-key login flow
- [ ] Admin messaging workflows for broadcast and user-specific communication

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

- [ ] Finalize ECS deployment strategy
- [ ] Use container image from `client/Dockerfile`
- [ ] Add environment management for production
- [ ] Add health checks to `/api/health`
- [ ] Add CDN / caching strategy for static assets
- [ ] Add rollout / rollback plan
- [ ] Add monitoring for memory and response times

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

- Mock login only

### Required

- [ ] Replace mock login with API-key login for the internal competition
- [ ] Support individual trader accounts keyed by assigned API keys
- [ ] Align auth model with backend auth/API key strategy
- [ ] Validate API key on login and establish client session from it
- [ ] Persist authenticated trader identity for REST and WS requests
- [ ] Add protected route handling
- [ ] Add session expiry / refresh behavior
- [ ] Add logout flow
- [ ] Add admin-vs-trader authorization rules

## 6. Admin Panel

The client needs an admin panel to run the event.

### Current

- `/admin` route scaffold exists

### Required

- [ ] Define event-ops workflows
- [ ] Add admin dashboard layout
- [ ] Add operational controls, for example:
  - add markets
  - market pause/resume
  - start trading
  - clear orderbook
  - enable/disable markets visible to users
  - event messaging / announcements
  - send broadcast messages to all users
  - send unique messages to individual users
  - load a file of per-user messages and send them in bulk
  - monitoring views
- [ ] Add audit visibility for admin actions
- [ ] Restrict access to admin users only

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
- [ ] Subscribe to account/trading events
- [x] Render L3 updates efficiently
- [x] Handle reconnect and resync
- [x] Surface connection degradation clearly

## 10. Trading Workflow

- [x] Submit order from the order ticket
- [x] Show order ACK / reject state
- [x] Show fills and order lifecycle updates
- [x] Show current positions
- [ ] Show balances / buying power if needed
- [x] Keep UI state consistent with server events

## 11. Documentation

- [ ] Client architecture doc
- [ ] Auth flow doc for API-key login
- [ ] ECS deployment doc
- [ ] Figma-to-implementation mapping doc
- [ ] Keybind reference
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

- [ ] Replace mock login with the API-key login flow for the competition
- [ ] Point deployed client environments at `http://16.59.150.9:8080` and `ws://16.59.150.9:8080/ws` until TLS is added
- [ ] Finish wiring live account/trading WS events end-to-end against the backend
- [ ] Define admin panel actions and permissions for market controls and messaging
- [ ] Define the file format and ingestion flow for per-user message uploads
- [ ] Add performance budgets for memory and UI update latency
- [ ] Add client logging/monitoring for REST and WS failures
- [ ] Document the implemented client architecture and Figma mapping
