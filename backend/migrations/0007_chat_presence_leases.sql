-- Chat WebSocket tickets are shared by all API replicas and consumed atomically.
-- Only a SHA-256 digest is stored; the random bearer value is returned once.
CREATE TABLE IF NOT EXISTS chat_ws_tickets (
    ticket_hash CHAR(64) PRIMARY KEY,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chat_ws_tickets_hash_format CHECK (ticket_hash ~ '^[0-9a-f]{64}$')
);

CREATE INDEX IF NOT EXISTS chat_ws_tickets_expiry_idx
    ON chat_ws_tickets(expires_at);

-- Expired tickets are rejected at consumption; this index supports periodic cleanup.

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
