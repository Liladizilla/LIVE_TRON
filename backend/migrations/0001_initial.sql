-- LIVE_TRON application schema. Migrations are additive and safe to re-run.
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    display_name VARCHAR(80) NOT NULL,
    avatar_url TEXT,
    email_verified_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT users_email_not_blank CHECK (length(trim(email)) > 0),
    CONSTRAINT users_display_name_not_blank CHECK (length(trim(display_name)) > 0)
);

CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_unique ON users (lower(email));

CREATE TABLE IF NOT EXISTS channels (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    slug VARCHAR(48) NOT NULL UNIQUE,
    title VARCHAR(100) NOT NULL,
    description VARCHAR(2000) NOT NULL DEFAULT '',
    category VARCHAR(32) NOT NULL DEFAULT 'talk',
    is_mature BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT channels_slug_format CHECK (slug ~ '^[a-z0-9][a-z0-9-]{2,47}$'),
    CONSTRAINT channels_title_not_blank CHECK (length(trim(title)) > 0)
);

CREATE INDEX IF NOT EXISTS channels_owner_idx ON channels(owner_user_id);

CREATE TABLE IF NOT EXISTS streams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE RESTRICT,
    title VARCHAR(120) NOT NULL,
    category VARCHAR(32) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'created',
    visibility VARCHAR(16) NOT NULL DEFAULT 'public',
    started_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT streams_status_valid CHECK (status IN ('created', 'live', 'ended', 'failed')),
    CONSTRAINT streams_visibility_valid CHECK (visibility IN ('public', 'unlisted', 'private')),
    CONSTRAINT streams_time_order CHECK (ended_at IS NULL OR started_at IS NULL OR ended_at >= started_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS streams_one_live_per_channel
    ON streams(channel_id) WHERE status = 'live';
CREATE INDEX IF NOT EXISTS streams_discovery_idx
    ON streams(status, visibility, started_at DESC);

CREATE TABLE IF NOT EXISTS follows (
    follower_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (follower_user_id, channel_id)
);
CREATE INDEX IF NOT EXISTS follows_channel_idx ON follows(channel_id);

CREATE TABLE IF NOT EXISTS refresh_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT refresh_sessions_hash_not_blank CHECK (length(trim(token_hash)) > 0)
);
CREATE INDEX IF NOT EXISTS refresh_sessions_user_idx ON refresh_sessions(user_id);
CREATE INDEX IF NOT EXISTS refresh_sessions_expiry_idx ON refresh_sessions(expires_at)
    WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS moderation_reports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    reporter_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    stream_id UUID REFERENCES streams(id) ON DELETE SET NULL,
    reason VARCHAR(40) NOT NULL,
    details VARCHAR(2000) NOT NULL DEFAULT '',
    status VARCHAR(16) NOT NULL DEFAULT 'open',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ,
    CONSTRAINT moderation_reports_status_valid CHECK (status IN ('open', 'reviewing', 'resolved', 'dismissed')),
    CONSTRAINT moderation_reports_reason_not_blank CHECK (length(trim(reason)) > 0)
);
CREATE INDEX IF NOT EXISTS moderation_reports_queue_idx
    ON moderation_reports(status, created_at);
