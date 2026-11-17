//! Helpers shared by the integration tests. They start real Postgres
//! containers, so Docker must be running.

#![allow(dead_code)] // each test file uses a different subset

use std::time::Duration;

use axum::{Router, body::Body, http::Request, response::Response};
use gh_server::{AppState, config::Config, db};
use sqlx::PgPool;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

pub struct TestDb {
    pub container: ContainerAsync<Postgres>,
    pub url: String,
}

/// Starts an empty Postgres 16 with pgvector, like production.
pub async fn start_postgres() -> TestDb {
    let container = Postgres::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await
        .expect("start Postgres container (is Docker running?)");
    // 127.0.0.1, not localhost: on Windows, localhost tries IPv6 first and
    // only falls back after about 2s, which eats the whole acquire timeout.
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    TestDb { container, url }
}

pub fn config(url: &str, max_connections: u32) -> Config {
    Config::from_lookup(|name| match name {
        "DATABASE_URL" => Some(url.to_string()),
        "DB_MAX_CONNECTIONS" => Some(max_connections.to_string()),
        "DB_MIN_CONNECTIONS" => Some("1".into()),
        _ => None,
    })
    .unwrap()
}

/// A migrated database and a pool with the production settings.
pub async fn migrated_pool(db: &TestDb, max_connections: u32) -> PgPool {
    db::migrate(&db.url).await.unwrap();
    db::connect(&config(&db.url, max_connections))
        .await
        .unwrap()
}

pub fn state(pool: PgPool) -> AppState {
    AppState {
        db: pool,
        shutdown: CancellationToken::new(),
    }
}

/// Sends one request through the router without opening a socket.
pub async fn send(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Polls `check` until it returns true or `limit` passes.
pub async fn eventually<F, Fut>(limit: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}
