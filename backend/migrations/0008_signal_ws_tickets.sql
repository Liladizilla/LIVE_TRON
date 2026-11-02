-- Signaling tickets are shared by all API replicas and consumed atomically.
-- Store only a one-way digest of the random bearer ticket.
CREATE TABLE IF NOT EXISTS signal_ws_tickets (
    ticket_hash CHAR(64) PRIMARY KEY,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    publisher BOOLEAN NOT NULL,
    publisher_key_hash TEXT,
    viewer_user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT signal_ws_tickets_hash_format CHECK (ticket_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT signal_ws_tickets_identity CHECK (
        (publisher AND publisher_key_hash IS NOT NULL AND viewer_user_id IS NULL)
        OR
        (NOT publisher AND publisher_key_hash IS NULL AND viewer_user_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS signal_ws_tickets_expiry_idx
    ON signal_ws_tickets(expires_at);
