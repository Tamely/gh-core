# Ricky: handoff

_Last updated: 2026-11-20_

## Owns
**Auth: authentication and authorization for gh-core.** Sign-in, sessions, judge room access, roles. Built in **Rust** as the `server/auth` crate, plugged into Kenny's Axum server in `server/` (see `handoffs/kenny.md`, PR #4).

**Database hosting** (assigned 2026-11-20). **Dev: local Docker** via `server/docker-compose.yml` (`pgvector/pgvector:pg16` + Redis 7, both on `127.0.0.1`, dev-only password; Kenny merges his services in). **QA/prod: the DigitalOcean droplet** (1 vCPU / 1GB for now, resize to ≥ 2 vCPU / 4GB before the January mock run; keep CPU/RAM-only resize so it can shrink back). Droplet not started. Plan: lock down the droplet (SSH keys only, firewall allows only SSH and later 80/443), Docker Compose with `pgvector/pgvector:pg16` and Redis 7 (no persistence) both listening on `127.0.0.1` only, a non-admin `ghcore_app` role, teammates reach it through an SSH tunnel, nightly `pg_dump` to DigitalOcean Spaces (hourly during the event) with a tested restore, monitoring alerts.

## Status
**PR 1 done, all 9 steps** on `ricky/auth-tokens` (fork `liangricky7/gh-core`, not pushed): `1bb55aa` steps 1–3, `56b8003` step 4 and the migration move, then steps 5–9. Next: push and open the PR, then PR 2 (email/SMS codes).

