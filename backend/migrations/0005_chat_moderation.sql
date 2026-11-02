CREATE TABLE IF NOT EXISTS chat_restrictions (
    stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('mute', 'ban')),
    reason VARCHAR(240),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (stream_id, user_id)
);

CREATE INDEX IF NOT EXISTS chat_restrictions_active_idx
    ON chat_restrictions(stream_id, expires_at);
