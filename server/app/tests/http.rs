//! HTTP behaviour that doesn't need a database: request IDs, the request
//! timeout, and graceful shutdown.

mod common;

use std::time::{Duration, Instant};

use axum::{
    Router,
    http::{StatusCode, header::RETRY_AFTER},
    routing::get,
};

use common::{TestRedis, offline_pool, send, start_redis, state};
use gh_server::{AppState, routes, serve, with_middleware};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

/// State whose Postgres pool never connects; fine for routes that don't use it.
async fn offline_state() -> (AppState, TestRedis) {
    let redis = start_redis().await;
    (state(offline_pool(), &redis.url).await, redis)
}

fn app_with_sleep(state: AppState) -> Router {
    with_middleware(
        routes()
            .route(
                "/test/sleep/{ms}",
                get(
                    |axum::extract::Path(ms): axum::extract::Path<u64>| async move {
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                        StatusCode::OK
                    },
                ),
            )
            .with_state(state),
    )
}

#[tokio::test]
async fn generates_request_id_or_keeps_the_clients() {
    let (state, _redis) = offline_state().await;
    let app = app_with_sleep(state);
    let res = send(&app, "/healthz").await;
    assert_eq!(res.status(), StatusCode::OK);
    let id = res.headers()["x-request-id"].to_str().unwrap();
    assert_eq!(id.len(), 36, "expected a UUID, got {id}");

    use tower::ServiceExt;
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::get("/healthz")
                .header("x-request-id", "client-chosen")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.headers()["x-request-id"], "client-chosen");

    // IDs that are too long or could mess with logs are replaced.
    for bad in [
        "x".repeat(65),
        "has space".into(),
        "quote\"".into(),
        String::new(),
    ] {
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::get("/healthz")
                    .header("x-request-id", &bad)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let id = res.headers()["x-request-id"].to_str().unwrap();
        assert_eq!(id.len(), 36, "{bad:?} should be replaced, got {id}");
    }
}

#[tokio::test]
async fn request_over_three_seconds_gets_503_with_retry_after() {
    let (state, _redis) = offline_state().await;
    let app = app_with_sleep(state);
    let started = Instant::now();
    let res = send(&app, "/test/sleep/10000").await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.headers()[RETRY_AFTER], "1");
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(3) && took < Duration::from_millis(3500),
        "{took:?}"
    );
}

#[tokio::test]
async fn unknown_route_is_404() {
    let (state, _redis) = offline_state().await;
    let app = app_with_sleep(state);
    assert_eq!(send(&app, "/nope").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn readyz_is_503_once_shutdown_starts() {
    let (state, _redis) = offline_state().await;
    let shutdown = state.shutdown.clone();
    let app = app_with_sleep(state);
    shutdown.cancel();
    assert_eq!(
        send(&app, "/readyz").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(send(&app, "/healthz").await.status(), StatusCode::OK);
}

struct Running {
    url: String,
    shutdown: CancellationToken,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    _redis: TestRedis,
}

async fn start(delay: Duration, drain_limit: Duration) -> Running {
    let (state, redis) = offline_state().await;
    let shutdown = state.shutdown.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(serve(
        listener,
        app_with_sleep(state),
        shutdown.clone(),
        delay,
        drain_limit,
    ));
    Running {
        url,
        shutdown,
        server,
        _redis: redis,
    }
}

#[tokio::test]
async fn shutdown_finishes_in_flight_requests_then_stops_accepting() {
    let running = start(Duration::from_millis(300), Duration::from_secs(15)).await;
    let client = reqwest::Client::new();

    let in_flight = tokio::spawn({
        let url = format!("{}/test/sleep/1000", running.url);
        let client = client.clone();
        async move { client.get(url).send().await.map(|r| r.status()) }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = Instant::now();
    running.shutdown.cancel();

    // During the delay the instance still accepts, but reports not ready.
    let ready = client
        .get(format!("{}/readyz", running.url))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);

    assert_eq!(in_flight.await.unwrap().unwrap(), StatusCode::OK);
    running.server.await.unwrap().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );

    // After the delay, new connections are refused.
    let fresh = reqwest::Client::new();
    assert!(
        fresh
            .get(format!("{}/healthz", running.url))
            .send()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn shutdown_gives_up_on_requests_that_outlive_the_drain_limit() {
    let running = start(Duration::ZERO, Duration::from_millis(500)).await;
    let stuck = tokio::spawn({
        // The 3s request timeout would end this first, so use 2.5s and a
        // drain limit well under it.
        let url = format!("{}/test/sleep/2500", running.url);
        async move { reqwest::get(url).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let started = Instant::now();
    running.shutdown.cancel();
    running.server.await.unwrap().unwrap();
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(500) && took < Duration::from_millis(1500),
        "{took:?}"
    );
    stuck.abort();
}
