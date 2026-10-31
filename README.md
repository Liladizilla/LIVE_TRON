# LIVE_TRON

**Live, together.** A premium-dark livestream platform for creators and communities across gaming, music, talk, education, sports, IRL, and live commerce.

> Current release: **0.1.0 · Discovery foundation**
>
> The current UI is a frontend preview. Stream cards use curated sample data and remote editorial imagery. No live media is being ingested or delivered yet.

## Product direction

- Responsive web application with an installable Progressive Web App (PWA).
- One discovery and viewing experience across desktop, tablet, and mobile.
- Category-agnostic discovery, creator channels, live chat, and eventually interactive live experiences.
- Premium dark visual system: graphite surfaces, restrained acid-lime accent, clear typography, low visual noise.
- Real-time media must be implemented and tested as infrastructure, not simulated by a decorative player.

## Current implementation

- React 19, TypeScript, Vite 6.
- Responsive application shell with desktop navigation and mobile drawer.
- Discovery feed with category filters and client-side search.
- Featured stream, stream cards, creator identity, viewer counts, and watch detail layout.
- Local preview interactions: follow, save, and chat composition.
- PWA manifest, app mark, install prompt handling, and a small offline app-shell service worker.
- GitHub Actions CI for TypeScript checks and production builds.

## Run locally

Requirements: Node.js 22+ and npm.

```bash
npm install
npm run dev
```

Open the URL printed by Vite (normally http://localhost:5173).

Production validation:

```bash
npm run build
npm run preview
```

## Architecture plan

The first release is intentionally a frontend foundation. The production system is planned as independently testable services:

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
- [ ] Account system and creator/channel profiles
- [ ] API contract, database schema, migrations, and seed data
- [ ] Creator Studio and secure stream-key lifecycle
- [ ] WebRTC ingest, SFU routing, and viewer playback
- [ ] Authenticated realtime chat, moderation, and presence
- [ ] Interactive polls, reactions, and overlays
- [ ] Creator analytics, reporting, and production deployment

## Project status

This repository is the source of truth for LIVE_TRON. Features marked as preview/local are not backed by a production API. The next engineering batch should establish the API contracts and persistence model before connecting account-dependent UI or real-time media.
