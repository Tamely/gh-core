-- sponsor_keys (used by the design doc's claim query, missing from §5). Claim with:
--   UPDATE sponsor_keys SET claimed_by_team = $1, claimed_at = now()
--   WHERE id = (SELECT id FROM sponsor_keys
--               WHERE sponsor_slug = $2 AND claimed_by_team IS NULL
--               ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED)
--   RETURNING key_value;
-- SKIP LOCKED makes concurrent claims take different keys without waiting on each other.
CREATE TABLE sponsor_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    sponsor_slug VARCHAR(48) NOT NULL,
    key_value TEXT NOT NULL UNIQUE,
    -- RESTRICT: a team that saw a key can't be deleted and put the key back in the pool.
    claimed_by_team UUID REFERENCES teams(id) ON DELETE RESTRICT,
    claimed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT sponsor_keys_claim_complete CHECK ((claimed_by_team IS NULL) = (claimed_at IS NULL)),
    -- One key per sponsor per team. Also the index for claimed_by_team.
    CONSTRAINT sponsor_keys_one_per_team UNIQUE (claimed_by_team, sponsor_slug)
);
-- The claim query's subselect: next unclaimed key for a sponsor, in id order.
CREATE INDEX sponsor_keys_unclaimed ON sponsor_keys (sponsor_slug, id) WHERE claimed_by_team IS NULL;
