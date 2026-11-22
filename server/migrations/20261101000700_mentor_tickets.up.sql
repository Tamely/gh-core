-- Design doc §5 mentor_tickets, with a status CHECK and queue indexes.
CREATE TABLE mentor_tickets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID REFERENCES teams(id) ON DELETE CASCADE,
    location_label VARCHAR(64) NOT NULL,
    svg_x REAL NOT NULL,
    svg_y REAL NOT NULL,
    tags TEXT[] NOT NULL,
    description TEXT NOT NULL,
    status VARCHAR(24) NOT NULL DEFAULT 'OPEN'
        CHECK (status IN ('OPEN', 'CLAIMED', 'RESOLVED', 'CANCELED')),
    claimed_by UUID REFERENCES users(id) ON DELETE SET NULL, -- the mentor
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at TIMESTAMPTZ,
    resolved_at TIMESTAMPTZ,
    CONSTRAINT mentor_tickets_claimed CHECK (status <> 'CLAIMED' OR (claimed_by IS NOT NULL AND claimed_at IS NOT NULL)),
    CONSTRAINT mentor_tickets_resolved CHECK (status <> 'RESOLVED' OR resolved_at IS NOT NULL)
);
-- A claimed ticket whose mentor is unset (unclaimed, or the mentor's account deleted, which
-- ON DELETE SET NULL does) goes back to the open queue instead of being stuck CLAIMED.
CREATE FUNCTION mentor_tickets_reopen() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.status = 'CLAIMED' AND NEW.claimed_by IS NULL THEN
        NEW.status := 'OPEN';
        NEW.claimed_at := NULL;
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER mentor_tickets_reopen BEFORE UPDATE OF claimed_by ON mentor_tickets
    FOR EACH ROW EXECUTE FUNCTION mentor_tickets_reopen();
CREATE TRIGGER mentor_tickets_updated_at BEFORE UPDATE ON mentor_tickets
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX mentor_tickets_team_id ON mentor_tickets (team_id);
CREATE INDEX mentor_tickets_claimed_by ON mentor_tickets (claimed_by);
-- The mentor queue: open tickets, oldest first, filtered by tag.
CREATE INDEX mentor_tickets_open ON mentor_tickets (created_at) WHERE status = 'OPEN';
CREATE INDEX mentor_tickets_tags ON mentor_tickets USING gin (tags);
