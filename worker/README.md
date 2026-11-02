# LIVE_TRON Cloudflare API

This Worker is the first production backend slice. It provides Clerk session verification, PostgreSQL-backed user synchronization, stream discovery, follow/save mutations, owner-scoped R2 image uploads, and authenticated Durable Object WebSocket chat with PostgreSQL message persistence.

## Required infrastructure

1. Create a PostgreSQL database (Neon is the recommended managed provider).
2. Apply `migrations/0001_initial.sql` to that database.
3. Create a Cloudflare Hyperdrive configuration pointing at the PostgreSQL connection string. Replace the placeholder `id` in `wrangler.toml` with the returned Hyperdrive ID.
4. Create an R2 bucket named `live-tron-media`.
5. Configure Clerk for the LIVE_TRON frontend. Obtain the Clerk JWKS URL and exact JWT issuer from the Clerk application.
6. Set Worker variables/secrets:
   - `APP_ORIGIN`: production frontend origin, e.g. `https://live-tron.pages.dev`
   - `CLERK_JWKS_URL`: Clerk application's JWKS endpoint
   - `CLERK_ISSUER`: exact issuer value emitted by Clerk session tokens
7. Deploy the Worker and set the frontend API base URL to its `workers.dev` or custom-domain origin.

The Worker uses Cloudflare bindings for Hyperdrive and R2, so no R2 access key is placed in the browser or Worker environment.

## Local development

```bash
cd worker
npm install
npx wrangler dev
```

For local database access, create a local Hyperdrive configuration with Wrangler or configure a local binding using your development PostgreSQL URL. Never commit connection strings or Clerk secrets.

## Deploy

```bash
cd worker
npx wrangler deploy
```

## API

- `GET /api/health` public health check
- `GET /api/me` authenticated account bootstrap
- `GET /api/streams?category=Gaming` authenticated stream discovery
- `POST|DELETE /api/channels/:channelId/follow`
- `POST|DELETE /api/streams/:streamId/save`
- `POST /api/media/upload/:key` authenticated image upload (8 MB max, images only)
- `GET /api/streams/:streamId/chat` persisted recent chat history
- `GET /api/streams/:streamId/chat/ws` authenticated WebSocket room

All private endpoints require `Authorization: Bearer <Clerk session token>`. Chat WebSocket clients send JSON `{"type":"chat","body":"message"}`; identity is derived from the verified Clerk session, never from a client-supplied username.

## Current boundary

This is not yet the complete streaming media plane. It does not implement WHIP ingest, WHEP playback, an SFU, stream-key management, creator studio workflows, moderation tooling, notification delivery, or a Clerk webhook for profile metadata synchronization. Do not expose the platform as capable of broadcasting until the media plane is connected and tested.
