# gh-core server

Rust (Axum + Tokio) API and WebSocket server.

## Run locally

```sh
docker compose up -d                                   # Postgres 16 + pgvector, Redis 7
export DATABASE_URL=postgres://gh:gh@127.0.0.1:5432/gh
export REDIS_URL=redis://127.0.0.1:6379
cargo run -p gh-server                                 # http://127.0.0.1:8080/healthz
```

Use `127.0.0.1`, not `localhost`: on Windows, `localhost` tries IPv6 first and
takes about 2s to fall back, which is as long as the pool waits for a connection.

## Checks

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test        # integration tests start their own Postgres and Redis containers; Docker must be running
```

## Release image

```sh
docker build -t gh-server .     # static musl binary in an empty image
docker run --rm -p 8080:8080 -e DATABASE_URL=... -e REDIS_URL=... gh-server
```

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `DATABASE_URL` | required | Postgres connection string (`postgres://` or `postgresql://`) |
| `DB_MAX_CONNECTIONS` | `20` | Pool size. Each instance also keeps 1 connection for `/readyz`, and two instances run side by side during a deploy, so keep 2 × (this + 1) well below Postgres's `max_connections` |
| `DB_MIN_CONNECTIONS` | `2` | Connections kept open while idle |
| `REDIS_URL` | required | Redis connection string (`redis://`; TLS isn't enabled yet) |
| `HTTP_PORT` | `8080` | Public HTTP port |
| `METRICS_PORT` | `9090` | Internal port for `/metrics`. Don't expose it publicly |
| `SHUTDOWN_DELAY_MS` | `5000` | After SIGTERM, how long `/readyz` reports 503 before new connections are refused |
| `RUST_LOG` | `info` | Log filter, e.g. `gh_server=debug` |

## Behaviour

- **Startup** fails with exit code 1 and one log line per problem if any
  setting is missing or wrong, if Postgres doesn't answer within 10s, if
  Redis can't be reached, or if a port is taken.

- **Logs** are JSON, one line per event. Every request gets an `x-request-id`
  (kept from the client if sent) that appears in its log lines and the response.
- **Timeouts:** every request is cut off at 3s. Each database connection runs
  with `statement_timeout = 2s` and `idle_in_transaction_session_timeout = 5s`.
  A request waits at most 2s for a free connection.
- **Overload** answers `503` with `Retry-After: 1` instead of hanging. When the
  pool is exhausted, most requests get it at 2s and the worst case is 3s.
- **Migrations** in `app/migrations/` run at startup on their own connection
  (no 2s limit). Concurrent starts are safe: sqlx takes an advisory lock.
  While an old instance is still running, a migration must stay compatible
  with the old code (add columns and tables; drop them in a later deploy).
- **Health:** `/healthz` is 200 while the process runs. `/readyz` is 200 only
  when Postgres and Redis both answer within 500ms and the server isn't
  shutting down. It checks Postgres on its own connection, so a full pool
  doesn't make the instance look broken (overload is answered with 503s on
  the requests themselves).
- **Request IDs** from clients are kept only if they're up to 64 characters
  of letters, digits and `-_.:`; otherwise a UUID is used.
- **SIGTERM:** `/readyz` goes 503, WebSocket clients are closed with 1012 and
  new upgrades get 503, new connections are refused after
  `SHUTDOWN_DELAY_MS`, in-flight requests finish, and the process exits
  within 15s. Give the container a stop timeout above 15s.

## Notify sign-ups

`POST /api/v1/notify` with `{"email": "..."}` stores an address for the
"registration is open" email (table `notify_signups`). Answers:

