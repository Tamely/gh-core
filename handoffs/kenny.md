# Kenny: handoff

_Last updated: 2026-11-17_

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

PR descriptions are drafted in `pr-drafts/` (not committed).

Measured (server in Docker limited to 2 CPUs, bench in Docker on the same machine, 1,000 clients over 25 rooms, 250 teams, 10 mentor tags, 100 events/s):
- 10-minute run: 0 dropped, 0 errors, 2,175,040/2,175,040 deliveries, 0 `seq` gaps.
- Publish to every subscriber: p99 2.43ms, max 6.1ms.
- Memory: 10.9MB idle 60s after start; 60MB with 1,000 clients, flat for 10 minutes.
- Two instances on one Redis: every event delivered across instances, p99 2.5ms.
- SIGTERM with 1,000 clients: all got 1012, exit 0 after 5.5s.
- Image 11.5MB.

Known: `queue_positions_can_be_swapped_but_not_duplicated` failed once in about 15 full-suite runs on a loaded laptop and never again; the error wasn't captured. Suspect the 2s acquire or statement timeout while many test containers start at once. If CI shows it, the panic message will say which.

Not started: Josh's backend requests (application API, team pick endpoint with the 52% cap, schedule and profile shapes). The live team split needs one more topic kind (e.g. a single `event:split` topic); adding a kind is a few lines in `server/app/src/topic.rs`.

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
| infra | Container stop grace period of at least 20s, the load balancer polling `/readyz`, `/api/v1/ws` routed with WebSocket upgrades, port 9090 kept internal | Zero-downtime deploys; 15s drain | before the January mock run |
| database | Schema gaps in design doc §5: a team membership table, `sponsor_keys`, a table for photo face embeddings, and loan history in `hardware_loans` | Later backend PRs (auth, scanning) | 2026-11-24 |
| auth | Replace `auth::AllowAll` with real rules. Browsers can't set headers on a WebSocket, so web uses the cookie and native needs a token another way (query param or first message). Cookie auth also needs an `Origin` check on the upgrade. | Live updates are open to anyone until then | with the auth PR |
| backend (REST) | REST snapshots for rooms, teams and tickets should return the topic's current `seq` (`GET gh:seq:{topic}`), so a client knows where its live stream starts after a reload | Gap detection right after a resync | with the first REST endpoints |

## Done for others
| For | What was done | Link |
|---|---|---|
