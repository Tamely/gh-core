# Ricky: handoff

_Last updated: 2026-11-22_

## ⚠️ Core schema PR: takes priority for migrations (2026-11-21)
**This set of changes is the precedent for the database.** Its migrations run **first**, before every other gh-core migration. Any migration whose number conflicts with it (sorts before it, uses the same number, or creates a table it creates) **changes its number and its SQL**, not this one.

**What it is:** branch `ricky/schema` (worktree `.claude/worktrees/schema` in the fork clone), PR: [Tamely/gh-core#13](https://github.com/Tamely/gh-core/pull/13). It covers the schema ticket: the main PostgreSQL 16 + pgvector database, with reversible sqlx migrations for every core table.
- `server/migrations/`: 11 reversible `up.sql` / `down.sql` pairs, versions **`20261101000000` to `20261101001000`**, one logical change each: extension and shared `set_updated_at()` trigger, `users`, `teams` + `team_members`, `presentation_rooms`, `entitlements` + `entitlement_scans`, `hardware_assets` + `hardware_loans`, `sponsor_keys`, `mentor_tickets`, `notification_dispatches`, `judging_queue`, `face_embeddings` + `nearest_faces()`.
- `server/schema.sql`: the committed `pg_dump --schema-only` snapshot.
- `server/seeds/seed.sql`: synthetic data (1,000 users, 250 teams, 25 rooms, 5 entitlements, 5,000 scans, 500 assets, 600 sponsor keys, 1,000 random unit 512-d vectors). Not a migration.
- `server/db-tests/run.sh` (+ `acceptance.sql`, `claim.pgbench`, `README.md`) checks every acceptance criterion; CI runs it in `.github/workflows/schema.yml`. Needs Docker and sqlx-cli (`brew install sqlx-cli`). `run.sh --update` rewrites `schema.sql` after a migration change.
- `server/docker-compose.yml`: same file as the auth PRs' (identical, so it merges cleanly).

**Migration numbering rule (everyone):** use timestamps (`sqlx migrate add -r <name>` makes a reversible pair with the current time). Nothing may sort at or before `20261101001000`. Tables this PR creates are changed later with `ALTER TABLE`, never created again. Every migration is an `up.sql` / `down.sql` pair, because the CI check runs everything down and up again.

**Review fixes (2026-11-22):** a three-part review of #13 (migrations, tests/CI, redundancy) found no blockers; these were fixed in place (the core migrations aren't merged anywhere yet):
- `presentation_rooms`: the judging window is both ends or neither. A half-set window used to pass the check.
- `mentor_tickets`: `CLAIMED` needs `claimed_by` + `claimed_at`, `RESOLVED` needs `resolved_at`. A trigger reopens a claimed ticket when its mentor is unset, including when the mentor's account is deleted (`ON DELETE SET NULL`); before, it stayed `CLAIMED` with no owner forever.
- `users.email`: also trimmed and non-empty.
- `nearest_faces` is `STRICT`: `k = NULL` returns nothing instead of every row. `search_path` is deliberately not pinned, because the auth tests run the migrations in their own schemas.
- The extensions down drops `vector` only when no table anywhere still uses it, so one test schema's down can't break another's.
- Tests: the "duplicate fails at COMMIT, not before" check could pass when the UPDATE itself failed; fixed. The thresholded recall check now uses queries that straddle 0.68 (about half match) and fails if `nearest_faces` returns anything past the threshold. New check for the room window, ticket rules and the reopen trigger. Each was sabotage-checked.
- `docker-compose.yml`: Postgres pinned to `pgvector/pgvector:0.8.7-pg16` (exact dumps are compared) and the health check uses TCP. **Auth PRs: merge `ricky/schema` again** to pick up the changed core migrations and compose file (no conflicts expected; they don't edit those files).
- CI: `permissions: contents: read`; pushes to main run the check unfiltered.
- Removed redundant test code (repeat schema diff, pgbench grep, DISTINCT counts, double VERBOSITY, default Redis/psql settings) and the README's copy of the check list.
- **Dev databases that ran the earlier core migrations:** `docker compose -f server/docker-compose.yml down -v` (sqlx sees the changed checksums).
- Not changed: same team queued twice in a room (may be intended; add `UNIQUE (room_id, team_id)` if not), and ids in the seed are still random UUIDs (the comment now says so).

**Simplification pass (2026-11-22):** `entitlements` and `hardware_assets` no longer have `updated_at` or its trigger (lookup tables loaded before the event). `run.sh` drops the claim timer (`lock_timeout` already fails a claim that waits; checked by removing `SKIP LOCKED`) and a redundant rollback re-read. The migrations changed again, so dev databases and the droplet's `ghcore` need resetting (done on the droplet); the auth branches pick it up on their next `ricky/schema` merge.

**What had to change elsewhere because of this:**
- **Done (2026-11-21): the auth PRs are renumbered** and each one merges `ricky/schema`, so their tests run on top of the core schema:
  - **Auth PR 1 (#7)** [`b0dfd87`](https://github.com/liangricky7/gh-core/commit/b0dfd87): `0001_users_and_auth.sql` → `20261121100000_user_roles_and_sessions.up/down.sql`, without `CREATE TABLE users` (the core `users` has the same email/phone rules). Keeps `user_roles` and `sessions`. Tests now apply every `*.up.sql` in order via `tests/common::migrate` and install `vector` in `public` once (advisory lock), with `search_path = <test schema>,public`.
  - **Auth PR 2 (#8)** [`ae72a38`](https://github.com/liangricky7/gh-core/commit/ae72a38) and **Auth PR 3 (#10)** [`05b1601`](https://github.com/liangricky7/gh-core/commit/05b1601): merged the above; no migrations of their own (the `0003` reservation is dropped).
  - **Auth PR 4 (#12)** [`b4cb618`](https://github.com/liangricky7/gh-core/commit/b4cb618): `0002_judge_rooms.sql` → `20261121100100_judge_rooms.up/down.sql`, without `CREATE TABLE presentation_rooms`. Keeps `sessions.room_id` and `judge_exchange_audit`. **`set_room` (and `judge-admin room`) now only moves the window of a room that already exists** (rooms need track, floor and map position, which event setup owns) and returns `RowNotFound` / "no room X: add it to presentation_rooms first" otherwise. Tests insert full rooms first. 33 tests + Clippy pass; judge exchange p99 24ms locally in a debug build (optimized build is the budget one).
  - **Auth PR 5 (#11)**: client only, no change needed.
- **Kenny**: delete `server/app/migrations/20261117000000_initial_schema.sql`; this PR replaces it, including his fixes (`gen_random_uuid()`, deferred queue uniqueness). Move `notify_signups` to `server/migrations/` as a reversible pair (its `20261121000000` already sorts after the core schema). Point the runner at the shared folder: `sqlx::migrate!("../migrations")`.
- **Dev databases** that already applied `0001` / `0002`: `docker compose -f server/docker-compose.yml down -v` (sqlx would see the history change).

**Decisions (and why):**
- **No `uuid-ossp`**: ids use `gen_random_uuid()`. Natural keys where they exist: `room_id`, `entitlement_id`, `asset_qr`.
- **pgvector 0.8+ is checked by the first migration**: it fails with a clear message on an older version, since iterative scans need 0.8.
- **`users` profile columns are nullable** (`full_name`, `school`, `qr_hmac_sig`): a first sign-in has none of them. Email or phone is required.
- **The face embedding has its own table** (`face_embeddings`, keyed by `user_id`, `ON DELETE CASCADE`), not a `users` column, so deleting a user deletes it and the HNSW index is on a small table.
- **ON DELETE rule:** rows *about* a user cascade (scans, team membership, notifications, face embedding). A user as the *actor* (`scanned_by`, `checked_out_by`, `claimed_by`) becomes NULL. **RESTRICT** where a deletion would lose track of property or access: `hardware_loans.user_id` (an unreturned loan would vanish), `sponsor_keys.claimed_by_team` (a seen key would go back into the pool), `entitlement_scans.entitlement_id`, `hardware_loans.asset_qr`. A check fails if any foreign key is left on the default NO ACTION.
- **Every foreign key column leads an index**; the test checks this from the catalog.
- **Statuses are `CHECK (… IN …)`**, not enums.
- **`updated_at` on every mutable table**, kept current by `set_updated_at()`.
- **Hardware is assets + loans**: §5 had one row per asset, so a return erased the loan. A partial unique index allows at most one open loan per asset.
- **`judging_queue (room_id, seq_index)` is `DEFERRABLE INITIALLY DEFERRED`**, checked at COMMIT, so a swap works in one transaction. **`entitlement_scans`' primary key stays non-deferrable**, because ON CONFLICT can't use a deferrable key.
- **`sponsor_keys`**: partial index `(sponsor_slug, id) WHERE claimed_by_team IS NULL` for the design doc's `FOR UPDATE SKIP LOCKED` claim, and `UNIQUE (claimed_by_team, sponsor_slug)`: one key per sponsor per team.
- **Vector search goes through `nearest_faces(query, k)`**: top k by `ORDER BY embedding <=> $1 LIMIT k` in a materialized CTE, then `distance <= 0.32`. The function sets `hnsw.iterative_scan = relaxed_order`, `hnsw.ef_search = 100` and `enable_seqscan = off`. With only ~1,000 rows the planner otherwise picks a sequential scan, and the default `ef_search` of 40 is below `k = 50`. The settings live on the function, not `ALTER DATABASE`, so they're in `schema.sql` and reversible. Raw queries must set the same three.
- **The snapshot drops `--` comments, blank-line runs and pg_dump's per-run `\restrict` key** (new in Postgres 16.10), so runs and minor versions compare equal. `_sqlx_migrations` is left out of it.
- **sqlx-cli 0.9.1** in CI and locally.

**Results (2026-11-21):**
- Local (Docker on Ricky's Mac): every check passes. Duplicate check p99 0.02ms; vector search p99 ~1ms (median 0.7), HNSW index used, top-50 recall 0.992, threshold recall 1.0; 50 concurrent claims → 50 different keys in ~1s with `lock_timeout` 100ms. Sabotage-checked: removing SKIP LOCKED, the deferral, the open-loan index, a `DROP` in a down file, an FK index, or an `ON DELETE` each makes it fail.
- **CI**: the workflow passes ([run](https://github.com/liangricky7/gh-core/actions/runs/44985439005), 1m12s, on a temporary draft PR in Ricky's fork, liangricky7/gh-core#2, to close once upstream runs). **On Tamely/gh-core every run is "action_required"**: GitHub holds workflows from fork PRs until a maintainer clicks *Approve and run* on #13 (and #7, #8, #10, #12).
- **Staging** (droplet Postgres 16.16, pgvector 0.8.7, in a throwaway database `ghcore_schema_check`, dropped after each run): all 11 migrations up, seed, every acceptance check, all down leaving nothing behind, **queue swap + commit-time 23505, and 50 concurrent claims → 50 different keys in ~1s, none blocked**. Migrations were applied file by file with `psql` (no sqlx on the droplet; sqlx itself is tested locally and in CI).
- **Vector p99 on staging: passes after slowing the staging health checks (2026-11-21).** Before: p99 swung 4.6–21ms (216 of 1,000 queries ≥ 5ms), and `ef_search` 40/50/64/100 made no difference. Cause: the droplet is 1 shared vCPU, and the staging Postgres and Redis health checks each ran a `docker exec` every 2s, pausing Postgres about once a second. **Fix (Ricky approved):** `/opt/gh-core/staging/compose.yml` health checks are now `interval: 30s, start_period: 30s, start_interval: 2s, retries: 3` (old file kept as `compose.yml.bak-2s-health`). After: **3 of 3 fresh runs pass, p99 3.24 / 2.66 / 2.51ms** (median ~2ms, recall 0.993), and over 30 extra passes only 3 of 3,000 queries hit 5ms (worst pass p99 5.31ms). The timed pass also runs after a full untimed warm-up pass now.
- **Rule for the droplet (Kenny / whoever writes the prod compose):** health checks at 30s or slower on a 1-vCPU box. Still resize before the January mock run; there's little headroom (median 2ms vs 5ms budget, plus 2–14% CPU steal).

**Open:**
- The ticket's table list didn't come through when pasted; the tables come from §5 plus everything the acceptance criteria and other handoffs need. Check it against the ticket.
- Josh's applications, team alliance and points ledger tables are **not** in this ticket and still have no owner.

## Owns
**Auth: authentication and authorization for gh-core.** Sign-in, sessions, judge room access, roles. Built in **Rust** as the `server/auth` crate, plugged into Kenny's Axum server in `server/` (see `handoffs/kenny.md`, PR #4).

**Database hosting** (assigned 2026-11-20). **Dev: local Docker** via `server/docker-compose.yml` (`pgvector/pgvector:pg16` + Redis 7, both on `127.0.0.1`, dev-only password; Kenny merges his services in). **QA/prod: the DigitalOcean droplet** (1 vCPU / 1GB for now, resize to ≥ 2 vCPU / 4GB before the January mock run; keep CPU/RAM-only resize so it can shrink back). **Droplet set up (2026-11-21):** Ubuntu 24.04, reserved IP `161.35.242.244`, DNS `app.geauxhack.org` → that IP (Namecheap A record; `@` and `www` are still Namecheap parking). Done: ufw on (only 22, 80, 443), 2 GB swap (`swappiness=10`), Docker 29 + Compose, Caddy in `/opt/gh-core/caddy` with a Let's Encrypt cert (auto-renews) serving a placeholder plus `/healthz`. SSH is key-only, as root for now. **Docker-published ports skip ufw**, so Postgres and Redis must be published on `127.0.0.1` only (or not at all). **Staging on the droplet (2026-11-21, temporary):** `/opt/gh-core/staging` runs the auth API, Postgres and Redis in Docker Compose. No ports are published, so only Caddy reaches the API. The API is a merge of #10 and #12 made only for staging (not pushed), plus a short staging-only `main` that isn't in the repo. It's a release build. Its signing key, `JUDGE_KEY` and Postgres password were generated on the droplet and live in `staging/.env` (mode 600) with the Google and GitHub OAuth secrets. Email and SMS codes return 503 there (no Postmark or Twilio). It adds one stand-in route, `GET /api/v1/rooms/{room_id}/scores` (`RequireRoom`). Caddy routes `/api/*` and Apple's POST `/auth/callback` to the API, and serves two stand-in pages from `/opt/gh-core/site`: https://app.geauxhack.org/oauth-test and `/judge/*`. It still keeps no request logs. Test room `STAGING-1` (window ends 2026-11-24 23:59 Central); its placard is in `/opt/gh-core/placards` and is a credential. The old placeholder Caddy files are saved as `*.placeholder`. `https://app.geauxhack.org/auth/callback` is registered with Google and GitHub. **Wipe all of this after the last staging test, before #7, #10 and #12 merge**, once Josh (auth is his) says testing is done. The steps are in the "Temporary staging" note in #7, #10 and #12 (staging `down -v`, restore Caddy's `*.placeholder` files, delete `staging/`, `site/` and `placards/`). Leave `/opt/gh-core/db` alone. Kenny's server and Josh's app replace it. **gh-core database on the droplet (2026-11-22):** `/opt/gh-core/db` (compose project `ghcore-db`), separate from the auth staging stack. `pgvector/pgvector:0.8.7-pg16`, database `ghcore`, published on `127.0.0.1:5432` only, 30s health checks. All 11 core migrations applied with `sqlx migrate run` through the tunnel (so `_sqlx_migrations` is filled and the backend runner will see them as done). No seed data. Passwords in `/opt/gh-core/db/.env` (mode 600): `POSTGRES_PASSWORD` (superuser, migrations only) and `APP_PASSWORD`.
- **`ghcore_app`** (`roles.sql`, rerunnable): the role the app connects as. It can read and write rows but not change the schema (tested: DROP/CREATE TABLE and editing `_sqlx_migrations` are refused). Default privileges give it the same rights on tables later migrations create, as long as migrations run as `postgres`. **Kenny:** connect the server as `ghcore_app`, run migrations as `postgres`.
- **SSH tunnel:** login `ghtunnel` can only forward to `127.0.0.1:5432` (no shell, no other ports; both tested). Only Ricky's laptop key is on it. To add a teammate, append their public key to `/home/ghtunnel/.ssh/authorized_keys` with the same `restrict,port-forwarding,permitopen="127.0.0.1:5432",command="/bin/false"` prefix. Use: `ssh -N -L 15432:127.0.0.1:5432 ghtunnel@app.geauxhack.org`, then connect to `127.0.0.1:15432`. They also need a database password; give out `APP_PASSWORD`, not the superuser one.
- **Backups:** `/etc/cron.d/ghcore-backup` runs `/opt/gh-core/db/backup.sh` at 09:00 UTC (03:00 Central): `pg_dump -Fc` to `/opt/gh-core/backups`, 14 days kept, log in `/var/log/ghcore-backup.log`. **Not off the droplet yet:** create a private Spaces bucket and access key, copy `spaces.env.example` to `spaces.env` (mode 600) and fill it in; `backup.sh` then uploads with rclone (installed). During the event, switch the cron line to hourly.
- **Restore test:** `/opt/gh-core/db/restore-test.sh [dump]` restores into a scratch database and compares columns, constraints, indexes, triggers, functions, extensions and row counts with the live one (pg_dump text can't be compared: it has a random `\restrict` line, and Postgres re-words `IN (...)` checks after a restore). Passed on the first backup; an empty dump is reported as a mismatch.
- **Still open (hosting):** Spaces upload (keys above), monitoring/alerts (a failed backup is only in the log today), resize to ≥ 2 vCPU / 4GB before the January mock run. Root's `authorized_keys` has an extra ed25519 key (`SHA256:e+ZCMG…`, same comment as Ricky's laptop key) and a `PASTE_KEY_HERE` line; left alone until Ricky says whose the key is.

## Status
**Auth is Josh's (2026-11-22).** Its PRs: [#7](https://github.com/Tamely/gh-core/pull/7) tokens and sessions, [#8](https://github.com/Tamely/gh-core/pull/8) email/SMS codes, [#10](https://github.com/Tamely/gh-core/pull/10) OAuth, [#12](https://github.com/Tamely/gh-core/pull/12) judge rooms, [#11](https://github.com/Tamely/gh-core/pull/11) client. Each PR description has its acceptance table, test results and staging notes; `server/auth/*.md` has the details. Merge order: #7 → #8 → #10 and #12 in either order (the second conflicts in `AuthState`: keep both sides). #7, #8, #10 and #12 still need `ricky/schema` merged in again (see the top of this file). Testing trap: two checkouts sharing one `CARGO_TARGET_DIR` volume can run each other's stale test binaries; give each its own.

## Decisions
- **Rust, not Go** (2026-11-20). The spec (§3) allows either for the single API binary; the team chose Rust and Kenny's Axum server already exists. Auth is a crate in his workspace, not a second server.
- **Production domain `app.geauxhack.org`** (2026-11-21, confirmed by Ricky), matching `handoffs/josh.md`. Auth code, tests and docs use `.org`; the server sets the real origin in `AuthState.origin`.
- **JWTs are signed and checked with `ed25519-dalek` directly**, not a JWT library. Verify accepts only `alg: EdDSA` whatever the header says, uses `verify_strict`, and rejects padded or non-canonical base64. `jsonwebtoken` is a test-only dependency that proves other software can verify our tokens from the JWKS.
- **`kid` is the RFC 7638 thumbprint** of the public key, so there's no key-ID config to get wrong.
- **Key config:** current private key as PKCS#8 PEM (`openssl genpkey -algorithm ed25519`); old keys as comma-separated base64url public keys (their JWKS `x`). Env var names are picked when it's wired into Kenny's config.
- **`verify` takes `now` as an argument** instead of reading the clock, so tests use a fake time.
- **`JUDGE_ROOM` can't be stored in `user_roles`.** It only exists in sessions from the judge exchange, tied to a room, so no account can be given it by accident.
- **JWKS is cached for 5 minutes.** After a key rotation, outside verifiers can miss the new key for up to 5 minutes. Only matters if something other than our server verifies tokens.
- **Development uses local Docker Postgres/Redis; the droplet is QA/prod only** (2026-11-20). No heavy traffic in the dev phase, so the droplet stays small until the mock run.
- **In QA/prod, the API server, Postgres and Redis all run on the same DigitalOcean droplet** (2026-11-20). Postgres and Redis are never reachable from the internet; the API connects over `127.0.0.1` (Kenny's note: not `localhost`). Avoids sending every query across the internet, which a separate host (e.g. AWS) would.
- **Revocation when Redis is down:** fall back to Postgres; if both are down, 503, never allow. Public routes (health checks, JWKS) skip the check, so they keep working.
- **Redis losing its data is detected with a `gh:revoked:ready` marker** set after a reload. Redis runs without persistence, so a restart empties it; without the marker, revoked tokens would pass until our server restarted.
- **Logout/revoke only fails (503) if Postgres fails.** Once Postgres says revoked, it's done: if Redis can't take the key, `gh:revoked:ready` is dropped (lookups go to Postgres) and the key is retried in the background every second until it lands. A revoke no longer depends on the user retrying after a 503. Remaining gap: if Redis can take neither the key nor the drop, the session can pass for as long as the Redis blip lasts.
- **Redis must run with `maxmemory-policy noeviction`** (Redis's default; the compose file and the droplet must keep it). Revoked keys have a TTL and the marker doesn't, so a `volatile-*` policy under memory pressure would drop revoked keys while keeping the marker, letting revoked tokens back in silently.
- **Reload runs twice around setting `gh:revoked:ready`**, so a revoke that lands mid-reload isn't lost, and **only one request reloads at a time** (the rest ask Postgres meanwhile). Neither race has an automated test: it needs a Redis that fails on cue.
- **Renewal re-reads roles** from `user_roles`, so a removed role is gone within 7 days at most. For an immediate change, also revoke the user's sessions.
- **`auth-dev` only runs against the local compose database** (127.0.0.1, localhost or host.docker.internal on port 5432, parsed by sqlx so `?host=` can't sneak past; keep SSH tunnels off 5432) and applies migrations with sqlx's runner (`_sqlx_migrations`), the same bookkeeping the server will use.
- **Public routes always see `Caller::Anonymous`**, so a public handler can't accidentally rely on who's calling.
- **Added a `SignedIn` policy** (any role, or none) for `/me` and logout, so a brand-new account with no roles can still sign out.
- **Judge sessions never renew**; they end with judging.
- **Deleting a user with sessions is refused** (RESTRICT, 2026-11-21), same as rooms: revoke, wait for expiry, delete the session rows, then the user. `user_roles` still cascades.
- **PR 4 open questions, answered with the doc's suggestions** (2026-11-21): the judging window is per room (`judging_starts_at` / `judging_ends_at`; no window = the placard doesn't work); a judge token's `sub` is `judge:<sid>`, so it can't pass for a user id; **rotating a placard also revokes every session it gave out** (a lost phone is cut off by the same command); **failed exchanges are audited too**, with the reason in `outcome` (the caller still only sees 401); the token stays in the URL **path** (`/judge/<token>`) as the ticket says, so the host's access log must redact `/judge/*` (the fragment would avoid that but changes Josh's route and deep link).
- **Placard layout:** room id, then the version as a big-endian u32, then the 32-byte HMAC, base64url without padding. Room ids are checked to be letters, digits, `-` and `_` (the table and the decoder), so a forged room id is safe to write to the audit log and to use as a file name. The room id is readable in the token (signed, not encrypted).
- **The exchange checks the version, not the stored hash.** `token_hash` is stored on every rotate as the ticket asks (only version + hash, never the token), but checking both would make one of the two checks untestable.
- **Session and audit row are one SQL statement**, so a session never exists without its audit row; one read plus one write per exchange.
- **`K_judge` is `JUDGE_KEY`**: 32 random bytes as base64 or base64url (`openssl rand 32 | base64`). A test key in tests, a fixed dev key in `auth-dev` unless `JUDGE_KEY` is set.
- **A rejected exchange costs one audit insert.** No rate limit on the exchange (guessing a 256-bit HMAC is hopeless); add a per-IP limit if junk ever floods the table.
- **Revocation doesn't close open WebSockets** (checked on connect only). Fine for now; re-check per event if it matters.
- **Router wiring rules for Kenny:** call `.layer(auth::layer)` **last** (axum's `Router::layer` only wraps routes that already exist, so a route added after it skips auth), and use `nest(Router)`, not `nest_service` (nested services have no matched route, so they'd all be 403).
- **Deferred (Ricky, 2026-11-21):** live SMS (#8) waits until later; Apple sign-in (#10) waits until later. Google and GitHub already passed a live local sign-in; they get rechecked on the deployed HTTPS site, including Safari.

- **Known small gaps, left on purpose:** unknown paths and wrong methods return 403 instead of 404/405 (safe, just less specific); CORS, if ever added, must sit outside the auth layer; the matrix test only checks `auth::policies()`, so Kenny's own routes need rows in a matrix of his; `gh:revoked:ready` is one shared Redis key, so two test runs at once on the same Redis can interfere.
- **`auth-dev` uses a fixed dev signing key**, so its tokens only work on `auth-dev serve`, never on a real server.
- **Secure-storage plugin: `@aparajita/capacitor-secure-storage` 8.0.1** (decided 2026-11-21, PR 5). It supports Capacitor 8. On iOS it uses the Keychain with `afterFirstUnlockThisDeviceOnly`, so the token is readable while the phone is locked and never goes to iCloud or a backup. On Android it uses AES-GCM with a Keystore key. It runs on native only, because its web fallback is plain localStorage.
- **SMS codes:** Twilio Verify generates and checks SMS codes itself, so the hash / expiry / attempt rules are built by us for **email** codes only (and for SMS in dev, where codes are printed).
- **Email via Postmark** (Ricky, 2026-11-21). Real Postmark and Twilio Verify senders are in PR 2, tested against a fake local server; a live try needs the ops accounts.
- **New accounts get no roles** (Ricky, 2026-11-21). An admin or the application flow grants them.
- **Codes live in Redis** (`gh:code:{email|phone}:{addr}`, a hash with the SHA-256, expiry and try count), 10 minutes, 5 tries, single use. Lost on a Redis restart: users just ask for a new code. The try is counted before comparing, so parallel guesses get no extra tries, and only the request whose compare-and-delete consumes that same hash/expiry signs in (a resend cannot be consumed by a stale check). Out of tries, the code is left to expire rather than deleted (deleting made a correct parallel guess lose).
- **Rate limits** (fixed windows on Redis `gh:rl:*`): per address 1 code a minute, 5 an hour and 20 guesses a day; per IP 300 sends and 600 checks an hour. The per-IP limits are high because everyone at the venue shares one Wi-Fi IP; the per-address limits are the real guard (without the daily cap, 5 codes an hour × 5 tries let one account be guessed at 600 times a day). An attacker can lock a victim's address out of codes for up to an hour, and out of guesses for the rest of the day; accepted. If check-in outgrows the per-IP limits, allow-list the venue IP.
- **Client IP = the last `X-Forwarded-For` entry** (the one our proxy adds), else the TCP peer. The proxy must **append** to `X-Forwarded-For` (Caddy does by default; nginx needs `$proxy_add_x_forwarded_for`). With neither, the code routes return 500 instead of putting every caller in one shared limit (see Needs from others for Kenny).
- **Emails must be plain ASCII `local@domain`**: no display names, lists, quotes or comments, since Postmark's `To` would read `x<attacker@evil.com>` as a different recipient than the address we store as verified. International (non-ASCII) addresses are refused for now.
- **A code is stored only after the provider accepts the email**, so a failed resend leaves the earlier code working. If the provider times out but still delivers, that code won't work and the user asks again.
- **Provider answers:** a permanent refusal (Twilio 400/403, Postmark 422 with ErrorCode 300/406) is 400 so the user fixes the address; Postmark's or Twilio's 429 is 429; anything else, including other Postmark 422s (our own setup), is 503. A Twilio 200 we can't read is 503, not "wrong code", because Twilio may already have used the code up.
- **Cookie vs token by `Origin`:** our web origin gets only the cookie (the token never reaches page scripts); anything else gets the token in the body. A page on another site gets nothing usable, which also blocks "log the victim into the attacker's account".
- **The log sender exists only in debug builds** (`#[cfg(debug_assertions)]`), so production can't be set up to log codes; a release `auth-dev` refuses to run.
- **reqwest 0.12 with rustls + webpki roots**, not 0.13: 0.13 reads the system's CA list, which Kenny's `FROM scratch` image doesn't have.
- **Code hash is unsalted SHA-256** of address and code. Reading Redis already means owning the server; switch to HMAC with a secret if that changes.

## Provides to others
Built in PR 1 ([Tamely/gh-core#7](https://github.com/Tamely/gh-core/pull/7)) and PR 2 ([Tamely/gh-core#8](https://github.com/Tamely/gh-core/pull/8)), both in review; shapes may still change before they merge:
- `auth::Keys` (`load`, `issue`, `verify`), `auth::Claims`, `auth::Role`.
- `auth::AuthState { keys, origin, now, sessions: Sessions::new(db, redis), policies, codes: Codes::from_env()? }` (PR 2 added `codes`; reads POSTMARK_SERVER_TOKEN, POSTMARK_FROM, TWILIO_ACCOUNT_SID, TWILIO_AUTH_TOKEN, TWILIO_VERIFY_SERVICE_SID, and a release build errors if any is missing), `auth::layer` (put on the whole router with `Router::layer`, as the last call), handlers read `Extension<auth::Caller>`.
- `auth::routes(state)`: `GET /.well-known/jwks.json`, `GET /api/v1/me` (`{id, roles, exp, roomId?}`), `POST /api/v1/auth/logout` (204, clears the cookie), `POST /api/v1/admin/users/{id}/revoke-sessions` (admin).
- Policy map `auth::policies()` keyed `"METHOD /path"` with `Public` / `SignedIn` / `RequireRole(...)` / `RequireRoom` (uses the `{room_id}` path parameter). A route with no policy returns `403`. **Kenny:** new routes need an entry, or they 403.
- Session cookie `__Host-session` (web), `Authorization: Bearer <jwt>` (native). **Josh:** when a response has an `x-session-token` header, native stores that token in place of the old one (sliding renewal); web gets a new cookie automatically.
- WebSockets: `auth::ws_caller(&state, &headers)` on upgrade, `auth::ws_first_message(&state, text)` for native's first message, `auth::may_subscribe(&caller, topic)` per subscribe. **Josh:** native sends the bare token as the first WebSocket message.
- Revocation keys in Redis: `gh:revoked:{sid}` and `gh:revoked:ready`.
- PR 2: `POST /api/v1/auth/code` and `POST /api/v1/auth/code/verify` (see Status). **Josh:** sign-in screen sends the email or E.164 phone, then the code; native keeps `token` from the verify response, web gets the cookie.
- PR 4 (not opened yet): `POST /api/v1/judge/exchange` (see Status). `AuthState` gains `judge: JudgeKey::load(&JUDGE_KEY)?`. `auth::set_room` / `auth::rotate` back the `judge-admin` tool. **Kenny:** judging routes use a `{room_id}` path parameter with `Policy::RequireRoom`. **Josh:** the judge page posts the token from `/judge/<token>` to the exchange, then removes it with `history.replaceState`; native keeps `token` from the body.

**PR 5 (client session) is a draft PR:** [Tamely/gh-core#11](https://github.com/Tamely/gh-core/pull/11) from `liangricky7:ricky/auth-client`, based on Josh's `pr-1.4-capacitor` (so the diff is only auth). `apps/client/src/auth/` gives one `auth` object:
- `startup()`: offline, no network call; returns signed-in, expired or signed-out
- `signIn(path, body)`: code, OAuth or judge exchange; a rejection leaves the current session alone
- `api()`, `refresh()` (from `/me`) and `signOut()`

Web relies on the HttpOnly cookie. Native keeps the token in the Keychain / Keystore and sends a Bearer header.

**Native API calls (fixed 2026-11-21, commit `0fcc4ed`, local only).** The native app runs at `capacitor://localhost`, so a relative `/api` call never reaches the server. Native now calls `API_ORIGIN` through CapacitorHttp:
- `API_ORIGIN` comes from `VITE_API_ORIGIN` and defaults to `https://app.geauxhack.org`.
- CapacitorHttp means no CORS and no Origin header, so the server returns the token in the body.
- `x-session-token` is readable.
- The server needs no CORS change.

**Stand-in for Josh's screens.** `/dev/auth` runs every auth call. It only exists in dev builds, or in a build with `VITE_AUTH_STANDIN=1`. `pnpm dev` proxies `/api` to `auth-dev serve`.

**Tested 2026-11-21** against `auth-dev serve`. auth-dev used its own `standin` database, because the old dev database has a pre-audit 0001 migration.
- Web, in the browser:
  - code sign-in (the cookie isn't visible to JavaScript)
  - /me returns 200
  - a reload starts signed in with no network call
  - sign-out, after which /me returns 401
  - a judge placard gives `JUDGE_ROOM` with its `roomId`, and `exp` = the end of judging
  - a tampered placard returns 401 and leaves the session as it was
- Native, two ways:
  - curl without an Origin header: the token comes in the body with no cookie, then Bearer → /me 200 → logout 204 → 401
  - unit tests of the CapacitorHttp adapter

`pnpm check` passes with 37 tests.

**Not done:** a simulator or device run. This Mac has only the Command Line Tools: no full Xcode, no Android SDK. Also not done: wiring into Josh's real UI.

## Needs from others
| For | What's needed | Why / blocks | By |
|---|---|---|---|
| Kenny | **Rebase onto auth once it's on `main`** (see Status for how to wire `AuthState`, `auth::layer`, `auth::routes` and the policy map; share your fred client, built with a 1s `default_command_timeout` (without one a Redis outage hangs requests instead of falling back to Postgres); `.layer(auth::layer)` goes last; use your migration runner on `server/migrations`, which already has sqlx bookkeeping from `auth-dev`) (per Ricky's senior dev, auth and Postgres land first). Your §5 migration is replaced by the core schema PR (top of this file). Merge your members/services into `server/Cargo.toml` and `server/docker-compose.yml`. Replace `auth::AllowAll` with auth's topic check. | Keeps one migration history and closes the open WebSocket | when auth is on `main` |
| Kenny | You take `POST /api/v1/notify` next; Ricky is on auth. | Answers "notify or auth next" in your handoff | — |
| Kenny | Serve with `into_make_service_with_connect_info::<SocketAddr>()` (as `auth-dev serve` does), so the sign-in code routes can see the caller's IP when no proxy header is there; without it they return 500 | Per-IP rate limits on sign-in codes (PR 2) | when you rebase onto auth |
| Kenny | **PR 4 is back with Ricky** (2026-11-21); ignore the requirements doc's "Kenny builds PR 4". | Avoids two people building it | now |
| Kenny / PR 3 / auth | **Migration numbers:** the core schema PR's migrations (`20261101000000`–`20261101001000`) run first and take priority. Everything else uses a later timestamp as a reversible pair and ALTERs core tables instead of creating them. Kenny: drop `20261117000000_initial_schema.sql` (replaced) and use `sqlx::migrate!("../migrations")`. See the top of this file. | One migration history, CI's down/up check | before your next migration |
| Josh | `handoffs/josh.md` says `/api/*` goes to "the Rust server": correct, the backend is Rust (Kenny's Axum server). No change needed. | — | — |
| ops | Google OAuth client IDs (web, iOS, Android) | OAuth PR | before OAuth PR |
| ops | GitHub OAuth app | OAuth PR | before OAuth PR |
| ops | Sign in with Apple: Services ID and private key (needs the Apple Developer enrollment Josh already asked for) | OAuth PR; App Store guideline 4.8 | before OAuth PR |
| ops | Accounts are set up (2026-11-21). Postmark email delivery/sign-in/reuse/logout passed live with an lsu.edu recipient. Await account approval for Gmail. Credentials are now in ignored `.env`. Left: complete Twilio Verify settings, confirm code length 6 / expiry 10 minutes / US geo permissions, then test a user-provided E.164 phone | Live sign-in codes (PR 2 is built and tested against a fake) | before the January mock run |
| infra | The HTTPS proxy in front of the API must **append** the client IP to `X-Forwarded-For` | Per-IP rate limits on sign-in codes; IPs in the judge audit log | when hosting exists |
| infra | `JUDGE_KEY` (K_judge, 32 random bytes) in the secrets store, next to the Ed25519 key. The proxy / static host must **redact `/judge/*` paths from access logs** and serve `/judge/*` pages with `Referrer-Policy: no-referrer` | Judge placards; the token is in the placard URL's path | before printing placards |
| infra | A secrets store for the Ed25519 private key and `K_judge` (Postgres and Redis are now Ricky's, on the droplet) | Sessions, judge tokens | before the January mock run |
| infra / Josh | **Who deploys the API and web app on the droplet?** HTTPS for `app.geauxhack.org`, the PWA files, `/api/*` and `/api/v1/ws` proxied to the API on the same origin, the `.well-known` files (Josh's needs), and Kenny's deploy needs (20s stop grace, `/readyz` checks, port 9090 kept internal) | Production hosting | before the January mock run |
| Josh | **Wire PR 5 (#11) into the UI:**
- call `auth.startup()` at launch
- sign-in screens and the judge page go through `auth.signIn`
- show an expired banner
- call `auth.refresh()` on resume or when back online | Sign-in works in the app | after #11 |
| Josh | `android:allowBackup="true"` restores the encrypted token without its Keystore key. The app treats that as signed out (the user signs in again). It would be cleaner to exclude the secure-storage prefs from backup. Your call. | Clean restore on a new phone | before store release |
| Josh | **Domain:** auth uses `geauxhack.org` (Ricky's decision). `pr-1.4-capacitor` and `landing-page` still say `geauxhack.com`. Please switch them, or tell Ricky if the landing page should stay on .com. | One domain everywhere | before deploy |
| Josh | Storage interface (PR 1.2), deep links for `/auth/callback` and `/judge/:token` (PR 1.7), `history.replaceState` after exchange, `judge:room-rendered` performance mark | Native sign-in, judge flow, 200ms measurement | not blocking PR 1 |

## Done for others
| For | What was done | Link |
|---|---|---|
| Kenny | Your "database: schema gaps in design doc §5" need: team membership (`team_members`), `sponsor_keys`, face embeddings (`face_embeddings`), loan history (`hardware_assets` + `hardware_loans`), plus `entitlements`. Core schema PR | [#13](https://github.com/Tamely/gh-core/pull/13) |
| Josh | Picked the native secure-storage plugin (your "Which secure-storage plugin" need): `@aparajita/capacitor-secure-storage` 8.0.1, wired in PR 5 | [#11](https://github.com/Tamely/gh-core/pull/11) |

## Notes for the next session
- Ricky is new to auth and asked for plain-language explanations; explain concepts before using them.
- Already covered (don't re-teach unless asked): JWT structure; signing vs encrypting; how Ed25519 public/private keys relate (walked through a toy Schnorr signature); HMAC; Redis vs Postgres; Capacitor and why cookies fail in its WebView (`capacitor://localhost` origin); Keychain vs Keystore; the offline startup flow; migrations, middleware/layers, matched paths, fake clocks, table-driven tests, stacked PRs.
- The spec is `~/Downloads/GH_Tooling-2.pdf` (Nov 19 version); not in the repo yet.
- The full ticket text isn't in the repo. Paste it into the new session if exact acceptance criteria are needed.
- Work in the fork clone `~/Documents/GitHub/gh-core-fork` (origin = `liangricky7/gh-core`, upstream = `Tamely/gh-core`), not `~/Documents/GitHub/gh-core`.
- Commits: no AI co-author or "Generated with" lines. `core.hooksPath` is set to `.githooks` in the fork clone.
