# Kenny: handoff

_Last updated: 2026-11-17_

## Owns
**Backend monolith (Rust, Axum)** in `server/`: HTTP API, Postgres pool and migrations, Redis pub/sub, WebSocket hub, health and metrics, the static release image and its CI.

## Status
Step 1 (server skeleton) is on branch `kenny/server-skeleton`.

### PR plan
| PR | Scope | Blocked on |
|---|---|---|
| A. Skeleton | Cargo workspace, env config with startup validation, JSON logs with request IDs, 3s request timeout, `/healthz`, SIGTERM drain (15s cap), static musl image `FROM scratch`, CI (fmt, clippy, test, static check) | none |
| B. Database | `docker-compose.yml` (pgvector pg16, redis 7), sqlx pool (max 20, min 2, 2s acquire, 503 + `Retry-After` when exhausted), per-connection `statement_timeout` 2s and `idle_in_transaction_session_timeout` 5s, migrations at startup, `/readyz` | schema fixes below |
| C. Live updates | Redis subscriber with reconnect backoff, WebSocket hub at `/api/v1/ws` (topics, `seq`, `resync`, 1013/1012 close codes), `/metrics` on port 9090, load tool in `server/bench/` | none |
| D. Benchmarks | 1,000-connection run, latency and memory numbers, SIGTERM under load | C |

## Decisions
- **Static check uses `readelf -d` (no `NEEDED` entries)**, not `ldd`'s wording. Rust's musl target builds static-pie, which `ldd` reports as "statically linked".
- **Request timeout returns 503**, not tower-http's default 408.
- **Drain cap is 15s.** Whatever runs the container must allow more than that before SIGKILL (Docker's default `stop` timeout is 10s).
- **The image is `FROM scratch` with only the binary.** Measured: image 3.5MB, binary 2.3MB, idle memory 9MB, `docker stop` exits in about 1s.
- **`seq` will be assigned once at publish time**, not per server instance, so it stays consistent when a client reconnects to another instance.

## Provides to others
- `GET /healthz`: 200 while the process is up. Stable.
- Every response has an `x-request-id` header; a client-sent one is kept. Stable.
- Env vars: `HTTP_PORT` (default 8080), `RUST_LOG`. More arrive with PR B.

## Needs from others
| For | What's needed | Why / blocks | By |
|---|---|---|---|
| infra | Container stop grace period of at least 20s, and the load balancer polling `/readyz` | Zero-downtime deploys; 15s drain | before the January mock run |
| database | Schema gaps in design doc §5: a team membership table, `sponsor_keys`, a table for photo face embeddings, `DEFERRABLE` on `judging_queue (room_id, seq_index)` so queue swaps work, and loan history in `hardware_loans` | PR B's first migration | 2026-11-20 |

## Done for others
| For | What was done | Link |
|---|---|---|