**Order (from Ricky's senior dev, 2026-11-20): all five auth PRs and the Postgres host are finished and pushed before Kenny pushes his server stack.** Local Docker counts as the host for now; it will most likely have to be the droplet. So auth lands on `main` first and Kenny builds on it:
- `server/Cargo.toml` and `server/docker-compose.yml` are the **real** files. Kenny merges his members and services into them.
- `server/migrations/0001_users_and_auth.sql` is the **first** migration and creates a minimal `users` table. Kenny's spec migration becomes 0002 and **adds** his columns to `users` instead of creating it.
- Auth is a set of building blocks with no server around it, tested on a small stand-in router: `auth::layer` (who is calling, policy map with default deny, revocation, renewal), `auth::routes`, and `auth::ws_caller` / `ws_first_message` / `may_subscribe` for WebSockets. Kenny's server builds an `AuthState` (his `PgPool` and fred client go in `Sessions`), calls `sessions.reload(now)` at startup, merges `auth::routes`, adds a policy for each of his routes to `auth::policies()`, puts `auth::layer` on the whole router with `Router::layer`, and uses the WebSocket functions in place of `auth::AllowAll`.

No Rust toolchain on Ricky's Mac yet: tests run in Docker (`rust:1-slim-bookworm`, Postgres from `pgvector/pgvector:pg16`):
```
docker compose -f server/docker-compose.yml up -d --wait
docker run --rm -v "$PWD/server:/work" -w /work -v gh-auth-cargo:/usr/local/cargo/registry -v gh-auth-target:/target -e CARGO_TARGET_DIR=/target -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/postgres -e REDIS_URL=redis://host.docker.internal:6379 rust:1-slim-bookworm cargo test -p auth
```

Try it by hand with the dev-only `auth-dev` tool (fixed dev key, never deployed). From the repo root, define a helper (works in zsh and bash), start the server, then mint a token and curl in a second terminal (define the helper there too):
```
gh_rust() { docker run --rm -v "$PWD/server:/work" -w /work -v gh-auth-cargo:/usr/local/cargo/registry -v gh-auth-target:/target -e CARGO_TARGET_DIR=/target -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/postgres -e REDIS_URL=redis://host.docker.internal:6379 "$@"; }
gh_rust -e AUTH_DEV_ADDR=0.0.0.0:8080 -p 127.0.0.1:8080:8080 rust:1-slim-bookworm cargo run -q --bin auth-dev -- serve
T=$(gh_rust rust:1-slim-bookworm cargo run -q --bin auth-dev -- mint ADMIN)
curl -H "Authorization: Bearer $T" 127.0.0.1:8080/api/v1/me
```

### PR 1 build order (agreed 2026-11-20)
All steps done (2026-11-20), shipped as **one PR** (liangricky7/gh-core#1 on the fork; to be opened on Tamely/gh-core). Auth PRs 2–5 are separate PRs, each merged before the next opens; Kenny's merges only after all five.

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

### Later PRs
2. Email/SMS one-time codes. 3. OAuth: Google, GitHub, Apple. 4. Judge room tokens, exchange endpoint, QR script, audit log. 5. Native/offline client pieces (with Josh's storage interface).

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
- **Revocation doesn't close open WebSockets** (checked on connect only). Fine for now; re-check per event if it matters.
- **Router wiring rules for Kenny:** call `.layer(auth::layer)` **last** (axum's `Router::layer` only wraps routes that already exist, so a route added after it skips auth), and use `nest(Router)`, not `nest_service` (nested services have no matched route, so they'd all be 403).
- **Known small gaps, left on purpose:** unknown paths and wrong methods return 403 instead of 404/405 (safe, just less specific); CORS, if ever added, must sit outside the auth layer; the matrix test only checks `auth::policies()`, so Kenny's own routes need rows in a matrix of his; `gh:revoked:ready` is one shared Redis key, so two test runs at once on the same Redis can interfere.
- **`auth-dev` uses a fixed dev signing key**, so its tokens only work on `auth-dev serve`, never on a real server.
- **Secure-storage plugin (proposed, not final):** `@aparajita/capacitor-secure-storage`, as named in the ticket. To be confirmed as maintained before PR 1.4 needs it.
- **SMS codes:** Twilio Verify generates and checks SMS codes itself, so the hash / expiry / attempt rules are built by us for **email** codes only.

## Provides to others
Built in PR 1 (not pushed yet; shapes may still change before it merges):
- `auth::Keys` (`load`, `issue`, `verify`), `auth::Claims`, `auth::Role`.
- `auth::AuthState { keys, origin, now, sessions: Sessions::new(db, redis), policies }`, `auth::layer` (put on the whole router with `Router::layer`, as the last call), handlers read `Extension<auth::Caller>`.
- `auth::routes(state)`: `GET /.well-known/jwks.json`, `GET /api/v1/me` (`{id, roles, exp, roomId?}`), `POST /api/v1/auth/logout` (204, clears the cookie), `POST /api/v1/admin/users/{id}/revoke-sessions` (admin).
- Policy map `auth::policies()` keyed `"METHOD /path"` with `Public` / `SignedIn` / `RequireRole(...)` / `RequireRoom` (uses the `{room_id}` path parameter). A route with no policy returns `403`. **Kenny:** new routes need an entry, or they 403.
- Session cookie `__Host-session` (web), `Authorization: Bearer <jwt>` (native). **Josh:** when a response has an `x-session-token` header, native stores that token in place of the old one (sliding renewal); web gets a new cookie automatically.
- WebSockets: `auth::ws_caller(&state, &headers)` on upgrade, `auth::ws_first_message(&state, text)` for native's first message, `auth::may_subscribe(&caller, topic)` per subscribe. **Josh:** native sends the bare token as the first WebSocket message.
- Revocation keys in Redis: `gh:revoked:{sid}` and `gh:revoked:ready`.
- Planned: `POST /api/v1/judge/exchange` (PR 4): judge QR token → `JUDGE_ROOM` session.

## Needs from others
| For | What's needed | Why / blocks | By |
|---|---|---|---|
| Kenny | **Rebase onto auth once it's on `main`** (see Status for how to wire `AuthState`, `auth::layer`, `auth::routes` and the policy map; share your fred client, built with a 1s `default_command_timeout` (without one a Redis outage hangs requests instead of falling back to Postgres); `.layer(auth::layer)` goes last; use your migration runner on `server/migrations`, which already has sqlx bookkeeping from `auth-dev`) (per Ricky's senior dev, auth and Postgres land first). Your §5 migration becomes 0002: `users` already exists from `0001_users_and_auth.sql` (`id`, `email`, `phone_e164`, `created_at`), so ALTER it to add your columns. Merge your members/services into `server/Cargo.toml` and `server/docker-compose.yml`. Replace `auth::AllowAll` with auth's topic check. | Keeps one migration history and closes the open WebSocket | when auth is on `main` |
| Kenny | You take `POST /api/v1/notify` next; Ricky is on auth. | Answers "notify or auth next" in your handoff | — |
| Kenny / database | Make `users.phone_e164`, `full_name`, `school`, `qr_hmac_sig` nullable (or filled later). A first Google/GitHub/Apple or email-code sign-in has none of them. | OAuth and codes PRs can't create users | before PR 2 |
| Kenny / database | `presentation_rooms.panel_hmac_token` stores the judge token itself. The ticket says store only a token **version** and a token **hash**, plus a judging window (start/end). | Judge rooms PR | before PR 4 |
| Josh | `handoffs/josh.md` says `/api/*` goes to "the Rust server": correct, the backend is Rust (Kenny's Axum server). No change needed. | — | — |
| ops | Google OAuth client IDs (web, iOS, Android) | OAuth PR | before OAuth PR |
| ops | GitHub OAuth app | OAuth PR | before OAuth PR |
| ops | Sign in with Apple: Services ID and private key (needs the Apple Developer enrollment Josh already asked for) | OAuth PR; App Store guideline 4.8 | before OAuth PR |
| ops | Twilio account with a Verify service; Postmark or SES account with a verified sending domain | One-time codes PR | before codes PR |
| infra | A secrets store for the Ed25519 private key and `K_judge` (Postgres and Redis are now Ricky's, on the droplet) | Sessions, judge tokens | before the January mock run |
| infra / Josh | **Who deploys the API and web app on the droplet?** HTTPS for `app.geauxhack.org`, the PWA files, `/api/*` and `/api/v1/ws` proxied to the API on the same origin, the `.well-known` files (Josh's needs), and Kenny's deploy needs (20s stop grace, `/readyz` checks, port 9090 kept internal) | Production hosting | before the January mock run |
| Josh | Storage interface (PR 1.2), deep links for `/auth/callback` and `/judge/:token` (PR 1.7), `history.replaceState` after exchange, `judge:room-rendered` performance mark | Native sign-in, judge flow, 200ms measurement | not blocking PR 1 |

## Done for others
| For | What was done | Link |
|---|---|---|

## Notes for the next session
- Ricky is new to auth and asked for plain-language explanations; explain concepts before using them.
- Already covered (don't re-teach unless asked): JWT structure; signing vs encrypting; how Ed25519 public/private keys relate (walked through a toy Schnorr signature); HMAC; Redis vs Postgres; Capacitor and why cookies fail in its WebView (`capacitor://localhost` origin); Keychain vs Keystore; the offline startup flow; migrations, middleware/layers, matched paths, fake clocks, table-driven tests, stacked PRs.
- The spec is `~/Downloads/GH_Tooling-2.pdf` (Nov 19 version); not in the repo yet.
- The full ticket text isn't in the repo. Paste it into the new session if exact acceptance criteria are needed.
- Work in the fork clone `~/Documents/GitHub/gh-core-fork` (origin = `liangricky7/gh-core`, upstream = `Tamely/gh-core`), not `~/Documents/GitHub/gh-core`.
- Commits: no AI co-author or "Generated with" lines. `core.hooksPath` is set to `.githooks` in the fork clone.
