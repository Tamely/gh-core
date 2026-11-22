-- entitlements (missing from §5) is the list of things a scan can grant, so a scan of an
-- unknown entitlement_id is rejected by the foreign key.
CREATE TABLE entitlements (
    entitlement_id VARCHAR(32) PRIMARY KEY CHECK (entitlement_id ~ '^[a-z0-9-]+$'), -- 'checkin', 'dinner-1', 'shirt'
    label TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now() -- lookup table, loaded before the event: no updated_at
);

-- Design doc §5 entitlement_scans. The primary key is the duplicate check: a second scan
-- fails with 23505, or inserts nothing with ON CONFLICT DO NOTHING. It must stay a plain
-- (non-deferrable) key, since ON CONFLICT can't use a deferrable one.
CREATE TABLE entitlement_scans (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entitlement_id VARCHAR(32) NOT NULL REFERENCES entitlements(entitlement_id) ON DELETE RESTRICT,
    scanned_by UUID REFERENCES users(id) ON DELETE SET NULL, -- the volunteer
    client_scanned_at TIMESTAMPTZ NOT NULL,
    server_synced_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, entitlement_id)
);
CREATE INDEX entitlement_scans_entitlement_id ON entitlement_scans (entitlement_id);
CREATE INDEX entitlement_scans_scanned_by ON entitlement_scans (scanned_by);
