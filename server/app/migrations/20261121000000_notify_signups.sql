-- Addresses from the landing page's "notify me" form, for the
-- "registration is open" email.
CREATE TABLE notify_signups (
    email TEXT PRIMARY KEY, -- trimmed and lowercased
    -- Client address from the reverse proxy's X-Real-IP, for the per-address limit.
    ip TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX notify_signups_ip_created_at ON notify_signups (ip, created_at);
