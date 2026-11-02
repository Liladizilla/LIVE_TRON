-- Shared WebRTC peer presence and publisher reservation across API replicas.
CREATE TABLE IF NOT EXISTS signal_peer_leases (
    participant_id UUID PRIMARY KEY,
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    publisher BOOLEAN NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS signal_peer_leases_stream_expiry_idx
    ON signal_peer_leases(stream_id, expires_at);

CREATE INDEX IF NOT EXISTS signal_peer_leases_expiry_idx
    ON signal_peer_leases(expires_at);

CREATE UNIQUE INDEX IF NOT EXISTS signal_peer_leases_one_publisher_idx
    ON signal_peer_leases(stream_id)
    WHERE publisher = TRUE;
