-- Design doc §5 presentation_rooms. The raw panel_hmac_token is replaced by what judge
-- room access needs: a token version and a SHA-256 hash (never the token), plus the
-- room's judging window. No window (NULL) means the room's placard doesn't work yet.
CREATE TABLE presentation_rooms (
    room_id VARCHAR(32) PRIMARY KEY CHECK (room_id ~ '^[A-Za-z0-9_-]+$'), -- e.g. 'PFT-1200'
    track VARCHAR(48) NOT NULL,      -- 'GENERAL' or a sponsor slug
    floor_number SMALLINT NOT NULL,
    svg_x REAL NOT NULL,
    svg_y REAL NOT NULL,
    token_version INT NOT NULL DEFAULT 1 CHECK (token_version > 0),
    token_hash BYTEA UNIQUE,
    judging_starts_at TIMESTAMPTZ,
    judging_ends_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT presentation_rooms_window CHECK (judging_ends_at > judging_starts_at)
);
CREATE TRIGGER presentation_rooms_updated_at BEFORE UPDATE ON presentation_rooms
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
