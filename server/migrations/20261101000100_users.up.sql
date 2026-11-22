-- Design doc §5 users, changed so sign-in can create an account before the application
-- is filled in: only email or phone is required. The face embedding moved to its own
-- table (face_embeddings) so deleting it or the user is one row, with its own index.
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Verified addresses only, lowercased. An SMS-only account has no email, and the reverse.
    email TEXT UNIQUE CHECK (email = lower(email)),
    phone_e164 TEXT UNIQUE CHECK (phone_e164 ~ '^\+[1-9][0-9]{1,14}$'),
    -- Filled in by the application, so empty right after a first sign-in.
    full_name TEXT,
    school TEXT,
    dietary_flags SMALLINT NOT NULL DEFAULT 0,
    qr_hmac_sig BYTEA UNIQUE,
    checked_in_at TIMESTAMPTZ,
    collateral_held BOOLEAN NOT NULL DEFAULT FALSE,
    travel_reimbursement_cap_cents INT NOT NULL DEFAULT 0 CHECK (travel_reimbursement_cap_cents >= 0),
    apns_fcm_tokens TEXT[] NOT NULL DEFAULT '{}',
    webpush_sub JSONB,
    live_activity_token TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT users_email_or_phone CHECK (email IS NOT NULL OR phone_e164 IS NOT NULL)
);
CREATE TRIGGER users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
