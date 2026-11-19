# Kenny: handoff

_Last updated: 2026-11-18_

## Owns
**Backend monolith (Rust, Axum)** in `server/`: HTTP API, Postgres pool and migrations, Redis pub/sub, WebSocket hub, health and metrics, the static release image, the load-test tool and their CI.

## Status
Four stacked branches, local only, not pushed yet. Each builds on the one before:

| Branch | Scope | State |
|---|---|---|
| `kenny/server-skeleton` | Cargo workspace, env config, JSON logs with request IDs, 3s timeout, `/healthz`, SIGTERM drain, static `FROM scratch` image, CI | done |
| `kenny/server-database` | sqlx pool with timeouts, 503 + `Retry-After` on overload, startup migrations (design doc §5), `/readyz`, `docker-compose.yml` | done |
| `kenny/server-realtime` | Redis fan-out, WebSocket hub at `/api/v1/ws`, `resync`/1013/1012, `/metrics` on port 9090 | done |
| `kenny/server-bench` | `server/bench` load tool, plus three fixes it found: `TCP_NODELAY`, metrics upkeep, resync after re-subscribe | done |
| `kenny/server-hardening` | `/readyz` on its own connection, request-ID validation, subscribe reply ordering, slow clients in the bench, test and build fixes | done |
| `kenny/server-edge-cases` | Redis command timeout and bigger buffers (bursts no longer cause resyncs), startup checks (URL schemes, port 0, 10s Postgres connect limit), tests that run the real binary, 25 more edge-case tests | done |

PR descriptions are drafted in `pr-drafts/` (not committed).

Measured (server in Docker limited to 2 CPUs, bench in Docker on the same machine, 1,000 clients over 25 rooms, 250 teams, 10 mentor tags, 100 events/s):
- 10-minute run: 0 dropped, 0 errors, 2,175,040/2,175,040 deliveries, 0 `seq` gaps.
- Publish to every subscriber: p99 2.43ms, max 6.1ms.
- Memory: 17MB idle 60s after start; 65.6MB with 1,000 clients, flat (was 10.9MB / 60MB before the burst fixes).
- Two instances on one Redis: every event delivered across instances, p99 2.5ms.
- SIGTERM with 1,000 clients: all got 1012, exit 0 after 5.5s.
- Image 11.5MB.
- Rolling deploy (A to B and back, 1,000 clients): 2,000 reconnects, 0 unannounced gaps, 0-2s without updates per move.
- Redis restart: 1.3s outage, every subscription got one `resync`, no client dropped. Postgres restart: `/readyz` 503 for ~1.1s, WebSockets unaffected.
- Bad Wi-Fi (300 clients, ~300ms, 5% loss) next to 700 normal and 50 frozen: normal p99 3.2ms, bad clients all stayed and got everything, frozen ones cut off.
- One-hour soak: no leaks (memory 65-68MB, sockets steady), 13M messages, 0 gaps. p99 6.2ms over the hour because of laptop VM stalls; re-measure on the real server.

- Slow client (`--slow 1 --pad 1024`, 3 minutes): the stuck client was cut off for being slow (server counter 1), the other 1,000 got every event at p99 3.8ms (4.7ms without it).

Test flake: `queue_positions_can_be_swapped_but_not_duplicated` failed once in about 15 full runs and couldn't be reproduced. Schema tests now use a plain pool instead of the 2s production limits, and CI runs 4 tests at a time. Heavier stress (30 containers at once) only produced container startup timeouts.

Not started: Josh's backend requests, as of his 2026-11-17 handoff on the `landing-page` branch:
- `POST /api/v1/notify` taking `{ "email" }`, stored for the "registration is open" email, CORS allowing `https://geauxhack.com` (the landing page posts to it cross-origin). Needs a table, a CORS layer and basic abuse limits.
- A schedule endpoint returning `EventInfo` and `ScheduleItem[]` from `apps/client/src/schedule/types.ts`.
- A current-user profile with `firstName` and `team` (needs auth).
- The application API, and the team pick endpoint with the 52% cap. The live team split needs one more topic kind (e.g. `event:split`), a few lines in `server/app/src/topic.rs`.

Checked against Josh's three draft branches (2026-11-18): all merge cleanly with `kenny/server-edge-cases`, and their client CI passes on the merge except Prettier, which flags `server/README.md` (see Needs from others). The production domain is now `app.geauxhack.com` (was `.org`), and the placeholder event dates are Feb 20-21, 2027.

