# LIVE_TRON

**Live, together.** A premium-dark livestream platform for creators and communities across gaming, music, talk, education, sports, IRL, and live commerce.

> Current release: **0.3.0 · Web discovery + account and creator API foundation**
>
> The web client remains a frontend preview. The API now supports account sessions, profiles, channels, and follows. Creator broadcasting, chat, and media ingest/playback are not yet implemented.

## Product direction

- Responsive web application with an installable Progressive Web App (PWA).
- One discovery and viewing experience across desktop, tablet, and mobile.
- Category-agnostic discovery, creator channels, live chat, and eventually interactive live experiences.
- Premium dark visual system: graphite surfaces, restrained acid-lime accent, clear typography, low visual noise.
- Real-time media must be implemented and tested as infrastructure, not simulated by a decorative player.

## Current implementation

- React 19, TypeScript, Vite 6 responsive discovery experience and PWA shell.
- Rust 2021 / Axum API with liveness, readiness, and capability metadata endpoints.
- PostgreSQL connection pool and additive schema for users, channels, streams, follows, refresh sessions, and moderation reports.
- Argon2id password hashing, short-lived HS256 access tokens scoped to the LIVE_TRON issuer/audience, opaque refresh tokens stored as SHA-256 hashes, and transactional refresh rotation.
- Registration, login, refresh, logout, current-profile, and profile-update endpoints.
- Creator channel creation, owner-only channel updates, public channel discovery, creator profiles, and follow/unfollow endpoints.
- Exact-origin CORS allowlist, validated environment settings, strict email/refresh-token/channel-slug validation, password/token unit tests, and a PostgreSQL-backed account-to-channel lifecycle test.

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
# Edit .env: set DATABASE_URL and generate a unique LIVE_TRON_JWT_SECRET
# of at least 32 bytes (for example, `openssl rand -hex 32`). Create the PostgreSQL database/user first.
cargo run --bin migrate
cargo run --bin live-tron-api
```

The API and migration runner load `backend/.env` automatically when started from the `backend` directory; shell environment variables override file values. Never commit real credentials. If `LIVE_TRON_JWT_SECRET` is missing, authenticated routes fail closed with 503.

## API endpoints

| Method | Endpoint | Access | Purpose |
| --- | --- | --- | --- |
| GET | `/healthz` | Public | Process liveness |
| GET | `/readyz` | Public | PostgreSQL readiness |
| GET | `/v1` | Public | API and media-plane capability metadata |
| POST | `/v1/auth/register` | Public | Create account and initial session |
| POST | `/v1/auth/login` | Public | Authenticate and issue tokens |
| POST | `/v1/auth/refresh` | Public | Rotate refresh token and issue access token |
| POST | `/v1/auth/logout` | Public | Revoke supplied refresh token |
| GET | `/v1/me` | Bearer | Read current account |
| PATCH | `/v1/me` | Bearer | Update display name |
| GET | `/v1/channels` | Public | Search and filter channels |
| POST | `/v1/channels` | Bearer | Create an owned channel |
| GET | `/v1/channels/:slug` | Public | Read channel details |
| PATCH | `/v1/channels/:slug` | Owner bearer | Update an owned channel |
| POST | `/v1/channels/:slug/follow` | Bearer | Follow a channel |
| DELETE | `/v1/channels/:slug/follow` | Bearer | Unfollow a channel |
| GET | `/v1/creators/:slug` | Public | Read creator profile and channels |

Protected endpoints require `Authorization: Bearer <access_token>`. Access tokens expire after 15 minutes. Refresh tokens are opaque 256-bit random values, returned once, stored only as SHA-256 hashes, and rotated transactionally on refresh. Store refresh tokens securely in a production client; do not put them in URLs or logs.

Registration requires an ASCII email with a valid dotted domain, display name (2–80 characters), and password (12–128 bytes). Channel slugs are lowercase URL-safe segments with single hyphen separators. Refresh tokens must be 64 hexadecimal characters and are revoked on logout or successful rotation. The initial implementation does not yet include email verification, password reset, MFA, device/session management UI, or abuse-rate limiting. Do not expose this API publicly until those production controls are added.

OpenAPI contract: [backend/openapi.yaml](backend/openapi.yaml).

## Database model

The additive migrations create:

- `users` with a case-insensitive unique email index
- `channels` with owner and stable slug
- `streams` with lifecycle state, visibility, and one-live-stream-per-channel constraint
- `follows` with a composite primary key
- `refresh_sessions` storing token hashes, not raw refresh tokens
- `moderation_reports` with a review queue index
- A follow-up validation migration that tightens channel slug and category constraints without deleting application rows

Migrations do not drop tables or cascade-delete application data.

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

- [x] Product identity, responsive discovery feed, categories, search, stream preview
- [x] PWA manifest, app icon, install prompt, offline shell
- [x] Rust API service and PostgreSQL migration runner
- [x] Account registration, login, password hashing, token issuance and refresh rotation
- [x] Current profile and profile update endpoints
- [x] Creator channel creation, ownership checks, discovery and public profiles
- [x] Follow and unfollow endpoints
- [ ] Email verification, password reset, MFA, and session management
- [ ] Rate limiting, abuse protection, audit events, and account lockout policy
- [ ] Stream lifecycle and secure publishing credential issuance
- [ ] WebRTC ingest, SFU routing, and viewer playback
- [ ] Authenticated realtime chat, moderation, and presence
- [ ] Interactive polls, reactions, and overlays
- [ ] Creator analytics, reporting, and production deployment

## Project status

This repository is the source of truth for LIVE_TRON. Features marked as preview/local are not backed by a production API. The media plane is not connected.
