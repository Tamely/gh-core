-- Design doc section 5, with two fixes:
--   * ids use the built-in gen_random_uuid() instead of the uuid-ossp extension.
--   * judging_queue's (room_id, seq_index) uniqueness is checked at commit, so
--     "Swap with On-Deck Team" can swap two seq_index values in one transaction.
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT UNIQUE NOT NULL,
    phone_e164 VARCHAR(20) NOT NULL,
    full_name TEXT NOT NULL,
    school TEXT NOT NULL,
    dietary_flags SMALLINT DEFAULT 0,
    qr_hmac_sig BYTEA NOT NULL,
    checked_in_at TIMESTAMPTZ,
    collateral_held BOOLEAN DEFAULT FALSE,
    travel_reimbursement_cap_cents INT DEFAULT 0,
    face_embedding vector(512), -- computed on-device; the selfie never reaches the server
    apns_fcm_tokens TEXT[] DEFAULT '{}',
    webpush_sub JSONB,
    live_activity_token TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE teams (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT UNIQUE NOT NULL,
    hacking_zone_id VARCHAR(32), -- optional SVG home zone, e.g. 'pft-atrium'
    svg_x REAL,
    svg_y REAL,
    devpost_url TEXT,
    opted_tracks TEXT[] DEFAULT '{}',
    heartbeat_confirmed_at TIMESTAMPTZ, -- T-30m presentation gate
    bt_theta DOUBLE PRECISION DEFAULT 0.0,
    final_composite_score DOUBLE PRECISION
);

CREATE TABLE presentation_rooms (
    room_id VARCHAR(32) PRIMARY KEY, -- e.g. 'PFT-1200'
    track VARCHAR(48) NOT NULL,      -- 'GENERAL' or a sponsor slug
    floor_number SMALLINT NOT NULL,
    svg_x REAL NOT NULL,
    svg_y REAL NOT NULL,
    panel_hmac_token TEXT UNIQUE NOT NULL
);

CREATE TABLE entitlement_scans (
    user_id UUID REFERENCES users(id),
    entitlement_id VARCHAR(32) NOT NULL, -- 'checkin', 'dinner-1', 'shirt', ...
    scanned_by UUID REFERENCES users(id),
    client_scanned_at TIMESTAMPTZ NOT NULL,
    server_synced_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (user_id, entitlement_id)
);

CREATE TABLE hardware_loans (
    asset_qr VARCHAR(64) PRIMARY KEY,
    category TEXT NOT NULL,
    checked_out_to UUID REFERENCES users(id),
    checked_out_at TIMESTAMPTZ,
    returned_at TIMESTAMPTZ
);

CREATE TABLE mentor_tickets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    team_id UUID REFERENCES teams(id),
    location_label VARCHAR(64) NOT NULL,
    svg_x REAL NOT NULL,
    svg_y REAL NOT NULL,
    tags TEXT[] NOT NULL,
    description TEXT NOT NULL,
    status VARCHAR(24) DEFAULT 'OPEN', -- OPEN, CLAIMED, RESOLVED, CANCELED
    claimed_by UUID REFERENCES users(id),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    claimed_at TIMESTAMPTZ,
    resolved_at TIMESTAMPTZ
);

CREATE TABLE notification_dispatches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id),
    event_type VARCHAR(48) NOT NULL,
    priority SMALLINT NOT NULL, -- 1 = critical (push -> SMS -> email)
    push_sent_at TIMESTAMPTZ,
    push_acked_at TIMESTAMPTZ,
    sms_triggered_at TIMESTAMPTZ,
    sms_status VARCHAR(24), -- 'DELIVERED', 'FAILED'
    email_triggered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE judging_queue (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    room_id VARCHAR(32) REFERENCES presentation_rooms(room_id),
    team_id UUID REFERENCES teams(id),
    track VARCHAR(48) NOT NULL,
    seq_index INT NOT NULL,
    status VARCHAR(20) DEFAULT 'QUEUED', -- QUEUED, IN_THE_HOLE, ON_DECK, PRESENTING, DONE, SKIPPED
    raw_score DOUBLE PRECISION,
    prev_team_id UUID REFERENCES teams(id),
    beat_prev_team BOOLEAN,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE (room_id, seq_index) DEFERRABLE INITIALLY DEFERRED
);