## Decisions
- **`seq` is assigned in Redis** by a Lua script (`INCR gh:seq:{topic}` then `PUBLISH`, atomically). Every instance forwards the same number, so a client that reconnects to another instance can still detect gaps, and two publishers can't reorder events.
- **After a Redis reconnect every client gets `resync`.** Pub/sub keeps nothing, so events during the outage are gone.
- **Slow clients:** each connection has a 64-message queue. At half full the client gets `resync`; when full it's closed with 1013. Queued control messages (`resync`) are still sent before the close frame; queued events are dropped.
- **Closing handshake:** after sending a close frame the server waits (up to 1s) for the client's reply before dropping the socket. Dropping it first reset the connection and made clients lose the close code.
- **`TCP_NODELAY` on every accepted socket.** Without it, back-to-back messages to one client waited up to 40ms for a delayed ACK (p99 went from 24ms to 2.6ms).
- **Metrics upkeep runs every 5s.** The Prometheus exporter keeps raw histogram samples until upkeep; without it memory grew about 7MB a minute when nothing scraped `/metrics`.
- **After a Redis reconnect the subscriber re-subscribes first, then clients get `resync`.** The other order let an event slip through between a client's reload and the subscription coming back.
- **Overload answers 503 with `Retry-After: 1`.** With a 2s acquire timeout plus a 2s statement timeout, a request can still run into the 3s request timeout, so the worst case for an exhausted pool is 3s, not 2s. Most requests get the 503 at 2s.
- **Migrations run on their own connection**, without the 2s `statement_timeout` the pool uses.
- **Migration = design doc §5 + two fixes:** `gen_random_uuid()` instead of `uuid-ossp`, and `judging_queue (room_id, seq_index)` is `DEFERRABLE INITIALLY DEFERRED` so "Swap with On-Deck Team" works in one statement.
- **Static check uses `readelf -d` (no `NEEDED` entries).** Rust's musl target builds static-pie, which `ldd` reports as "statically linked", not "not a dynamic executable".
- **No `.sqlx` folder yet:** nothing uses the `query!` macros. Add `cargo sqlx prepare` output when the first one lands.
- **`/readyz` checks Postgres on its own one-connection pool.** On the shared pool, a burst that filled all 20 connections made every instance report not ready together, and a load balancer could pull them all.
- **Client `x-request-id` values are kept only if short and plain** (64 chars of `A-Za-z0-9-_.:`), because they go into every log line.
- **The subscribe reply is queued before forwarding starts**, so `subscribed` always comes before the topic's first event.
- **Dev builds drop dependency debug info.** `target/` hit 10.5GB and filled the disk; now about 2GB.
- **A publish that times out may still have happened** (seen twice in the soak). Retrying can deliver an event twice with a new `seq`, so event payloads should carry state, not changes. Whoever writes features should follow this.
- **Redis commands time out after 1s.** The client's default is no timeout, so a publish during a Redis outage hung until the request timeout.
- **Bursts don't look like slow clients.** The Redis client's event buffers hold 1024 messages (default 32; 8192 cost 22MB idle because every channel reserves its slots up front), and each connection's send queue is 512 (was 64). Before, a burst of a few dozen events made clients resync. Queued events share their bytes, so a full queue is about 20KB.
- **Startup rejects URLs for the wrong service and port 0.** The Postgres driver ignores the scheme, so `mysql://host/db` connected to whatever Postgres was on that host.
- **Startup gives up on Postgres after 10s.** A host that drops packets held startup for ~20s on Windows and ~2 minutes on Linux.
- **Redis TLS (`rediss://`) isn't supported yet**: it needs fred's TLS feature. Say if managed Redis needs it.
- **Use `127.0.0.1`, not `localhost`, in local URLs.** On Windows, `localhost` tries IPv6 first and takes about 2s to fall back, which is the whole acquire timeout.

## Provides to others
- `GET /healthz`, `GET /readyz` (Postgres + Redis, 500ms; 503 while shutting down). Stable.
- `GET /api/v1/ws`: protocol in `server/README.md`. Topics `room:{id}`, `team:{id}`, `mentor-tag:{tag}`, `user:{id}`. Stable shape; topic list may grow.
- Server code publishes with `state.events.publish(&topic, "type", &payload)`. Stable.
- Handlers should take DB connections through `db::acquire` (feeds `db_pool_waiting`) and return `db::DbError` for 503 handling.
- `x-request-id` on every response. Env vars: `DATABASE_URL`, `REDIS_URL`, `HTTP_PORT`, `METRICS_PORT`, `DB_MAX_CONNECTIONS`, `DB_MIN_CONNECTIONS`, `SHUTDOWN_DELAY_MS`, `RUST_LOG`.

## Needs from others
| For | What's needed | Why / blocks | By |
|---|---|---|---|
| josh | Either add `server/` to `.prettierignore`, or confirm the backend should follow Prettier for its Markdown and YAML. Your CI runs `prettier --check .` on every PR with no path filter, and `server/README.md` fails it, so each backend PR would fail your check once it's on `main`. | Backend PRs passing CI | before PR 1.1 merges |
| infra | Container stop grace period of at least 20s, the load balancer polling `/readyz`, `/api/v1/ws` routed with WebSocket upgrades, port 9090 kept internal | Zero-downtime deploys; 15s drain | before the January mock run |
| database | Schema gaps in design doc §5: a team membership table, `sponsor_keys`, a table for photo face embeddings, and loan history in `hardware_loans` | Later backend PRs (auth, scanning) | 2026-11-24 |
| auth | Replace `auth::AllowAll` with real rules. Browsers can't set headers on a WebSocket, so web uses the cookie and native needs a token another way (query param or first message). Cookie auth also needs an `Origin` check on the upgrade. | Live updates are open to anyone until then | with the auth PR |
| backend (REST) | REST snapshots for rooms, teams and tickets should return the topic's current `seq` (`GET gh:seq:{topic}`), so a client knows where its live stream starts after a reload | Gap detection right after a resync | with the first REST endpoints |

## Done for others
| For | What was done | Link |
|---|---|---|
