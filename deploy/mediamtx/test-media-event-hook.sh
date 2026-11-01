#!/bin/sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

cat > "$tmp/curl" <<'MOCK_CURL'
#!/bin/sh
printf '%s\n' "$@" > "$ARGS_CAPTURE"
for arg in "$@"; do
  case "$arg" in
    @*) cat "${arg#@}" >> "$CAPTURE" ;;
  esac
done
MOCK_CURL
chmod +x "$tmp/curl"

stream_id="00000000-0000-4000-8000-000000000001"
export PATH="$tmp:$PATH"
export CAPTURE="$tmp/curl-args"
export ARGS_CAPTURE="$tmp/curl-argv"
export LIVE_TRON_MEDIA_EVENT_SECRET="test-only-secret-that-is-long-enough"

/bin/sh "$root/live-tron-media-event.sh" ingest_started "$stream_id"
grep -F -- 'X-Live-Tron-Media-Secret: test-only-secret-that-is-long-enough' "$CAPTURE" >/dev/null
if grep -F -- 'test-only-secret-that-is-long-enough' "$ARGS_CAPTURE" >/dev/null; then
  echo "shared secret leaked into curl arguments" >&2
  exit 1
fi
grep -F -- '--retry' "$ARGS_CAPTURE" >/dev/null
grep -F -- '--retry-all-errors' "$ARGS_CAPTURE" >/dev/null
grep -F -- "\"stream_id\":\"$stream_id\"" "$CAPTURE" >/dev/null
grep -F -- '"event_type":"ingest_started"' "$CAPTURE" >/dev/null
grep -F -- 'http://live-tron-api:8080/internal/v1/media/events' "$CAPTURE" >/dev/null

/bin/sh "$root/live-tron-media-event.sh" ingest_ended "$stream_id"
grep -F -- '"event_type":"ingest_ended"' "$CAPTURE" >/dev/null

if /bin/sh "$root/live-tron-media-event.sh" unsupported "$stream_id" >/dev/null 2>&1; then
  echo "unsupported event type was accepted" >&2
  exit 1
fi

if /bin/sh "$root/live-tron-media-event.sh" ingest_started "not-a-uuid" >/dev/null 2>&1; then
  echo "invalid stream path was accepted" >&2
  exit 1
fi

echo "MediaMTX lifecycle hook tests passed"
