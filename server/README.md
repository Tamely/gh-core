# gh-core server

Rust (Axum + Tokio) API and WebSocket server.

## Run locally

```sh
docker compose up -d                                   # Postgres 16 + pgvector, Redis 7
export DATABASE_URL=postgres://gh:gh@127.0.0.1:5432/gh
cargo run -p gh-server                                 # http://127.0.0.1:8080/healthz
```

Use `127.0.0.1`, not `localhost`: on Windows, `localhost` tries IPv6 first and
takes about 2s to fall back, which is as long as the pool waits for a connection.

## Checks

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test        # integration tests start their own Postgres containers; Docker must be running
```

## Release image

```sh
docker build -t gh-server .     # static musl binary in an empty image
docker run --rm -p 8080:8080 -e DATABASE_URL=... gh-server
```

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `DATABASE_URL` | required | Postgres connection string |
| `DB_MAX_CONNECTIONS` | `20` | Pool size. Two instances run side by side during a deploy, so keep 2 × this well below Postgres's `max_connections` |
| `DB_MIN_CONNECTIONS` | `2` | Connections kept open while idle |
| `HTTP_PORT` | `8080` | Public HTTP port |
| `SHUTDOWN_DELAY_MS` | `5000` | After SIGTERM, how long `/readyz` reports 503 before new connections are refused |
| `RUST_LOG` | `info` | Log filter, e.g. `gh_server=debug` |

## Behaviour

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
  when Postgres answers within 500ms and the server isn't shutting down.
- **SIGTERM:** `/readyz` goes 503, new connections are refused after
  `SHUTDOWN_DELAY_MS`, in-flight requests finish, and the process exits
  within 15s. Give the container a stop timeout above 15s.
