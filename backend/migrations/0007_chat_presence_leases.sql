-- Presence leases are shared across API replicas. A crashed process stops refreshing
-- its leases, which expire automatically and cannot leave permanent ghost viewers.
CREATE TABLE IF NOT EXISTS chat_presence_leases (
    connection_id UUID PRIMARY KEY,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS chat_presence_stream_expiry_idx
    ON chat_presence_leases(stream_id, expires_at);

CREATE INDEX IF NOT EXISTS chat_presence_expiry_idx
    ON chat_presence_leases(expires_at);
