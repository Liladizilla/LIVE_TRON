#!/bin/sh
# MediaMTX runOnOnline/runOnOffline callback.
# Generate the id once so curl retries preserve the same idempotency key.
set -eu

event_type="${1:-}"
stream_id="${2:-}"

case "$event_type" in
  ingest_started|ingest_ended) ;;
  *) echo "unsupported media event type" >&2; exit 64 ;;
esac

case "$stream_id" in
  ????????-????-????-????-????????????) ;;
  *) echo "MTX_PATH must be a canonical stream UUID" >&2; exit 64 ;;
esac

: "${LIVE_TRON_MEDIA_EVENT_SECRET:?LIVE_TRON_MEDIA_EVENT_SECRET is required}"
: "${LIVE_TRON_MEDIA_EVENT_URL:=http://live-tron-api:8080/internal/v1/media/events}"

event_id="$(cat /proc/sys/kernel/random/uuid)"
payload="$(printf '{"event_id":"%s","stream_id":"%s","event_type":"%s"}' "$event_id" "$stream_id" "$event_type")"

# Do not place the shared secret in curl's argv, where same-container users
# could read it through process listings. curl reads the header from this
# private temporary file instead.
umask 077
header_file="$(mktemp)"
trap 'rm -f "$header_file"' EXIT HUP INT TERM
printf 'X-Live-Tron-Media-Secret: %s\n' "$LIVE_TRON_MEDIA_EVENT_SECRET" > "$header_file"

curl \
  --fail \
  --silent \
  --show-error \
  --connect-timeout 2 \
  --max-time 10 \
  --retry 5 \
  --retry-all-errors \
  --retry-delay 1 \
  --request POST \
  --header 'Content-Type: application/json' \
  --header "@$header_file" \
  --data "$payload" \
  "$LIVE_TRON_MEDIA_EVENT_URL"
