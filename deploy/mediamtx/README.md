# MediaMTX integration

This directory contains the first real media-server integration boundary for LIVE_TRON.

## Why MediaMTX

MediaMTX is a standalone media router with WebRTC publishing and reading, WHIP/WHEP endpoints, and an external HTTP authorization callback. It is the first deployable media-plane adapter; the Rust signaling relay remains a separate realtime signaling feature and is not used as a media transport.

Pinned configuration target: MediaMTX **v1.21.3**. Re-check the upstream configuration reference before changing the image version.

## Network contract

1. Run MediaMTX and the LIVE_TRON API on the same private container/VPC network.
2. The API must be reachable from MediaMTX as `http://live-tron-api:8080`. Change `authHTTPAddress` only to the private service DNS name actually used by the deployment.
3. Do not publish or proxy `/internal/v1/media/authorize` to the public internet. The callback deliberately has no second shared-secret header because MediaMTX's HTTP-auth callback contract does not provide configurable custom headers. Network isolation is therefore mandatory.
4. Expose the WebRTC HTTP listener on TCP 8889 through an HTTPS reverse proxy, and expose UDP 8189 directly for ICE media. Configure `MTX_WEBRTCADDITIONALHOSTS` to the public DNS name or public IP advertised to clients.
5. Do not expose the MediaMTX Control API, metrics, or pprof endpoints.

## Publish and read contract

- Stream path is the canonical stream UUID, for example `<stream-uuid>`.
- Publish with WHIP at `https://media.example.com/<stream-uuid>/whip`. Send the one-time LIVE_TRON publishing key as an `Authorization: Bearer <publishing-key>` token.
- Read with WHEP at `https://media.example.com/<stream-uuid>/whep`. The initial callback authorizes only streams that are already `live`, `public`, and non-mature.
- The callback validates publishing keys against the existing SHA-256 hashes and current revocation state. A rotated or revoked key is rejected on the next MediaMTX authorization request.
- Private and unlisted playback are intentionally denied until a short-lived viewer media-token flow is implemented.

## Lifecycle limitation

This adapter does **not** yet automatically translate MediaMTX online/offline events into LIVE_TRON lifecycle callbacks. The existing API only marks a stream live after a trusted `ingest_started` event. Do not manually mark streams live as a workaround and do not advertise this integration as production-ready until the lifecycle event bridge, TURN reachability, TLS routing, and end-to-end WHIP-to-WHEP tests are in place.

## Local configuration validation

Use the exact pinned image to validate the config before deployment:

```sh
docker run --rm \
  -v "$PWD/deploy/mediamtx:/etc/mediamtx:ro" \
  bluenviron/mediamtx:1.21.3 \
  --validate-conf=/etc/mediamtx/mediamtx.yml
```

This check validates MediaMTX's config syntax only. It does not prove network reachability, authentication, ingest, or playback.
