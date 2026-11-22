-- Judge room access (auth PR 4). The core schema's `presentation_rooms` already has the
-- placard's token_version and token_hash (never the token) and the judging window.

-- A session belongs to a user or, for judges, to a room. Never both, never neither.
ALTER TABLE sessions
    ALTER COLUMN user_id DROP NOT NULL,
    -- RESTRICT, not CASCADE: deleting rows would skip the Redis revocation cache,
    -- so their tokens would keep working. Rotate (which revokes) before deleting.
    ADD COLUMN room_id VARCHAR(32) REFERENCES presentation_rooms(room_id) ON DELETE RESTRICT,
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
CREATE INDEX judge_exchange_audit_session_id ON judge_exchange_audit (session_id);
