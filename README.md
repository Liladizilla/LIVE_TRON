# LIVE_TRON

**Live, together.** A premium-dark livestream platform for creators and communities across gaming, music, talk, education, sports, IRL, and live commerce.

> Current release: **0.3.0 · Account, creator, and stream lifecycle API**
>
> The web client remains a frontend preview. The API supports account sessions, profiles, channels, follows, stream metadata, and secure publishing-key lifecycle. A real SFU/media plane and browser playback are still not connected.

## Product direction

- Responsive web application with an installable Progressive Web App (PWA).
- Browser WHEP playback transport for canonical stream UUIDs, with SDP negotiation, connection-state handling, retry UI, and session deletion on teardown.
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
- Owner-scoped stream creation and metadata editing, visibility controls, public live-stream discovery, and private-stream detail protection.
- 256-bit publishing keys generated from OS randomness, SHA-256 hash-only storage, one-time plaintext delivery, rotation, revocation, and internal publish authorization.
- Authenticated internal media lifecycle callbacks with constant-time shared-secret comparison, event-ID idempotency, and guarded created → live → ended transitions.
- Exact-origin CORS allowlist, a 64 KiB JSON request-body cap, validated environment settings, strict email/refresh-token/channel-slug validation, Argon2 timing equalization for unknown login accounts, password/token unit tests, and a PostgreSQL-backed account-to-channel lifecycle test.

## Run the web client

Requirements: Node.js 22+ and npm.

```bash
npm install
npm run dev
```

