//! Shared test setup. Needs the dev databases:
//!   docker compose -f server/docker-compose.yml up -d
//!   DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/postgres REDIS_URL=redis://127.0.0.1:6379 cargo test -p auth
#![allow(dead_code)]

use std::{
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use auth::{layer, policies, routes, AuthState, Codes, JudgeKey, Keys, Role, Sender, Sessions};
use axum::{body::Body, http::Request, middleware::from_fn_with_state, Router};
use ed25519_dalek::SigningKey;
use fred::prelude::{Builder, Client, ClientLike, Config};
use sqlx::{postgres::PgConnectOptions, PgPool};
use tower::ServiceExt;

pub const NOW: i64 = 1_800_000_000;
pub const DAY: i64 = 24 * 60 * 60;
pub const ORIGIN: &str = "https://app.geauxhack.com";

pub fn keys() -> Keys {
    Keys::new(SigningKey::from_bytes(&[1; 32]), vec![])
}

/// K_judge for tests.
pub fn judge_key() -> JudgeKey {
    JudgeKey::new([9; 32])
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
    sqlx::raw_sql(include_str!("../../../migrations/0002_judge_rooms.sql")).execute(&db).await.unwrap();
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
    let client = Builder::from_config(Config::from_url(&url).unwrap())
        .with_performance_config(|c| c.default_command_timeout = Duration::from_secs(1))
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
    state_with(db, redis, now, log_codes())
}

pub fn state_with(db: PgPool, redis: Client, now: fn() -> i64, codes: Codes) -> Arc<AuthState> {
    Arc::new(AuthState { keys: keys(), origin: ORIGIN.into(), now, sessions: Sessions::new(db, redis), policies: policies(), codes, judge: judge_key() })
}

pub fn log_codes() -> Codes {
    Codes::new(Sender::Log, Sender::Log)
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
    call_body(app, method, uri, headers, "").await
}

/// `call` with a request body (set `content-type` yourself).
pub async fn call_body(app: &Router, method: &str, uri: &str, headers: &[(&str, &str)], body: &str) -> Res {
    let mut req = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let res = app.clone().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let headers = res.headers().clone();
    let body = axum::body::to_bytes(res.into_body(), 1 << 16).await.unwrap();
    Res { status, headers, body: String::from_utf8(body.to_vec()).unwrap() }
}

/// Random hex, so emails, phones and IPs don't collide with earlier test runs
/// (rate limits and codes live in Redis for up to an hour).
pub fn random() -> String {
    let mut b = [0u8; 8];
    getrandom::getrandom(&mut b).unwrap();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A stand-in for Postmark and Twilio Verify on a local port. It records every
/// request; Twilio's check approves exactly the code `123456`. Failures on cue:
/// email `bounced-*` (Postmark 422/406), `unconfirmed-*` (422/400), `busy-*` (429);
/// phones `+1500*` (Twilio 400, a landline); check code `999999` (a 200 that isn't JSON).
pub struct Fake {
    pub url: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub headers: axum::http::HeaderMap,
    pub body: String,
}

pub async fn fake_provider() -> Fake {
    use axum::http::{StatusCode, Uri};
    let seen = Arc::new(Mutex::new(vec![]));
    let log = seen.clone();
    let app = Router::new().fallback(move |uri: Uri, headers: axum::http::HeaderMap, body: String| {
        let log = log.clone();
        async move {
            let path = uri.path().to_string();
            log.lock().unwrap().push(Seen { path: path.clone(), headers, body: body.clone() });
            if body.contains("bounced-") {
                (StatusCode::UNPROCESSABLE_ENTITY, r#"{"ErrorCode":406,"Message":"inactive recipient"}"#)
            } else if body.contains("unconfirmed-") {
                (StatusCode::UNPROCESSABLE_ENTITY, r#"{"ErrorCode":400,"Message":"sender signature not confirmed"}"#)
            } else if body.contains("busy-") {
                (StatusCode::TOO_MANY_REQUESTS, "{}")
            } else if body.contains("To=%2B1500") {
                (StatusCode::BAD_REQUEST, r#"{"code":60205}"#)
            } else if body.contains("Code=999999") {
                (StatusCode::OK, "<html>bad gateway</html>")
            } else if body.contains("Code=999998") {
                (StatusCode::NOT_FOUND, r#"{"code":20404}"#)
            } else if body.contains("Code=999997") {
                (StatusCode::TOO_MANY_REQUESTS, r#"{"code":60202}"#)
            } else if path.ends_with("/Verifications") {
                (StatusCode::CREATED, r#"{"status":"pending"}"#)
            } else if path.ends_with("/VerificationCheck") && body.contains("Code=123456") {
                (StatusCode::OK, r#"{"status":"approved"}"#)
            } else if path.ends_with("/VerificationCheck") {
                (StatusCode::OK, r#"{"status":"pending"}"#)
            } else {
                (StatusCode::OK, "{}")
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Fake { url, seen }
}

/// Postmark for email and Twilio Verify for SMS, both pointed at `fake`.
pub fn fake_codes(fake: &Fake) -> Codes {
    Codes::new(
        Sender::Postmark { api: fake.url.clone(), token: "pm-token".into(), from: "codes@geauxhack.com".into() },
        Sender::TwilioVerify {
            api: fake.url.clone(),
            account_sid: "AC123".into(),
            auth_token: "tw-token".into(),
            service_sid: "VA123".into(),
        },
    )
}

/// The code in the last email the fake received.
pub fn last_code(fake: &Fake) -> String {
    let seen = fake.seen.lock().unwrap();
    let email: serde_json::Value = serde_json::from_str(&seen.last().expect("an email was sent").body).unwrap();
    let text = email["TextBody"].as_str().unwrap();
    let start = text.find(|c: char| c.is_ascii_digit()).unwrap();
    text[start..start + 6].to_string()
}
