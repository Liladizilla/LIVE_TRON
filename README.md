# LIVE_TRON

**Live, together.** A premium-dark livestream platform for creators and communities across gaming, music, talk, education, sports, IRL, and live commerce.

> Current release: **0.2.0 · Web discovery + API foundation**
>
> The web client is a frontend preview. Stream cards use curated sample data. The API foundation is now present, but account flows, creator publishing, chat, and media ingest/playback are not yet implemented.

## Product direction

- Responsive web application with an installable Progressive Web App (PWA).
- One discovery and viewing experience across desktop, tablet, and mobile.
- Category-agnostic discovery, creator channels, live chat, and eventually interactive live experiences.
- Premium dark visual system: graphite surfaces, restrained acid-lime accent, clear typography, low visual noise.
- Real-time media must be implemented and tested as infrastructure, not simulated by a decorative player.

## Current implementation

- React 19, TypeScript, Vite 6 responsive discovery experience.
- Category filters, client-side search, stream detail preview, local follow/save/chat interactions.
- PWA manifest, app mark, install prompt handling, and offline app-shell service worker.
- Rust 2021 / Axum API service with liveness, readiness, and capability metadata endpoints.
- PostgreSQL connection pool, explicit migration runner, additive initial schema, and bounded pool configuration.
- Exact-origin CORS allowlist; no wildcard origin default.
- API unit tests and CI jobs for web build, Rust formatting, Clippy, tests, and PostgreSQL migrations.

## Run the web client

Requirements: Node.js 22+ and npm.

```bash
npm install
npm run dev
```

Open the URL printed by Vite (normally http://localhost:5173).

## Run the API

Requirements: Rust stable and PostgreSQL 16+.

```bash
cd backend
cp .env.example .env
# Create the database/user described by DATABASE_URL, then export the variables:
export DATABASE_URL='postgres://live_tron:YOUR_PASSWORD@127.0.0.1:5432/live_tron'
export LIVE_TRON_BIND='127.0.0.1:8080'
export LIVE_TRON_CORS_ORIGINS='http://localhost:5173'

cargo run --bin migrate
cargo run --bin live-tron-api
```

The API and migration runner load `backend/.env` automatically when started from the `backend` directory; shell environment variables override file values. Do not put production secrets in source control.

| Endpoint | Meaning |
| --- | --- |
| `GET /healthz` | Process liveness. Does not imply database or media availability. |
| `GET /readyz` | PostgreSQL readiness. Returns 503 if the database is missing or unreachable. |
| `GET /v1` | API metadata and explicit media-plane status. |

OpenAPI contract: [backend/openapi.yaml](backend/openapi.yaml).

## Database model

The first additive migration creates:

- `users` and case-insensitive unique email index
- `channels` with owner and stable slug
- `streams` with lifecycle state, visibility, and one-live-stream-per-channel constraint
- `follows` with a composite primary key
- `refresh_sessions` storing token hashes, not raw refresh tokens
- `moderation_reports` with a review queue index

Migrations do not drop tables or cascade-delete application data. The API does not yet expose account, channel, stream, or moderation CRUD endpoints.

## Architecture plan

1. **Web client / PWA** — React, TypeScript, accessible responsive UI, service worker, install/update lifecycle.
2. **Application API** — authentication, profiles, channels, follows, discovery metadata, moderation and creator settings.
3. **Media plane** — WebRTC ingest and playback through WHIP/WHEP-compatible endpoints and a real SFU router. A signaling endpoint alone is not an SFU.
4. **Realtime plane** — authenticated chat, presence, reactions, polls, and event delivery over WebSocket/data channels where appropriate.
5. **Persistence** — PostgreSQL for durable application data; Redis for ephemeral presence, rate limits, and fanout where justified.
6. **Operations** — metrics, structured logs, abuse controls, TURN capacity, regional routing, and media quality telemetry.

### Media engineering guardrails

- Do not advertise latency or quality targets until measured on real devices and networks.
- Do not expose stream secrets in browser bundles or public channel metadata.
- Authenticate publish permissions separately from viewer access.
- Validate origin, ownership, room membership, rate limits, and moderation permissions server-side.
- A working WHIP/WHEP negotiation path must be connected to an actual SFU track router before calling the platform live.
- Plan TURN from the beginning; direct peer connectivity is not reliable on every mobile or carrier network.

## Roadmap

- [x] Product identity and responsive visual foundation
- [x] Discovery feed, categories, search, stream detail preview
- [x] PWA manifest, icon, install prompt, offline shell
- [x] Initial CI build workflow
- [x] Rust API service foundation and health/readiness endpoints
- [x] Additive PostgreSQL schema and migration runner
- [x] API tests and PostgreSQL-backed CI job
- [ ] Authentication, account lifecycle, and secure session rotation
- [ ] Creator/channel APIs and ownership authorization
- [ ] Stream lifecycle and secure publishing credential issuance
- [ ] WebRTC ingest, SFU routing, and viewer playback
- [ ] Authenticated realtime chat, moderation, and presence
- [ ] Interactive polls, reactions, and overlays
- [ ] Creator analytics, reporting, and production deployment

## Project status

This repository is the source of truth for LIVE_TRON. Features marked as preview/local are not backed by a production API. The media plane is not connected.