Open the URL printed by Vite (normally http://localhost:5173).

To use real API-backed discovery, copy the root `.env.example` to `.env.local` and set `VITE_LIVE_TRON_API_URL` to the browser-reachable API origin. Add that web origin to the API's `LIVE_TRON_CORS_ORIGINS`. When configured, discovery reads only public, live, non-mature streams from `GET /v1/streams`; an empty API result stays empty instead of silently substituting demo broadcasts. The API does not currently expose viewer counts or creator artwork, so counts are shown as unavailable and category artwork is used until those fields exist.

The browser API client keeps access and refresh tokens in memory only. Reloading the page signs the browser out. Persistent sessions should use a same-origin backend-for-frontend with Secure, HttpOnly, SameSite cookies before production deployment.

Creator Studio is available from the Go live action and account control. It supports registration/sign-in, lists channels owned by the signed-in account, creates a channel when needed, creates a stream record, and displays the one-time publishing key returned by the API. The key remains in component memory and is not recoverable after the Studio session is discarded. The browser WHIP publisher can use that key to attempt camera/microphone ingest when `VITE_MEDIAMTX_WHIP_BASE_URL` points to a configured MediaMTX server. A stream record remains `created` until MediaMTX reports an ingest lifecycle event; creating it does not make it live.

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
| POST | `/v1/channels/:slug/streams` | Owner bearer | Create stream and receive one-time publishing key |
| GET | `/v1/streams` | Public | Discover live, public, non-mature streams |
| GET | `/v1/streams/:stream_id` | Public / owner | Read stream; private streams are owner-only |
| PATCH | `/v1/streams/:stream_id` | Owner bearer | Edit title, category, or visibility before stream ends |
| POST | `/v1/streams/:stream_id/publishing-key/rotate` | Owner bearer | Revoke prior key and issue a replacement |
| DELETE | `/v1/streams/:stream_id/publishing-key` | Owner bearer | Revoke active publishing key |
| POST | `/internal/v1/streams/:stream_id/authorize-publish` | Publishing key | Validate an active key for a trusted media gateway |
| POST | `/internal/v1/media/events` | Internal shared secret | Idempotent ingest-started / ingest-ended callbacks |
| GET | `/v1/media/ice-config` | Bearer | STUN list and one-hour coturn REST credentials |
| POST | `/internal/v1/media/authorize` | Private MediaMTX network only | Validate WebRTC publish keys and public live-stream reads |
| POST | `/v1/streams/:stream_id/signal-ticket` | Bearer viewer / `Publish` key | Issue a one-use, 60-second browser WebSocket ticket |
| GET | `/v1/streams/:stream_id/signal` | One-use WebSocket ticket | Authenticated WebSocket offer/answer/ICE signaling relay |

Set `LIVE_TRON_MEDIA_EVENT_SECRET` to a unique random value of at least 32 bytes on both the API and MediaMTX before enabling lifecycle hooks. Keep it private to the media network. Publishing keys are returned once and must be stored by the creator securely.

The signaling ticket endpoint authenticates viewers with a bearer access token and publishers with `Authorization: Publish <publishing_key>`. It returns a random one-use ticket that expires after 60 seconds. Pass the returned `websocket_protocols` array to the browser WebSocket constructor; the ticket is carried in `Sec-WebSocket-Protocol`, not a URL or a custom browser header. The server stores only the ticket hash and consumes it during the WebSocket upgrade. The signaling socket relays targeted offer, answer, and ICE-candidate JSON messages between one publisher and connected viewers. A first MediaMTX adapter now provides an HTTP authorization callback and a WebRTC server configuration under `deploy/mediamtx/`; it validates publisher keys against the database and allows only public, non-mature streams already marked live for WHEP reads. Set `LIVE_TRON_MEDIAMTX_AUTH_SECRET` (at least 32 bytes) on the API and the same value in MediaMTX's `MTX_AUTHHTTPADDRESS` query parameter; keep the callback private to the media-server network. MediaMTX `runOnOnline` and `runOnOffline` hooks now call the authenticated `/internal/v1/media/events` endpoint through `deploy/mediamtx/live-tron-media-event.sh`. The hook script retries transient API failures with a stable event ID. Mount the script in the MediaMTX container, provide `/bin/sh`, `curl`, and `/proc/sys/kernel/random/uuid`, and configure `LIVE_TRON_MEDIA_EVENT_SECRET` in both services. This wires lifecycle notifications but does not yet prove end-to-end WHIP/WHEP playback. Configure `LIVE_TRON_STUN_URLS` (comma-separated) for STUN and configure both `LIVE_TRON_TURN_URLS` and `LIVE_TRON_TURN_SHARED_SECRET` for coturn REST credentials. TURN credentials are generated per account and expire after one hour. The WebSocket room registry and one-use ticket store are process-local, so this first signaling implementation requires a single API instance; Redis-backed cross-instance signaling is a later step. Each room is capped at 256 participants, each connection is capped at 60 signaling messages per second, and the process stores at most 10,000 unexpired tickets. This is a signaling relay only: it does not receive, mix, forward, or play audio/video.

Protected endpoints require `Authorization: Bearer <access_token>`. Access tokens expire after 15 minutes. Refresh tokens are opaque 256-bit random values, returned once, stored only as SHA-256 hashes, and rotated transactionally on refresh. Store refresh tokens securely in a production client; do not put them in URLs or logs.

JSON request bodies are capped at 64 KiB. Registration requires an ASCII email with a valid dotted domain, display name (2–80 characters), and password (12–128 bytes). Login returns the same public error for unknown accounts and incorrect passwords, and performs a password-hash operation for unknown accounts to reduce timing-based account enumeration. Channel slugs are lowercase URL-safe segments with single hyphen separators. Refresh tokens must be 64 hexadecimal characters and are revoked on logout or successful rotation. The initial implementation does not yet include email verification, password reset, MFA, device/session management UI, or abuse-rate limiting. Do not expose this API publicly until those production controls are added.

OpenAPI contract: [backend/openapi.yaml](backend/openapi.yaml).

### Browser WebRTC broadcasting

The reusable `LiveBroadcastPanel` publishes camera and microphone media through MediaMTX WHIP. It requires a canonical stream UUID and the one-time publishing key returned by the authenticated stream-creation API. Pass the key to the component from the creator's in-memory session; never persist it in local storage, place it in a URL, log it, or expose it in public stream metadata. Configure `VITE_MEDIAMTX_WHEP_BASE_URL` with the public HTTPS MediaMTX origin and optionally configure `VITE_MEDIAMTX_STUN_URLS`. For TURN, pass the current ICE server list from the authenticated `GET /v1/media/ice-config` response through the component's `iceServers` prop.

The publisher requests camera/microphone permission only after the creator presses Start. A pending startup can be cancelled; in-flight WHIP requests are aborted, and a device stream granted after cancellation is immediately stopped. The publisher gathers ICE candidates, sends an SDP offer to `/<stream-uuid>/whip` with the publishing key in an HTTP Bearer header, applies the SDP answer, and deletes the allocated WHIP session when stopped or unmounted. It rejects cross-origin session URLs to avoid forwarding the publishing credential to another host. MediaMTX and the browser must permit the required cross-origin WHIP requests, including authenticated DELETE. MediaMTX's bearer-token authentication and WHIP's bearer-token requirement are documented by the [MediaMTX authentication guide](https://mediamtx.org/docs/features/authentication) and [RFC 9725](https://www.rfc-editor.org/rfc/rfc9725/).

This component is intentionally not wired to the preview-only Go Live button. The current app still needs authenticated account/session UI, a real stream-creation flow, secure in-memory publishing-key handoff, and a deployed MediaMTX endpoint before creators can use it end to end. A successful TypeScript build does not prove camera ingest, TURN reachability, lifecycle callbacks, or viewer playback.

### Browser WebRTC playback

The watch page uses the WHEP player when the selected stream has a canonical UUID and `VITE_MEDIAMTX_WHEP_BASE_URL` is configured. Set that variable to the public HTTPS origin of MediaMTX (for example, `https://media.example.com`), and optionally set `VITE_MEDIAMTX_STUN_URLS` to a comma-separated STUN URL list. For authenticated TURN credentials, pass the ICE server list from the API to the player component. The player waits for ICE gathering, posts an SDP offer to `/<stream-uuid>/whep`, applies the SDP answer, and deletes the WHEP session when the player unmounts. It does not use the separate WebSocket signaling relay.

The current discovery catalogue still contains preview/demo records with non-UUID IDs, so those records intentionally continue to show the preview. Playback activates for real stream records after the frontend is connected to the stream API. This player has not yet been verified against a deployed MediaMTX server.

## Database model

The additive migrations create:

- `users` with a case-insensitive unique email index
- `channels` with owner and stable slug
- `streams` with lifecycle state, visibility, and one-live-stream-per-channel constraint
- `follows` with a composite primary key
- `refresh_sessions` storing token hashes, not raw refresh tokens
- `moderation_reports` with a review queue index
- `stream_publish_credentials` storing only SHA-256 publishing-key hashes, with one active key per stream
- `media_events` with unique event IDs for callback idempotency
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
- [x] Stream lifecycle API, owner authorization, visibility, and secure publishing credential issuance/rotation/revocation
- [x] Internal media lifecycle event authentication and idempotency
- [x] Authenticated WebSocket signaling relay and short-lived coturn credential endpoint
- [ ] MediaMTX deployment, authenticated WHIP ingest, and WHEP playback end-to-end\n- [x] MediaMTX HTTP authorization callback and WebRTC server configuration
- [ ] Authenticated realtime chat, moderation, and presence
- [ ] Interactive polls, reactions, and overlays
- [ ] Creator analytics, reporting, and production deployment

## Project status

This repository is the source of truth for LIVE_TRON. Features marked as preview/local are not backed by a production API. The media plane is not connected. Stream status changes to live only after a trusted internal ingest event. The current internal event callback, publishing-key authorization endpoint, and WebSocket signaling relay are integration boundaries, not an SFU. Signaling rooms are in-process and single-instance only. No video is ingested, routed, or played back by this release.
