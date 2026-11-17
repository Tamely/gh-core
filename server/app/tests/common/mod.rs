//! Helpers shared by the integration tests. They start real Postgres and Redis
//! containers, so Docker must be running.

#![allow(dead_code)] // each test file uses a different subset

use std::{sync::Arc, time::Duration};

use axum::{Router, body::Body, http::Request, response::Response};
use futures_util::{SinkExt, StreamExt};
use gh_server::{AppState, auth::AllowAll, config::Config, db, events, hub::Hub, ws};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers_modules::{
    postgres::Postgres,
    redis::Redis,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, protocol::CloseFrame},
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

pub struct TestRedis {
    pub container: ContainerAsync<Redis>,
    pub url: String,
}

pub async fn start_redis() -> TestRedis {
    let container = Redis::default()
        .with_tag("7")
        .start()
        .await
        .expect("start Redis container (is Docker running?)");
    let port = container.get_host_port_ipv4(6379).await.unwrap();
    TestRedis {
        container,
        url: format!("redis://127.0.0.1:{port}"),
    }
}

pub fn config(url: &str, max_connections: u32) -> Config {
    Config::from_lookup(|name| match name {
        "DATABASE_URL" => Some(url.to_string()),
        "REDIS_URL" => Some("redis://unused".into()),
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

/// A migrated database and a pool without the production timeouts, for tests
/// about the schema rather than the pool. A busy machine starting many test
/// containers can push a query past the 2s limit and make them flaky.
pub async fn schema_pool(db: &TestDb) -> PgPool {
    db::migrate(&db.url).await.unwrap();
    PgPool::connect(&db.url).await.unwrap()
}

/// A pool that never connects; fine for tests that don't touch Postgres.
pub fn offline_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(200))
        .connect_lazy("postgres://nobody@127.0.0.1:1/none")
        .unwrap()
}

/// A full app state, as one server instance would have it.
pub async fn state(pool: PgPool, redis_url: &str) -> AppState {
    let hub = Arc::new(Hub::default());
    let (events, subscriber) = events::connect(redis_url, hub.clone()).await.unwrap();
    // The subscriber stops when dropped; keep it for the life of the test.
    std::mem::forget(subscriber);
    AppState {
        db: pool,
        events,
        hub,
        authorizer: Arc::new(AllowAll),
        ws: ws::Settings::default(),
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

/// Serves `app` on a random local port. Returns its `host:port`.
pub async fn serve_on_random_port(app: Router, shutdown: CancellationToken) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(gh_server::serve(
        listener,
        app,
        shutdown,
        Duration::from_millis(300),
        Duration::from_secs(15),
    ));
    addr
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

/// A WebSocket client for tests.
pub struct Client {
    pub ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

pub const RECV_TIMEOUT: Duration = Duration::from_secs(5);

impl Client {
    pub async fn connect(addr: &str) -> Client {
        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/ws"))
            .await
            .expect("WebSocket connect");
        Client { ws }
    }

    /// Connects with a tiny receive buffer, so a client that stops reading
    /// backs up quickly.
    pub async fn connect_small_buffer(addr: &str) -> Client {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.set_recv_buffer_size(4096).unwrap();
        let stream = socket.connect(addr.parse().unwrap()).await.unwrap();
        let (ws, _) = tokio_tungstenite::client_async(
            format!("ws://{addr}/api/v1/ws"),
            MaybeTlsStream::Plain(stream),
        )
        .await
        .unwrap();
        Client { ws }
    }

    pub async fn send_text(&mut self, text: &str) {
        self.ws.send(Message::text(text)).await.unwrap();
    }

    pub async fn command(&mut self, op: &str, topic: &str) -> Value {
        self.send_text(&serde_json::json!({ "op": op, "topic": topic }).to_string())
            .await;
        self.next_json().await
    }

    pub async fn subscribe(&mut self, topic: &str) {
        let reply = self.command("subscribe", topic).await;
        assert_eq!(reply["type"], "subscribed", "{reply}");
        assert_eq!(reply["topic"], topic);
    }

    /// Next text message as JSON. Panics on close or after `RECV_TIMEOUT`.
    pub async fn next_json(&mut self) -> Value {
        loop {
            let message = tokio::time::timeout(RECV_TIMEOUT, self.ws.next())
                .await
                .expect("timed out waiting for a message")
                .expect("connection ended")
                .expect("read error");
            match message {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Ping(_) | Message::Pong(_) => continue,
                other => panic!("expected text, got {other:?}"),
            }
        }
    }

    /// True if no text or close arrives within `wait` (pings don't count).
    pub async fn is_quiet(&mut self, wait: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            match tokio::time::timeout_at(deadline, self.ws.next()).await {
                Err(_) => return true,
                Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
                Ok(_) => return false,
            }
        }
    }

    /// Reads until the server closes. Returns every text message seen first
    /// and the close code (None if the socket ended without a close frame).
    pub async fn read_until_close(&mut self) -> (Vec<Value>, Option<u16>) {
        let mut texts = Vec::new();
        loop {
            match tokio::time::timeout(Duration::from_secs(10), self.ws.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    texts.push(serde_json::from_str(&text).unwrap())
                }
                Ok(Some(Ok(Message::Close(frame)))) => {
                    return (texts, frame.map(|f: CloseFrame| u16::from(f.code)));
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(_)) | None) => return (texts, None),
                Err(_) => panic!("server never closed the connection"),
            }
        }
    }
}
