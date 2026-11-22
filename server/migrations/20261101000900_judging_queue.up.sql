-- Design doc §5 judging_queue. (room_id, seq_index) is unique, but checked at COMMIT
-- (DEFERRABLE INITIALLY DEFERRED), so "Swap with On-Deck Team" can swap two rows'
-- seq_index inside one transaction. A transaction that still has a duplicate fails at COMMIT.
CREATE TABLE judging_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    room_id VARCHAR(32) NOT NULL REFERENCES presentation_rooms(room_id) ON DELETE CASCADE,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    track VARCHAR(48) NOT NULL,
    seq_index INT NOT NULL CHECK (seq_index >= 0),
    status VARCHAR(20) NOT NULL DEFAULT 'QUEUED'
        CHECK (status IN ('QUEUED', 'IN_THE_HOLE', 'ON_DECK', 'PRESENTING', 'DONE', 'SKIPPED')),
    raw_score DOUBLE PRECISION,
    prev_team_id UUID REFERENCES teams(id) ON DELETE SET NULL,
    beat_prev_team BOOLEAN,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Also the index for room_id.
    CONSTRAINT judging_queue_room_seq UNIQUE (room_id, seq_index) DEFERRABLE INITIALLY DEFERRED
);
CREATE TRIGGER judging_queue_updated_at BEFORE UPDATE ON judging_queue
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX judging_queue_team_id ON judging_queue (team_id);
CREATE INDEX judging_queue_prev_team_id ON judging_queue (prev_team_id);
