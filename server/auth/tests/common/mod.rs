//! Shared test setup. Needs the dev databases:
//!   docker compose -f server/docker-compose.yml up -d
//!   DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/postgres REDIS_URL=redis://127.0.0.1:6379 cargo test -p auth
#![allow(dead_code)]

use std::{str::FromStr, sync::Arc, time::Duration};

use auth::{layer, policies, routes, AuthState, Keys, Role, Sessions};
use axum::{body::Body, http::Request, middleware::from_fn_with_state, Router};
use ed25519_dalek::SigningKey;
use fred::prelude::{Builder, Client, ClientLike, Config};
use sqlx::{postgres::PgConnectOptions, PgPool};
use tower::ServiceExt;

pub const NOW: i64 = 1_800_000_000;
pub const DAY: i64 = 24 * 60 * 60;
pub const ORIGIN: &str = "https://app.geauxhack.org";

pub fn keys() -> Keys {
    Keys::new(SigningKey::from_bytes(&[1; 32]), vec![])
}

/// A fresh schema with the migration loaded; the pool only sees that schema.
pub async fn db(schema: &str) -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("set DATABASE_URL (see tests/common/mod.rs)");
    let admin = PgPool::connect(&url).await.unwrap();
    sqlx::raw_sql(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema};"))
        .execute(&admin)
        .await
        .unwrap();
    let opts = PgConnectOptions::from_str(&url).unwrap().options([("search_path", schema)]);
    let db = PgPool::connect_with(opts).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/0001_users_and_auth.sql")).execute(&db).await.unwrap();
    db
}

/// A pool that never connects: Postgres is "down".
pub fn dead_db() -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(300))
        .connect_lazy("postgres://postgres:postgres@127.0.0.1:1/postgres")
        .unwrap()
}

pub async fn redis() -> Client {
    let url = std::env::var("REDIS_URL").unwrap_or("redis://127.0.0.1:6379".into());
    // Generous: Docker Desktop on a Mac can stall a command past 1s under load.
    let client = Builder::from_config(Config::from_url(&url).unwrap())
        .with_performance_config(|c| c.default_command_timeout = Duration::from_secs(5))
        .build()
        .unwrap();
    client.init().await.unwrap();
    client
}

/// A client whose server isn't there: Redis is "down".
pub async fn dead_redis() -> Client {
    let client = Builder::from_config(Config::from_url("redis://127.0.0.1:1").unwrap())
        .with_performance_config(|c| c.default_command_timeout = Duration::from_millis(300))
        .build()
        .unwrap();
    let _ = client.init().await;
    client
}

pub async fn user(db: &PgPool, email: &str, roles: &[Role]) -> String {
    let id: String = sqlx::query_scalar("INSERT INTO users (email) VALUES ($1) RETURNING id::text")
        .bind(email)
        .fetch_one(db)
        .await
        .unwrap();
    for role in roles {
        let role = serde_json::to_value(role).unwrap();
        sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ($1::uuid, $2)")
            .bind(&id)
            .bind(role.as_str().unwrap())
            .execute(db)
            .await
            .unwrap();
    }
    id
}

pub fn state(db: PgPool, redis: Client, now: fn() -> i64) -> Arc<AuthState> {
    Arc::new(AuthState { keys: keys(), origin: ORIGIN.into(), now, sessions: Sessions::new(db, redis), policies: policies() })
}

/// Auth's routes plus `extra`, under the auth layer, like the real server.
pub fn app(state: Arc<AuthState>, extra: Router) -> Router {
    routes(state.clone()).merge(extra).layer(from_fn_with_state(state, layer))
}

pub struct Res {
    pub status: u16,
    pub headers: axum::http::HeaderMap,
    pub body: String,
}

pub async fn call(app: &Router, method: &str, uri: &str, headers: &[(&str, &str)]) -> Res {
    let mut req = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let res = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let headers = res.headers().clone();
    let body = axum::body::to_bytes(res.into_body(), 1 << 16).await.unwrap();
    Res { status, headers, body: String::from_utf8(body.to_vec()).unwrap() }
}
