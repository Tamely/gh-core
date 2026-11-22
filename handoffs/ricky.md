# Ricky: handoff

_Last updated: 2026-11-21_

## ⚠️ Core schema PR: takes priority for migrations (2026-11-21)
**This set of changes is the precedent for the database.** Its migrations run **first**, before every other gh-core migration. Any migration whose number conflicts with it (sorts before it, uses the same number, or creates a table it creates) **changes its number and its SQL**, not this one.

**What it is:** branch `ricky/schema` (worktree `.claude/worktrees/schema` in the fork clone), PR: _link below once opened_. It covers the schema ticket: the main PostgreSQL 16 + pgvector database, with reversible sqlx migrations for every core table.
- `server/migrations/`: 11 reversible `up.sql` / `down.sql` pairs, versions **`20261101000000` to `20261101001000`**, one logical change each: extension and shared `set_updated_at()` trigger, `users`, `teams` + `team_members`, `presentation_rooms`, `entitlements` + `entitlement_scans`, `hardware_assets` + `hardware_loans`, `sponsor_keys`, `mentor_tickets`, `notification_dispatches`, `judging_queue`, `face_embeddings` + `nearest_faces()`.
- `server/schema.sql`: the committed `pg_dump --schema-only` snapshot.
- `server/seeds/seed.sql`: synthetic data (1,000 users, 250 teams, 25 rooms, 5 entitlements, 5,000 scans, 500 assets, 600 sponsor keys, 1,000 random unit 512-d vectors). Not a migration.
- `server/db-tests/run.sh` (+ `acceptance.sql`, `claim.pgbench`, `README.md`) checks every acceptance criterion; CI runs it in `.github/workflows/schema.yml`. Needs Docker and sqlx-cli (`brew install sqlx-cli`). `run.sh --update` rewrites `schema.sql` after a migration change.
- `server/docker-compose.yml`: same file as the auth PRs' (identical, so it merges cleanly).

**Migration numbering rule (everyone):** use timestamps (`sqlx migrate add -r <name>` makes a reversible pair with the current time). Nothing may sort at or before `20261101001000`. Tables this PR creates are changed later with `ALTER TABLE`, never created again. Every migration is an `up.sql` / `down.sql` pair, because the CI check runs everything down and up again.