- `204` stored, or already on the list (the two look the same, so the form
  can't reveal who signed up)
- `400` not shaped like an address, or not JSON; `422` no `email` field;
  `413` body over 1KB; `415` not `application/json`
- `429` the client added 10 new addresses in the last hour

Browsers may call it only from `https://geauxhack.com` (CORS). The limit is
per `X-Real-IP`, which the reverse proxy must set (overwriting any value the
client sent). Requests without it share one bucket.

## Live updates

`GET /api/v1/ws`. Clients send `{"op":"subscribe","topic":"room:PFT-1200"}`
(or `unsubscribe`). Topics: `room:{id}`, `team:{id}`, `mentor-tag:{tag}`,
`user:{id}`.

Events arrive as `{"topic","seq","type","payload"}`. `seq` goes up by exactly
one per topic, the same on every instance. On a gap, or on
`{"type":"resync","topic"}`, reload that topic over REST; events aren't
replayed. The `subscribed` reply always arrives before the topic's first
event. If Redis restarts without persistence, `seq` starts again at 1; clients
get `resync` first, and the jump reads as a gap anyway.

Server code publishes with `state.events.publish(&topic, "type", &payload)`.
A publish that times out may still have gone through, so a retry can deliver
an event twice (with a new `seq`). Make payloads carry state ("room is now on
team 7"), not changes ("advance by one").
Every event goes through Redis (`gh:events:{topic}`), and every instance
forwards it to its own clients, so it doesn't matter which instance a client
is on. A Lua script bumps `gh:seq:{topic}` and publishes in one step.

Limits: messages up to 4KB, 50 subscriptions per connection, a ping every 25s
(2 missed pongs disconnect). Each connection has a 512-message send queue: at
half full the client gets `resync`, when full it's closed with 1013. A slow
client never delays anyone else. After a Redis reconnect every client gets
`resync`, because events published during the outage are lost.

Close codes: 1000 normal, 1001 ping timeout, 1003 binary message, 1008 bad
frame or message over 4KB, 1012 server restarting, 1013 too slow.

## Metrics (`:9090/metrics`)

`http_request_duration_seconds{route,status}`, `db_pool_connections`,
`db_pool_idle_connections`, `db_pool_waiting`, `ws_connections`,
`ws_subscriptions{kind}`, `ws_messages_sent_total`, `ws_resyncs_total`,
`ws_slow_client_disconnects_total`, `redis_connected`,
`redis_reconnects_total`, `redis_outage_seconds`, `event_delivery_seconds`
(publish to WebSocket write).

`db_pool_waiting` counts requests inside `db::acquire`, so handlers should
take connections through it.

## Load test (`bench/`)

`gh-bench` opens WebSocket clients spread over rooms, teams, mentor tags and
one user topic each, publishes events through Redis at a fixed rate, and fails
unless every client got every event in order, nobody was dropped, and the p99
time from publish to reaching every subscriber is under `--p99-ms` (default 5).
Run it on the same machine as the server so both use one clock:

```sh
docker compose up -d
docker build -t gh-server . && docker build --target bench -t gh-bench .
docker run -d --name gh --cpus=2 --network server_default   -e DATABASE_URL=postgres://gh:gh@postgres:5432/gh -e REDIS_URL=redis://redis:6379 gh-server
docker run --rm --network server_default gh-bench   --url ws://gh:8080/api/v1/ws --redis redis://redis:6379 --clients 1000 --rate 100 --duration 600
```

Repeat `--url` to spread clients over several instances. Watch server memory
with `docker stats gh` while it runs.

Slow-client check: `--slow 1 --pad 1024 --metrics http://gh:9090/metrics`
adds a client subscribed to every room and mentor tag that never reads. It
passes only if the server cuts that client off for being slow and the other
clients still meet the latency target.

### Resilience runs

All with the server limited to 2 CPUs and the bench in Docker on the same host.

**Rolling deploy.** Clients get `--url` for both instances and `--reconnect`, then one instance is stopped while the other runs:

```sh
docker run --rm --network server_default gh-bench --url ws://gh-a:8080/api/v1/ws --url ws://gh-b:8080/api/v1/ws --reconnect --redis redis://redis:6379 --duration 160
# meanwhile: start gh-b, docker stop gh-a, start gh-a, docker stop gh-b
```

Measured (1,000 clients, A to B and back): 2,000 reconnects, all 1,000 connected at the end, 0 unannounced seq gaps, 0 clients refused during a drain, 0 to 2s without updates per move (the reconnect jitter), p99 4.1ms throughout.

**Redis and Postgres restarts.** `docker restart server-redis-1`, later `docker restart server-postgres-1`, under 1,000 clients with `--reconnect`: no client disconnected. Redis was down 1.3s and every one of the 4,000 subscriptions got one `resync`. `/readyz` was 503 for about 1.2s each time, `/healthz` stayed 200. Events published during the Redis outage wait up to 1s in the client's retry queue, so latency for that window reads up to 0.9s.

**Bad Wi-Fi and frozen phones.** Delay and loss come from `tc netem`, run from a helper container that shares the bench's (and, for the return path, the server's) network:

```sh
docker run --rm --network container:bench-bad --cap-add NET_ADMIN alpine:3.20 sh -c "apk add -q iproute2 && tc qdisc add dev eth0 root netem delay 150ms 50ms loss 5%"
```

Measured: 300 clients at ~300ms round trip and 5% loss each way, 700 normal clients, 50 frozen clients (`--slow 50`), separate topics via `--prefix`. Normal clients p99 3.2ms with every event delivered; bad-Wi-Fi clients all stayed connected and got every event (p50 0.36s, p99 1.7s); all 50 frozen clients were cut off for being slow.

**One-hour soak.** 1,000 clients, 100 events/s, 60 minutes: 0 dropped, 0 gaps, 0 resyncs, 13,049,940 messages. Memory 65-68MB and open sockets steady at 1,016 the whole hour, back to 16 sockets after. p99 to every subscriber was 6.2ms over the hour (2.4ms over 10 minutes): the slow stretches coincide with moments when the bench, in a different container, also stalled for about a second, which points at the laptop's Docker VM rather than the server. Two publishes timed out during those stalls and were delivered anyway, which is why publishes must be safe to repeat.
