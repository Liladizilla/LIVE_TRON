# MediaMTX lifecycle integration

This directory contains the MediaMTX v1.21.3 WebRTC server baseline and LIVE_TRON lifecycle event bridge.

## Event flow

MediaMTX v1.21.3 provides `runOnOnline` and `runOnOffline` path hooks. They are used here instead of the older `runOnReady` / `runOnNotReady` names.

- `runOnOnline` invokes `live-tron-media-event.sh ingest_started $MTX_PATH`.
- `runOnOffline` invokes `live-tron-media-event.sh ingest_ended $MTX_PATH`.
- The script generates one UUID event ID per invocation, builds a JSON payload, stores the shared-secret header in a mode-0600 temporary file (not curl's process arguments), and sends it to the existing authenticated `POST /internal/v1/media/events` API route.
- Curl retries reuse the same payload and event ID, so the API's idempotency key remains stable during transient failures.
- The API owns lifecycle transitions. A stream becomes `live` only after MediaMTX reports it online; it becomes `ended` when MediaMTX reports it offline.

## Runtime requirements

The MediaMTX container must provide:
- `/bin/sh`
- `curl`
- `mktemp`
- `/proc/sys/kernel/random/uuid`
- The mounted script at `/etc/mediamtx/live-tron-media-event.sh`, readable by the MediaMTX process.

Mount the script read-only. The configuration invokes it through `/bin/sh`, so the script does not need its executable bit set. The official MediaMTX image contents can vary by packaging; verify these runtime dependencies rather than assuming they exist.

Set these environment variables in the MediaMTX container:
- `LIVE_TRON_MEDIA_EVENT_SECRET`: same value as API `LIVE_TRON_MEDIA_EVENT_SECRET` (minimum 32 bytes).
- `LIVE_TRON_MEDIA_EVENT_URL`: optional private API URL; defaults to `http://live-tron-api:8080/internal/v1/media/events`.

Also set `LIVE_TRON_MEDIAMTX_AUTH_SECRET` on the API and provide the same value in `authHTTPAddress` as shown in `mediamtx.yml`. Never commit real secrets. Do not expose either internal callback to public ingress.

## Failure behavior

The hook command exits non-zero if the API remains unavailable after retries. MediaMTX logs the command failure, but the stream itself is not forcibly disconnected by this callback. Alert on failed lifecycle delivery and reconcile stream status against MediaMTX's actual online state. The API event endpoint is idempotent for repeated event IDs and rejects invalid lifecycle transitions.

## Networking

Expose the WebRTC HTTP listener on TCP 8889 through an HTTPS reverse proxy, and expose UDP 8189 for ICE media. Set `MTX_WEBRTCADDITIONALHOSTS` to the public DNS name or IP advertised to clients. Configure TURN for restrictive NATs and mobile networks. Keep the auth and event callback routes private.

## Validation

The CI job validates the MediaMTX v1.21.3 configuration and checks the lifecycle hook script with `sh -n`. These checks validate syntax only; they do not prove deployment reachability or successful WHIP-to-WHEP playback.
