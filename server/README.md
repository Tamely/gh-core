# gh-core server

Rust (Axum + Tokio) API and WebSocket server.

## Run locally

```sh
cargo run -p gh-server          # http://localhost:8080/healthz
cargo test
cargo clippy --all-targets -- -D warnings
```

## Release image

```sh
docker build -t gh-server .     # static musl binary in an empty image
docker run --rm -p 8080:8080 gh-server
```

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `HTTP_PORT` | `8080` | Public HTTP port |
| `RUST_LOG` | `info` | Log filter, e.g. `gh_server=debug` |

Logs are JSON, one line per event. Every request gets an `x-request-id`
(kept from the client if sent) that appears in its log lines and the response.
