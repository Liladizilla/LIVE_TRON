-- Stream credentials and media event idempotency. No application rows are deleted.
ALTER TABLE streams
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE TABLE IF NOT EXISTS stream_publish_credentials (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE RESTRICT,
    key_hash CHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    CONSTRAINT stream_publish_credentials_hash_format CHECK (key_hash ~ '^[0-9a-f]{64}$')
);

CREATE UNIQUE INDEX IF NOT EXISTS stream_publish_credentials_one_active
    ON stream_publish_credentials(stream_id) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS stream_publish_credentials_hash_lookup
    ON stream_publish_credentials(key_hash) WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS media_events (
    event_id UUID PRIMARY KEY,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE RESTRICT,
    event_type VARCHAR(24) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT media_events_type_valid CHECK (event_type IN ('ingest_started', 'ingest_ended'))
);
CREATE INDEX IF NOT EXISTS media_events_stream_created_idx
    ON media_events(stream_id, created_at DESC);
