-- Auth tables. Runs after Kenny's 0001 (design doc §5), which creates `users`.
-- Move this file into his migrations folder, with his numbering, when his stack lands.

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
