-- §5 had one hardware_loans row per asset, so a return erased the loan. Now assets and
-- loans are separate: every checkout is a row, and a partial unique index allows at most
-- one open (unreturned) loan per asset.
CREATE TABLE hardware_assets (
    asset_qr VARCHAR(64) PRIMARY KEY, -- 'ASSET-001'
    category TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now() -- lookup table, loaded before the event: no updated_at
);

-- RESTRICT on user_id: a user with loan history can't be deleted by accident, since an
-- unreturned loan would vanish with them. Clear their loans first.
CREATE TABLE hardware_loans (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    asset_qr VARCHAR(64) NOT NULL REFERENCES hardware_assets(asset_qr) ON DELETE RESTRICT,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    checked_out_by UUID REFERENCES users(id) ON DELETE SET NULL, -- the volunteer
    checked_out_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    returned_at TIMESTAMPTZ,
    CONSTRAINT hardware_loans_return_after_checkout CHECK (returned_at >= checked_out_at)
);
CREATE UNIQUE INDEX hardware_loans_one_open_per_asset ON hardware_loans (asset_qr) WHERE returned_at IS NULL;
CREATE INDEX hardware_loans_asset_qr ON hardware_loans (asset_qr, checked_out_at);
CREATE INDEX hardware_loans_user_id ON hardware_loans (user_id);
CREATE INDEX hardware_loans_checked_out_by ON hardware_loans (checked_out_by);
