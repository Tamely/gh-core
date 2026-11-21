# Ricky: handoff

_Last updated: 2026-11-20_

## Owns
**Auth: authentication and authorization for gh-core.** Sign-in, sessions, judge room access, roles. Built in **Rust** as the `server/auth` crate, plugged into Kenny's Axum server in `server/` (see `handoffs/kenny.md`, PR #4).

**Database hosting** (assigned 2026-11-20). **Dev: local Docker** via `server/docker-compose.yml` (`pgvector/pgvector:pg16` + Redis 7, both on `127.0.0.1`, dev-only password; placeholder until Kenny's compose file lands). **QA/prod: the DigitalOcean droplet** (1 vCPU / 1GB for now, resize to ≥ 2 vCPU / 4GB before the January mock run; keep CPU/RAM-only resize so it can shrink back). Droplet not started. Plan: lock down the droplet (SSH keys only, firewall allows only SSH and later 80/443), Docker Compose with `pgvector/pgvector:pg16` and Redis 7 (no persistence) both listening on `127.0.0.1` only, a non-admin `ghcore_app` role, teammates reach it through an SSH tunnel, nightly `pg_dump` to DigitalOcean Spaces (hourly during the event) with a tested restore, monitoring alerts.

## Status
**PR 1a in progress** on branch `ricky/auth-tokens` (fork `liangricky7/gh-core`, not pushed). Steps 1–3 below are written; nothing committed yet.

Kenny's server branches aren't pushed, so `server/Cargo.toml` is a **placeholder workspace** with only `auth` in it. The migration test makes a stand-in `users (id)` table in place of Kenny's 0001. When his stack lands: keep his `server/Cargo.toml` and add `"auth"` to its members, move `server/auth/migrations/0002_auth.sql` into his migrations folder with his numbering, and mount `auth::routes(keys)` in his router.

No Rust toolchain on Ricky's Mac yet: tests run in Docker (`rust:1-slim-bookworm`, Postgres from `pgvector/pgvector:pg16`):
```
docker compose -f server/docker-compose.yml up -d --wait
docker run --rm -v "$PWD/server:/work" -w /work -v gh-auth-cargo:/usr/local/cargo/registry -v gh-auth-target:/target -e CARGO_TARGET_DIR=/target -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/postgres rust:1-slim-bookworm cargo test -p auth
```

### PR 1 build order (agreed 2026-11-20)
Proposed split into two stacked PRs: **1a = steps 1–3** (no request-path code), **1b = steps 4–9**. 1a is underway; one combined PR is still an option.

| # | Step | Proves | Test |
|---|---|---|---|
| 1 | Migration `0002_auth.sql`: `user_roles` (HACKER/VOLUNTEER/MENTOR/ADMIN; JUDGE_ROOM is session-only), `sessions` (`id` = `sid`) | Loads on top of Kenny's schema | Bad role / duplicate / expiry before creation rejected; deleting a user cascades |
| 2 | Ed25519 JWT issue/verify (`server/auth/src/token.rs`), `kid` = RFC 7638 thumbprint, old public keys accepted | Mint tokens, reject every bad one | Table test: tampered, wrong key, `alg: none`/`HS256`, unknown `kid`, expired (fake clock), old key OK |
| 3 | `GET /.well-known/jwks.json` (`auth::routes`) | Others can verify with public info only | `jsonwebtoken` verifies our token using only the JWKS; no `d` in output |
| 4 | Caller extraction: `__Host-session` cookie or `Authorization: Bearer`; `Origin` = `https://app.geauxhack.com` on cookie requests that change data | Every request is a known caller or anonymous | Cookie, Bearer, none, garbage, cross-origin POST → 403 |
| 5 | Sessions + revocation: logout, admin revoke, Redis `gh:revoked:{sid}` with TTL, Postgres fallback on Redis error (both down → 503), reload on startup, sliding renewal under 7 days | Revoke works on the next request, even across Redis restarts/outages | Revoke → 401; flush+restart → 401; Redis down → 401; day 6 / 8 / 15 with fake clock |
| 6 | Policies + default deny: one global layer looks up the matched path in a policy map (`Public`, `RequireRole`, `RequireRoom`); no entry → 403. Kenny's `/healthz`, `/readyz` are `Public`. First route `GET /api/v1/me` | A forgotten route is locked | Per-policy unit tests; unlisted route → 403 |
| 7 | WebSocket auth replacing Kenny's `auth::AllowAll`: web = cookie + `Origin` check; native = token as first message (never in the URL, which gets logged). `user:{id}` self only, `room:{id}` that room's judges + admins, `team:*` / `mentor-tag:*` admin only until their tables exist | Live updates no longer open to anyone | Topic × caller tests |
| 8 | Route × role matrix test (anon, each role, judge room A/B, revoked, expired); fails if a policy-map route has no row | Whole authorization picture in one table | `cargo test` |
| 9 | Dev-only `mint-token` binary (never part of the server) | Test signed-in requests before sign-in exists | `curl -H "Authorization: Bearer $(mint-token --role ADMIN)" …/api/v1/me` |

### Later PRs
2. Email/SMS one-time codes. 3. OAuth: Google, GitHub, Apple. 4. Judge room tokens, exchange endpoint, QR script, audit log. 5. Native/offline client pieces (with Josh's storage interface).

### Scope (summary of the ticket)
- **Sign-in:** Google, GitHub, Apple (OAuth authorization code + PKCE; system browser on native; callback via Universal/App Link `https://app.geauxhack.com/auth/callback`; accounts linked by *verified* email). Plus 6-digit email/SMS codes (OS secure random, only a hash stored, 10 min expiry, 5 attempts, constant-time compare, rate limits per email/phone and per IP → `429`, phones in E.164). SMS via Twilio Verify; email via Postmark or SES.
- **Sessions:** Ed25519 JWTs (`alg: EdDSA`, `kid` header), 14 days. Claims: `sub`, `roles`, `sid`, `iat`, `exp`, plus `room_id` for judges. Public keys at `/.well-known/jwks.json`. Private key from a secrets store. `sessions` table for revocation; revoked `sid`s cached in Redis so a revoke takes effect on the next request. Sliding renewal when under 7 days left (tested with a fake clock). Old `kid`s accepted until their tokens expire.
- **Web:** cookie `__Host-session; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age=1209600`. Same-origin API (`app.geauxhack.com/api/v1/...`). CSRF: SameSite + `Origin` check on data-changing requests. Non-secret profile snapshot (including `exp`) in IndexedDB.
- **Native:** token in secure storage (iOS Keychain `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; Android Keystore-backed). `Authorization: Bearer`. Offline startup reads the token and profile, checks `exp` against the device clock, no network calls. Expired while offline: cached content read-only, ask to sign in once online.
- **Judge rooms:** token = base64url(`room_id ‖ version ‖ HMAC-SHA256(K_judge, room_id ‖ version)`); only version + token hash stored; rotate = bump version. `POST /api/v1/judge/exchange` checks HMAC, version, judging window → `JUDGE_ROOM` session for that room, expiring at the end of judging. `Referrer-Policy: no-referrer`, client strips the token with `history.replaceState`, never logged. Admin script prints QR codes. Audit log of every exchange. Targets: 200ms scan → `judge:room-rendered` (median of 10, cold cache, physical phone); `/judge/exchange` under 20ms p99.
- **Authorization:** roles `HACKER`, `VOLUNTEER`, `MENTOR`, `JUDGE_ROOM`, `ADMIN`; `user_roles` table. `RequireRole(...)`, `RequireRoom(room_id)`. Routes without a policy → `403`. Table-driven tests over every route × role.

## Decisions
- **Rust, not Go** (2026-11-20). The spec (§3) allows either for the single API binary; the team chose Rust and Kenny's Axum server already exists. Auth is a crate in his workspace, not a second server.
- **Production domain `app.geauxhack.com`** (was `.org`), matching Josh's and Kenny's latest handoffs.
- **JWTs are signed and checked with `ed25519-dalek` directly**, not a JWT library. Verify accepts only `alg: EdDSA` whatever the header says, uses `verify_strict`, and rejects padded or non-canonical base64. `jsonwebtoken` is a test-only dependency that proves other software can verify our tokens from the JWKS.
- **`kid` is the RFC 7638 thumbprint** of the public key, so there's no key-ID config to get wrong.
- **Key config:** current private key as PKCS#8 PEM (`openssl genpkey -algorithm ed25519`); old keys as comma-separated base64url public keys (their JWKS `x`). Env var names are picked when it's wired into Kenny's config.
- **`verify` takes `now` as an argument** instead of reading the clock, so tests use a fake time.
- **`JUDGE_ROOM` can't be stored in `user_roles`.** It only exists in sessions from the judge exchange, tied to a room, so no account can be given it by accident.
- **JWKS is cached for 5 minutes.** After a key rotation, outside verifiers can miss the new key for up to 5 minutes. Only matters if something other than our server verifies tokens.
- **Development uses local Docker Postgres/Redis; the droplet is QA/prod only** (2026-11-20). No heavy traffic in the dev phase, so the droplet stays small until the mock run.
- **In QA/prod, the API server, Postgres and Redis all run on the same DigitalOcean droplet** (2026-11-20). Postgres and Redis are never reachable from the internet; the API connects over `127.0.0.1` (Kenny's note: not `localhost`). Avoids sending every query across the internet, which a separate host (e.g. AWS) would.
- **Revocation when Redis is down:** fall back to Postgres; if both are down, 503, never allow.
- **Secure-storage plugin (proposed, not final):** `@aparajita/capacitor-secure-storage`, as named in the ticket. To be confirmed as maintained before PR 1.4 needs it.
- **SMS codes:** Twilio Verify generates and checks SMS codes itself, so the hash / expiry / attempt rules are built by us for **email** codes only.

## Provides to others
- `auth::Keys` (`load`, `issue`, `verify`), `auth::Claims`, `auth::Role`: **in progress (PR 1a)**, shape may still change.
- `auth::routes(keys)` → `GET /.well-known/jwks.json`, policy Public: **in progress (PR 1a)**.
- Planned, not built yet:
  - `POST /api/v1/judge/exchange`: judge QR token → `JUDGE_ROOM` session.
  - Session cookie `__Host-session` (web), `Authorization: Bearer <jwt>` (native).
  - JWT claims: `sub`, `roles`, `sid`, `iat`, `exp`, `room_id` (judges).
  - Policy map with `RequireRole(...)` / `RequireRoom` for every backend route. A route with no policy returns `403`. **Kenny:** new routes need an entry, or they 403.
  - `GET /api/v1/me` for Josh's profile snapshot.

## Needs from others
| For | What's needed | Why / blocks | By |
|---|---|---|---|
| Kenny | **Push the server stack** (at least `kenny/server-skeleton` and `kenny/server-database`) | PR 1b wires auth into your router, `db::acquire` and Redis; 1a only uses a placeholder workspace | before PR 1b |
| Kenny | You take `POST /api/v1/notify` next; Ricky is on auth. | Answers "notify or auth next" in your handoff | — |
| Kenny / database | Make `users.phone_e164`, `full_name`, `school`, `qr_hmac_sig` nullable (or filled later). A first Google/GitHub/Apple or email-code sign-in has none of them. | OAuth and codes PRs can't create users | before PR 2 |
| Kenny / database | `presentation_rooms.panel_hmac_token` stores the judge token itself. The ticket says store only a token **version** and a token **hash**, plus a judging window (start/end). | Judge rooms PR | before PR 4 |
| Josh | `handoffs/josh.md` says `/api/*` goes to "the Rust server": correct, the backend is Rust (Kenny's Axum server). No change needed. | — | — |
| ops | Google OAuth client IDs (web, iOS, Android) | OAuth PR | before OAuth PR |
| ops | GitHub OAuth app | OAuth PR | before OAuth PR |
| ops | Sign in with Apple: Services ID and private key (needs the Apple Developer enrollment Josh already asked for) | OAuth PR; App Store guideline 4.8 | before OAuth PR |
| ops | Twilio account with a Verify service; Postmark or SES account with a verified sending domain | One-time codes PR | before codes PR |
| infra | A secrets store for the Ed25519 private key and `K_judge` (Postgres and Redis are now Ricky's, on the droplet) | Sessions, judge tokens | before the January mock run |
| infra / Josh | **Who deploys the API and web app on the droplet?** HTTPS for `app.geauxhack.com`, the PWA files, `/api/*` and `/api/v1/ws` proxied to the API on the same origin, the `.well-known` files (Josh's needs), and Kenny's deploy needs (20s stop grace, `/readyz` checks, port 9090 kept internal) | Production hosting | before the January mock run |
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