**What has to change elsewhere because of this:**
- **Auth PR 1 (#7)** `0001_users_and_auth.sql` is version 1, so it would run *before* the core schema. Rename it to a timestamp after `20261101001000` as a reversible pair, and **remove its `CREATE TABLE users`**: the core `users` already has the same `email` / `phone_e164` rules and the email-or-phone check, so auth's code and tests keep working. It keeps `user_roles` and `sessions` (auth owns those).
- **Auth PR 4 (#12)** `0002_judge_rooms.sql`: same renumbering, and **remove its `CREATE TABLE presentation_rooms`**: the core table already has `token_version`, `token_hash`, `judging_starts_at` / `judging_ends_at` with the same checks. It keeps `sessions.room_id` and `judge_exchange_audit`. The core table also has `track`, `floor_number`, `svg_x`, `svg_y` as NOT NULL (design doc §5), so `judge-admin room` and the judge tests must supply them (or only update the window on rooms that already exist).
- **Auth PR 3 (#10)**: the `0003` reservation is dropped; use a timestamp if it needs a migration.
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
- **Staging** (droplet Postgres 16.16, pgvector 0.8.7, in a throwaway database, since dropped): all 11 migrations up, seed, every acceptance check, all down leaving nothing behind. **Vector p99 isn't reliable there:** median 1.6–1.9ms, but p99 came out between 1.9 and 7ms across fresh runs, because the droplet is one shared vCPU that also runs the staging API, Redis and Caddy. Re-measure after the planned resize (≥ 2 vCPU / 4GB). On staging the migrations were applied file by file with `psql` (no sqlx there); sqlx itself is tested locally and in CI.

**Open:**
- The ticket's table list didn't come through when pasted; the tables come from §5 plus everything the acceptance criteria and other handoffs need. Check it against the ticket.
- Josh's applications, team alliance and points ledger tables are **not** in this ticket and still have no owner.

## Owns
**Auth: authentication and authorization for gh-core.** Sign-in, sessions, judge room access, roles. Built in **Rust** as the `server/auth` crate, plugged into Kenny's Axum server in `server/` (see `handoffs/kenny.md`, PR #4).

**Database hosting** (assigned 2026-11-20). **Dev: local Docker** via `server/docker-compose.yml` (`pgvector/pgvector:pg16` + Redis 7, both on `127.0.0.1`, dev-only password; Kenny merges his services in). **QA/prod: the DigitalOcean droplet** (1 vCPU / 1GB for now, resize to ≥ 2 vCPU / 4GB before the January mock run; keep CPU/RAM-only resize so it can shrink back). **Droplet set up (2026-11-21):** Ubuntu 24.04, reserved IP `161.35.242.244`, DNS `app.geauxhack.org` → that IP (Namecheap A record; `@` and `www` are still Namecheap parking). Done: ufw on (only 22, 80, 443), 2 GB swap (`swappiness=10`), Docker 29 + Compose, Caddy in `/opt/gh-core/caddy` with a Let's Encrypt cert (auto-renews) serving a placeholder plus `/healthz`. SSH is key-only, as root for now. **Docker-published ports skip ufw**, so Postgres and Redis must be published on `127.0.0.1` only (or not at all). **Staging on the droplet (2026-11-21, temporary):** `/opt/gh-core/staging` runs the auth API, Postgres and Redis in Docker Compose. No ports are published, so only Caddy reaches the API. The API is a merge of #10 and #12 made only for staging (not pushed), plus a short staging-only `main` that isn't in the repo. It's a release build. Its signing key, `JUDGE_KEY` and Postgres password were generated on the droplet and live in `staging/.env` (mode 600) with the Google and GitHub OAuth secrets. Email and SMS codes return 503 there (no Postmark or Twilio). It adds one stand-in route, `GET /api/v1/rooms/{room_id}/scores` (`RequireRoom`). Caddy routes `/api/*` and Apple's POST `/auth/callback` to the API, and serves two stand-in pages from `/opt/gh-core/site`: https://app.geauxhack.org/oauth-test and `/judge/*`. It still keeps no request logs. Test room `STAGING-1` (window ends 2026-11-24 23:59 Central); its placard is in `/opt/gh-core/placards` and is a credential. The old placeholder Caddy files are saved as `*.placeholder`. `https://app.geauxhack.org/auth/callback` is registered with Google and GitHub. **Wipe all of this once #7, #10 and #12 merge:** `docker compose down -v` in `staging/`, delete `staging/`, `site/` and `placards/`, and remove the test routes from the Caddyfile. Kenny's server and Josh's app replace it. Plan for the rest: lock down the droplet (SSH keys only, firewall allows only SSH and later 80/443), Docker Compose with `pgvector/pgvector:pg16` and Redis 7 (no persistence) both listening on `127.0.0.1` only, a non-admin `ghcore_app` role, teammates reach it through an SSH tunnel, nightly `pg_dump` to DigitalOcean Spaces (hourly during the event) with a tested restore, monitoring alerts.

## Status
**PR 1 done and in review** as [Tamely/gh-core#7](https://github.com/Tamely/gh-core/pull/7) from `liangricky7:ricky/auth-tokens` (the fork PR liangricky7/gh-core#1 is closed).

**PR 2 (email/SMS sign-in codes) is open** as [Tamely/gh-core#8](https://github.com/Tamely/gh-core/pull/8), from `liangricky7:ricky/auth-email-codes`. It depends on PR 1 (#7), which is still open; the upstream PR targets `main` because PR 1's head branch exists only on the fork. Merge #7 first. All 29 tests, Clippy with warnings denied and the release library build pass; live email verification/replay/logout passed. SMS live validation remains pending. **No migration is added.**

**PR 2 acceptance audit (2026-11-21):** Checked the original auth ticket against the PR 2 scope; evidence and remaining live checks are in [`server/auth/CODES.md`](../server/auth/CODES.md). Fixed a resend/concurrent-verification race: a check now atomically consumes only the hash/expiry it verified, so it cannot delete a newer challenge or reuse an old one across a resend. The new regression failed against the old unconditional delete. Provider redirects are refused to avoid forwarding codes/credentials; regressions cover redirects, Redis failure, the actual Redis TTL and Twilio 404/429/error responses. All 29 auth tests pass, Clippy passes with warnings denied, the release library build passes, and `git diff --check` is clean. Live **email** delivery passed on 2026-11-21 with a user-provided lsu.edu recipient: send 202, received-code verification/profile 200, reuse 401, logout 204, revoked-session lookup 401; Secure/HttpOnly/SameSite=Lax/14-day cookie, no token in JSON and no-store checks passed. A transient TLS timeout required a retry; no timeout/code changes were made. Postmark account approval still blocks Gmail delivery (422 / ErrorCode 412); **SMS remains unverified** with incomplete configuration. Existing provider settings were preserved and subsequently moved into ignored `server/.env`; the tracked template is blank. The audit fixes and acceptance checklist are committed as [`802e4e6`](https://github.com/liangricky7/gh-core/commit/802e4e6) and pushed in [PR #8](https://github.com/Tamely/gh-core/pull/8). Carry these fixes into PR 3 when updating the stack; its existing worktree was left untouched and is currently based on `87186a9`.

**Final PR 2 code review (2026-11-21):** Reviewed the complete PR 2 diff plus the current audit fixes: generation/hash/expiry/attempt accounting, resend/consume races, provider error handling and redirects, session delivery, proxy-IP wiring and dev/release boundaries. No additional auth-code correctness defects found. **Credential blocker resolved (2026-11-21):** with Ricky's approval, all five Postmark/Twilio settings were preserved in ignored `server/.env` with permissions `0600`; `server/.env.example` now contains only blank values. Verified Git ignores the local file and no copied credential values remain in tracked working files. No implementation changes or live sends were made during this review; the latest 29-test/Clippy/release results and successful live email test remain applicable. SMS live validation is still pending. PR 2 is now published as [#8](https://github.com/Tamely/gh-core/pull/8); this handoff update is committed separately.

**Pre-merge audit of PRs 1–4 (2026-11-21):** checked every ticket AC against tests, the design, and the docs. Fixes (pushed 2026-11-21; PR descriptions of #7 and #10 updated to match; each merged forward PR1 → PR2 → PR3 and PR2 → PR4, merge commits, no rebase):
- PR 1 `f565769`: deleting a user with sessions is refused (RESTRICT) instead of cascading, since a vanished session row never reaches the Redis revocation cache. **Dev databases that applied 0001 before this need `docker compose -f server/docker-compose.yml down -v`** (sqlx checksum mismatch); tests are unaffected.
- PR 1 `87ef41f` (+ PR 3 `fa11ae2`): the tests' Redis client waits 5s instead of 1s. Docker Desktop on the Mac sometimes stalled one command past 1s under load, failing `rate_limits` or an OAuth test about one run in three.
- PR 2 `f24e24f`: `CODES.md` is now one AC table naming the test for each requirement (SMS live check still open, ops).
- PR 3 `5eb6e77`: OAuth start reuses `codes::limit` instead of its own Lua counter; `OAUTH.md` said `.com`, now `.org`.
- PR 4: no code changes needed.
- **Testing trap:** two checkouts that share one `CARGO_TARGET_DIR` volume and both mount at `/work` can reuse each other's stale test binaries (cargo goes by file times), so a branch's new tests silently don't run. Give each checkout its own target volume, or `touch` the `.rs` files first.
- **Results after the fixes**, forced rebuild, Clippy `-D warnings` clean on all four: PR 1 12/12, PR 2 29/29, PR 3 36/36, PR 4 33/33 tests.
- **Merge order upstream:** #7 → #8 → then #10 and PR 4 in either order; the second of those will conflict in `AuthState` (`oauth` + `judge` fields), `caller.rs`, `lib.rs`, `policy.rs`, `tests/common/mod.rs`, `auth-dev.rs`. Keep both sides.

**PR 4 (judge rooms) is open** as [Tamely/gh-core#12](https://github.com/Tamely/gh-core/pull/12) (opened 2026-11-21), on `ricky/auth-judge-rooms` (worktree `.claude/worktrees/auth-judge-rooms` in the fork clone), branched from PR 2 (`ricky/auth-email-codes`) so it doesn't wait on PR 3. Ricky took PR 4 back from Kenny (2026-11-21). Built from the requirements doc https://claude.ai/artifact/NCxzNrcHu26EL2vE11kQpc:
- Migration `server/migrations/0002_judge_rooms.sql`: minimal `presentation_rooms` (`room_id` letters/digits/`-`/`_`, `token_version`, `token_hash`, `judging_starts_at` / `judging_ends_at`), `sessions.user_id` nullable plus `sessions.room_id` (exactly one is set; a room with sessions can't be deleted, since a cascade would skip the Redis revocation cache: rotate first), and `judge_exchange_audit`.
- `POST /api/v1/judge/exchange` `{"token"}` (`Public`) → 200 `{id: "judge:<sid>", roles: ["JUDGE_ROOM"], exp, roomId}` with the cookie for our web `Origin` (Max-Age = until judging ends) or `token` in the body otherwise. Every rejection (bad or missing body, won't decode, bad HMAC, unknown room, old version, outside the window) is the same bare 401. 503 if Postgres is down. `Cache-Control: no-store` and `Referrer-Policy: no-referrer` on every answer. Code in `server/auth/src/judge.rs`.
- `judge-admin` (release-safe admin binary, runs against the real database): `room ROOM_ID STARTS ENDS` (create a room or move its window), `print [DIR]` (every room's URL, plus `DIR/ROOM_ID.svg` QR codes), `rotate ROOM_ID` (new placard; the old one and every session it gave out stop working). Needs `DATABASE_URL`, `JUDGE_KEY`, and `REDIS_URL` for rotate; `APP_URL` defaults to `https://app.geauxhack.org`.
- Tests: `tests/judge.rs` (exchange, room A vs B, admin passes every room, never renews, dead at the close of judging, logout, each rejection, rotation, audit rows, no token in any table, migration rules) and real judge sessions from the exchange in `tests/matrix.rs`. All auth tests and Clippy (`-D warnings`) pass. Sabotage checked: skipping the version, window or HMAC check each turns a test red. `judge-admin` smoke-tested on a throwaway database.
- **Latency** (`cargo test -p auth --test judge -- --ignored --nocapture exchange_p99`, 2,000 exchanges, optimized build, Ricky's Mac, after the row-lock fix): API and Postgres on the same Docker network (like the droplet) **p99 9 to 17ms with 16 callers** over three runs, 4ms with one. Under the 20ms budget, but closer than before the lock (11ms). Re-measure on the droplet.
- Reviewed (subagent, medium) and fixed: exchange vs rotate race (the exchange holds a `FOR SHARE` row lock until its session is written, so rotate's revoke always sees it), room delete no longer cascades past revocation, bad JSON bodies get the same 401 and an audit row, `judge-admin print` writes QR files readable by the owner only (0600).
- **Left open from the review, with recommendations** (2026-11-21): (a) **decided (Ricky, 2026-11-21): follow Josh's route** and keep the token in the path, `/judge/<token>` (his router in PR 1.2 and 1.4 already uses `/judge/:token`, so nothing changes in his code or in `judge-admin`). In return, no access log may record `/judge/*`: Caddy on the droplet has no `log` directive today, so it logs no requests; if logging is turned on it must filter `/judge/*`, and Kenny's server must not log those paths; (b) no rate limit on the exchange: fine for launch (guessing a 256-bit HMAC is hopeless); if junk floods the audit table, add a per-IP limit with `codes::limit`; (c) a hacker scanning a placard in their own browser loses their session cookie: Josh's page should warn first if `/me` says someone is signed in; (d) 503s (Postgres down) aren't audited: leave it, there's nowhere to write; (e) **fixed**: PR 1's `users` → `sessions` is now RESTRICT ([`f565769`](https://github.com/liangricky7/gh-core/commit/f565769)).
- **Acceptance criteria** (from the requirements doc), 2026-11-21:
  - Forged, tampered, rotated, outside-window tokens rejected: yes, `tests/judge.rs` with a fake clock.
  - Judge A gets 403 on room B and on every non-judge route; admin passes every `RequireRoom` route: yes, `tests/matrix.rs` (every route × judge A/B/admin) and `tests/judge.rs`.
  - Ends at the close of judging, never renews; revoked → 401 next request: yes, `tests/judge.rs`.
  - Every exchange audited with device, time and room; token in no table: yes, `tests/judge.rs`. No log line: the auth crate and server log nothing per request, and the token is in the body, not the URL the API sees. Kenny: if you add a request logger, keep bodies out of it.
  - p99 under 20ms: yes, see Latency.
  - Admin script prints URL + QR for every room, rotate kills the old placard: yes, smoke-tested again after the review fixes on a throwaway database (SVGs are 0600); rotate also tested in `tests/judge.rs`.
  - Sabotage (skip version / window / HMAC check → test goes red): yes, rerun on the final exchange code.
  - **Scan → render and address bar, checked with a stand-in page** (2026-11-21): a throwaway `/judge/:token` page (not Josh's, not in the repo) running against `auth-dev` in a desktop browser. It calls `history.replaceState` first, exchanges, gets the HttpOnly cookie, calls `/me`, then marks `judge:room-rendered`. Results: the address bar shows `/judge`, Back skips straight past the token page, and the cookie isn't readable by page JavaScript. Navigation start → mark over 10 loads: **median ≈50ms, max 111ms** (first load in a fresh browser 225ms). So the server side fits the 200ms budget with room to spare. **Still to do with Josh:** the official run on his real page, on a physical phone over venue Wi-Fi (median of 10, cold cache, p95 + device in the PR).
  - **For Josh's page:** call `history.replaceState` before the exchange, not after; and on a reload of `/judge` (no token left) check `/me` first, because there's nothing to exchange and the cookie is already set.

**Order (from Ricky's senior dev, 2026-11-20): all five auth PRs and the Postgres host are finished and pushed before Kenny pushes his server stack.** Local Docker counts as the host for now; it will most likely have to be the droplet. So auth lands on `main` first and Kenny builds on it:
- `server/Cargo.toml` and `server/docker-compose.yml` are the **real** files. Kenny merges his members and services into them.
- **Superseded by the core schema PR (see the top of this file):** the core schema migrations (`20261101…`) run first and create `users` and `presentation_rooms`; auth's migrations are renumbered after them and only add `user_roles`, `sessions`, `sessions.room_id` and `judge_exchange_audit`.
- Auth is a set of building blocks with no server around it, tested on a small stand-in router: `auth::layer` (who is calling, policy map with default deny, revocation, renewal), `auth::routes`, and `auth::ws_caller` / `ws_first_message` / `may_subscribe` for WebSockets. Kenny's server builds an `AuthState` (his `PgPool` and fred client go in `Sessions`), calls `sessions.reload(now)` at startup, merges `auth::routes`, adds a policy for each of his routes to `auth::policies()`, puts `auth::layer` on the whole router with `Router::layer`, and uses the WebSocket functions in place of `auth::AllowAll`.

No Rust toolchain on Ricky's Mac yet: tests run in Docker (`rust:1-slim-bookworm`, Postgres from `pgvector/pgvector:pg16`):
```
docker compose -f server/docker-compose.yml up -d --wait
docker run --rm -v "$PWD/server:/work" -w /work -v gh-auth-cargo:/usr/local/cargo/registry -v gh-auth-target:/target -e CARGO_TARGET_DIR=/target -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/postgres -e REDIS_URL=redis://host.docker.internal:6379 rust:1-slim-bookworm cargo test -p auth
```

Try it by hand with the dev-only `auth-dev` tool (fixed dev key, never deployed). From the repo root, define a helper (works in zsh and bash), start the server, then mint a token and curl in a second terminal (define the helper there too):
```
gh_rust() { docker run --rm -v "$PWD/server:/work" -w /work -v gh-auth-cargo:/usr/local/cargo/registry -v gh-auth-target:/target -e CARGO_TARGET_DIR=/target -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/postgres -e REDIS_URL=redis://host.docker.internal:6379 --env-file server/.env "$@"; }
gh_rust -e AUTH_DEV_ADDR=0.0.0.0:8080 -p 127.0.0.1:8080:8080 rust:1-slim-bookworm cargo run -q --bin auth-dev -- serve
T=$(gh_rust rust:1-slim-bookworm cargo run -q --bin auth-dev -- mint ADMIN)
curl -H "Authorization: Bearer $T" 127.0.0.1:8080/api/v1/me
```

### PR 1 build order (agreed 2026-11-20)
All steps done (2026-11-20), shipped as **one PR**: [Tamely/gh-core#7](https://github.com/Tamely/gh-core/pull/7) (the fork PR liangricky7/gh-core#1 is closed). Auth PRs 2–5 are separate PRs, each merged before the next opens; Kenny's merges only after all five.

| # | Step | Proves | Test |
|---|---|---|---|
| 1 | Migration `server/migrations/0001_users_and_auth.sql`: minimal `users` (verified lowercase email and/or E.164 phone), `user_roles` (HACKER/VOLUNTEER/MENTOR/ADMIN; JUDGE_ROOM is session-only), `sessions` (`id` = `sid`) | Loads as the first migration | Bad role / duplicate / expiry before creation rejected; deleting a user cascades |
| 2 | Ed25519 JWT issue/verify (`server/auth/src/token.rs`), `kid` = RFC 7638 thumbprint, old public keys accepted | Mint tokens, reject every bad one | Table test: tampered, wrong key, `alg: none`/`HS256`, unknown `kid`, expired (fake clock), old key OK |
| 3 | `GET /.well-known/jwks.json` (`auth::routes`) | Others can verify with public info only | `jsonwebtoken` verifies our token using only the JWKS; no `d` in output |
| 4 | **Done** (`auth::layer`, `auth::identify`). Caller extraction: `__Host-session` cookie or `Authorization: Bearer`; `Origin` = `https://app.geauxhack.org` on cookie requests that change data | Every request is a known caller or anonymous | Cookie, Bearer, none, garbage, cross-origin POST → 403 |
| 5 | **Done** (`auth::Sessions`, tests/sessions.rs). Sessions + revocation: logout `POST /api/v1/auth/logout`, admin `POST /api/v1/admin/users/{id}/revoke-sessions`, Redis `gh:revoked:{sid}` with TTL plus a `gh:revoked:ready` marker (missing = Redis lost its data → ask Postgres and reload), Postgres fallback on Redis error (both down → 503), reload on startup, sliding renewal under 7 days (cookie re-set on web, `x-session-token` response header on native; judge sessions never renew) | Revoke works on the next request, even across Redis restarts/outages | Revoke → 401; flush+restart → 401; Redis down → 401; day 6 / 8 / 15 with fake clock |
| 6 | **Done** (`auth::Policy`, `auth::policies()`). Policies + default deny: one global layer looks up `"METHOD /matched/path"` in a policy map (`Public`, `SignedIn`, `RequireRole`, `RequireRoom`); no entry → 403, signed out → 401, wrong role → 403. Kenny's `/healthz`, `/readyz`, `/api/v1/ws` are `Public`. First route `GET /api/v1/me` | A forgotten route is locked | Per-policy unit tests; unlisted route → 403 |
| 7 | **Done** (`auth::ws_caller`, `ws_first_message`, `may_subscribe`, tests/ws.rs). WebSocket auth replacing Kenny's `auth::AllowAll`: web = cookie + `Origin` check; native = token as first message (never in the URL, which gets logged). `user:{id}` self only, `room:{id}` that room's judges + admins, `team:*` / `mentor-tag:*` admin only until their tables exist | Live updates no longer open to anyone | Topic × caller tests |
| 8 | **Done** (tests/matrix.rs). Route × role matrix test (anon, each role, no roles, judge room A/B, revoked, expired, unlisted and unknown routes); fails if a policy-map route has no row | Whole authorization picture in one table | `cargo test` |
| 9 | **Done** as `auth-dev mint ROLE [email]` plus `auth-dev serve` (auth's routes on :8080), since there's no server to curl yet. Dev-only binary, fixed dev key, never part of the server | Test signed-in requests before sign-in exists | `curl -H "Authorization: Bearer $(auth-dev mint ADMIN)" 127.0.0.1:8080/api/v1/me` (full Docker commands under Status) |

### PR 2: sign-in codes (built 2026-11-21)
- `POST /api/v1/auth/code` `{"email"}` or `{"phone"}` (E.164) → 202 (always, so it can't tell who has an account), 400 bad address (ours or the provider's: invalid or landline number, bounced email), 429 rate limit (ours or the provider's), 503 Redis/provider down, 500 if the server can't see the client's IP (wiring bug).
- `POST /api/v1/auth/code/verify` `{"email" or "phone", "code"}` → 200 `{id, roles, exp}` plus the `__Host-session` cookie when `Origin` is our web app, or plus `token` in the body otherwise (native). 401 wrong/expired/used code, 400 bad address or no `code`, 429 rate limit (per IP, per address per day, or Twilio's), 503 Redis/Twilio/Postgres down, 500 if the server can't see the client's IP. `Cache-Control: no-store`.
- Both are `Public` in `auth::policies()` with rows in `tests/matrix.rs`. Code in `server/auth/src/codes.rs`, tests in `server/auth/tests/codes.rs`.
- `auth-dev serve` sends real codes through Postmark / Twilio when `server/.env` has their keys (copy `server/.env.example`; `.env` is git-ignored; `POSTMARK_SERVER_TOKEN=POSTMARK_API_TEST` checks requests without sending), and otherwise prints that channel's codes to its terminal: `curl -H 'content-type: application/json' -d '{"email":"me@example.com"}' 127.0.0.1:8080/api/v1/auth/code`, then the same to `/api/v1/auth/code/verify` with `"code"`.

### Later PRs
3. OAuth: Google, GitHub, Apple. 4. **Judge rooms: built, back with Ricky (see Status)**. Requirements doc: https://claude.ai/artifact/NCxzNrcHu26EL2vE11kQpc. 5. Native/offline client pieces (with Josh's storage interface).

**Open question for Ricky's senior dev:** can PR 4 merge before PR 3 if OAuth is still waiting on credentials?

### Scope (summary of the ticket)
- **Sign-in:** Google, GitHub, Apple (OAuth authorization code + PKCE; system browser on native; callback via Universal/App Link `https://app.geauxhack.org/auth/callback`; accounts linked by *verified* email). Plus 6-digit email/SMS codes (OS secure random, only a hash stored, 10 min expiry, 5 attempts, constant-time compare, rate limits per email/phone and per IP → `429`, phones in E.164). SMS via Twilio Verify; email via Postmark or SES.
- **Sessions:** Ed25519 JWTs (`alg: EdDSA`, `kid` header), 14 days. Claims: `sub`, `roles`, `sid`, `iat`, `exp`, plus `room_id` for judges. Public keys at `/.well-known/jwks.json`. Private key from a secrets store. `sessions` table for revocation; revoked `sid`s cached in Redis so a revoke takes effect on the next request. Sliding renewal when under 7 days left (tested with a fake clock). Old `kid`s accepted until their tokens expire.
- **Web:** cookie `__Host-session; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=1209600`. Same-origin API (`app.geauxhack.org/api/v1/...`). CSRF: SameSite + `Origin` check on data-changing requests. Non-secret profile snapshot (including `exp`) in IndexedDB.
- **Native:** token in secure storage (iOS Keychain `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; Android Keystore-backed). `Authorization: Bearer`. Offline startup reads the token and profile, checks `exp` against the device clock, no network calls. Expired while offline: cached content read-only, ask to sign in once online.
- **Judge rooms:** token = base64url(`room_id ‖ version ‖ HMAC-SHA256(K_judge, room_id ‖ version)`); only version + token hash stored; rotate = bump version. `POST /api/v1/judge/exchange` checks HMAC, version, judging window → `JUDGE_ROOM` session for that room, expiring at the end of judging. `Referrer-Policy: no-referrer`, client strips the token with `history.replaceState`, never logged. Admin script prints QR codes. Audit log of every exchange. Targets: 200ms scan → `judge:room-rendered` (median of 10, cold cache, physical phone); `/judge/exchange` under 20ms p99.
- **Authorization:** roles `HACKER`, `VOLUNTEER`, `MENTOR`, `JUDGE_ROOM`, `ADMIN`; `user_roles` table. `RequireRole(...)`, `RequireRoom(room_id)`. Routes without a policy → `403`. Table-driven tests over every route × role.

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
| Kenny | Your "database: schema gaps in design doc §5" need: team membership (`team_members`), `sponsor_keys`, face embeddings (`face_embeddings`), loan history (`hardware_assets` + `hardware_loans`), plus `entitlements`. Core schema PR | _link below once opened_ |
| Josh | Picked the native secure-storage plugin (your "Which secure-storage plugin" need): `@aparajita/capacitor-secure-storage` 8.0.1, wired in PR 5 | [#11](https://github.com/Tamely/gh-core/pull/11) |

## Notes for the next session
- Ricky is new to auth and asked for plain-language explanations; explain concepts before using them.
- Already covered (don't re-teach unless asked): JWT structure; signing vs encrypting; how Ed25519 public/private keys relate (walked through a toy Schnorr signature); HMAC; Redis vs Postgres; Capacitor and why cookies fail in its WebView (`capacitor://localhost` origin); Keychain vs Keystore; the offline startup flow; migrations, middleware/layers, matched paths, fake clocks, table-driven tests, stacked PRs.
- The spec is `~/Downloads/GH_Tooling-2.pdf` (Nov 19 version); not in the repo yet.
- The full ticket text isn't in the repo. Paste it into the new session if exact acceptance criteria are needed.
- Work in the fork clone `~/Documents/GitHub/gh-core-fork` (origin = `liangricky7/gh-core`, upstream = `Tamely/gh-core`), not `~/Documents/GitHub/gh-core`.
- Commits: no AI co-author or "Generated with" lines. `core.hooksPath` is set to `.githooks` in the fork clone.
