-- Judge room access (auth PR 4). Kenny's design-doc §5 schema comes later and
-- adds track, floor and map position to `presentation_rooms` with ALTER TABLE.

-- One row per presentation room. The placard token is never stored: only its
-- version and a SHA-256 hash. Rotating bumps the version, so old placards fail.
-- No judging window (NULL) means the room's placard doesn't work yet.
CREATE TABLE presentation_rooms (
    room_id           VARCHAR(32) PRIMARY KEY CHECK (room_id ~ '^[A-Za-z0-9_-]+$'),
    token_version     INT NOT NULL DEFAULT 1 CHECK (token_version > 0),
    token_hash        BYTEA UNIQUE,
    judging_starts_at TIMESTAMPTZ,
    judging_ends_at   TIMESTAMPTZ,
    CHECK (judging_ends_at > judging_starts_at)
);

-- A session belongs to a user or, for judges, to a room. Never both, never neither.
ALTER TABLE sessions
    ALTER COLUMN user_id DROP NOT NULL,
    ADD COLUMN room_id VARCHAR(32) REFERENCES presentation_rooms(room_id) ON DELETE CASCADE,
    ADD CONSTRAINT sessions_user_or_room CHECK ((user_id IS NULL) <> (room_id IS NULL));
-- Rotating a placard revokes the room's sessions.
CREATE INDEX sessions_room_id ON sessions (room_id) WHERE room_id IS NOT NULL;

-- Every placard exchange, accepted or not. Never the token itself.
-- `room_id` is whatever the token claimed, so no foreign key: forged tokens name
-- rooms that don't exist, and NULL means the token didn't even decode.
CREATE TABLE judge_exchange_audit (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    room_id    VARCHAR(32),
    at         TIMESTAMPTZ NOT NULL,
    user_agent TEXT,
    ip         TEXT,
    outcome    TEXT NOT NULL CHECK (outcome IN ('ok', 'malformed', 'bad_signature', 'unknown_room', 'old_version', 'outside_window')),
    -- Set on success. A rejected exchange never has one.
    session_id UUID REFERENCES sessions(id) ON DELETE SET NULL,
    CHECK (outcome = 'ok' OR session_id IS NULL)
);
CREATE INDEX judge_exchange_audit_room_at ON judge_exchange_audit (room_id, at);
