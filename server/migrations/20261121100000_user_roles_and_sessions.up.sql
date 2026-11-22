-- Auth PR 1: account roles and sessions. Runs after the core schema (20261101…), which
-- creates `users` (verified lowercase email and/or E.164 phone, at least one of them).

-- Roles a user account holds. JUDGE_ROOM is not here on purpose: it only ever
-- exists inside a session from the judge QR exchange, tied to one room.
CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role    TEXT NOT NULL CHECK (role IN ('HACKER', 'VOLUNTEER', 'MENTOR', 'ADMIN')),
    PRIMARY KEY (user_id, role)
);

-- One row per signed-in device. `id` is the JWT's `sid` claim.
-- A user with sessions can't be deleted (RESTRICT, not CASCADE): a session row
-- that vanishes is never put in the Redis revocation cache, so its token would
-- keep working until it expires. Revoke the user's sessions and wait for them to
-- expire before deleting the rows and the user.
CREATE TABLE sessions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    CHECK (expires_at > created_at)
);
-- "Sign out everywhere" and deleting a user look sessions up by user.
CREATE INDEX sessions_user_id ON sessions (user_id);
