-- Design doc §5 teams, plus team_members (missing from §5).
CREATE TABLE teams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT UNIQUE NOT NULL,
    hacking_zone_id VARCHAR(32), -- optional SVG home zone, e.g. 'pft-atrium'
    svg_x REAL,
    svg_y REAL,
    devpost_url TEXT,
    opted_tracks TEXT[] NOT NULL DEFAULT '{}',
    heartbeat_confirmed_at TIMESTAMPTZ, -- T-30m presentation gate
    bt_theta DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    final_composite_score DOUBLE PRECISION,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER teams_updated_at BEFORE UPDATE ON teams
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- One team per hacker, so user_id is the key.
CREATE TABLE team_members (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    team_id UUID NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX team_members_team_id ON team_members (team_id);
