-- First migration: accounts and sessions. Kenny's design-doc §5 schema comes after this
-- and adds its columns to `users` with ALTER TABLE.

-- Only what sign-in needs. Emails are stored only once verified, lowercased.
-- Someone who signs in by SMS may have no email, and the reverse.
CREATE TABLE users (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email      TEXT UNIQUE CHECK (email = lower(email)),
    phone_e164 TEXT UNIQUE CHECK (phone_e164 ~ '^\+[1-9][0-9]{1,14}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (email IS NOT NULL OR phone_e164 IS NOT NULL)
);

-- Roles a user account holds. JUDGE_ROOM is not here on purpose: it only ever
-- exists inside a session from the judge QR exchange, tied to one room.
CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role    TEXT NOT NULL CHECK (role IN ('HACKER', 'VOLUNTEER', 'MENTOR', 'ADMIN')),
    PRIMARY KEY (user_id, role)
);

-- One row per signed-in device. `id` is the JWT's `sid` claim.
CREATE TABLE sessions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    CHECK (expires_at > created_at)
);
-- "Sign out everywhere" and deleting a user look sessions up by user.
CREATE INDEX sessions_user_id ON sessions (user_id);
