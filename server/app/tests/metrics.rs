//! `/metrics` lists every required series and lives only on the internal router.
//! Separate test binary because the metrics recorder is process-wide.

mod common;

use std::time::Duration;

use axum::{Router, http::StatusCode};
use common::*;
use gh_server::{app, metrics, topic::Topic};
use serde_json::json;
use testcontainers_modules::testcontainers::core::ExecCommand;

async fn page(internal: &Router) -> String {
    let res = send(internal, "/metrics").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

#[tokio::test]
async fn metrics_cover_http_pool_websockets_and_redis() {
    let handle = metrics::install();
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let state = state(migrated_pool(&db, 2).await, &redis.url).await;
    let public = app(state.clone());
    let internal = metrics::router(handle, state.db.clone());

    // Some traffic: an HTTP request, a WebSocket client receiving an event.
    assert_eq!(send(&public, "/healthz").await.status(), StatusCode::OK);
    let addr = serve_on_random_port(public.clone(), state.shutdown.clone()).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("room:1").await;
    state
        .events
        .publish(&Topic::parse("room:1").unwrap(), "t", &json!(null))
        .await
        .unwrap();
    client.next_json().await;

    // The public router has no /metrics.
    assert_eq!(
        send(&public, "/metrics").await.status(),
        StatusCode::NOT_FOUND
    );

    // Unknown paths share one label instead of adding a series per URL.
    send(&public, "/rooms/12345").await;
    send(&public, "/rooms/67890").await;

    let text = page(&internal).await;

    for expected in [
        r#"http_request_duration_seconds_bucket{route="/healthz",status="200""#,
        "db_pool_connections ",
        "db_pool_idle_connections ",
        "db_pool_waiting ",
        "ws_connections 1",
        r#"ws_subscriptions{kind="room"} 1"#,
        r#"ws_subscriptions{kind="mentor-tag"} 0"#,
        "ws_messages_sent_total 1",
        "ws_resyncs_total 0",
        "ws_slow_client_disconnects_total 0",
        "redis_reconnects_total 0",
        "redis_connected 1",
        "event_delivery_seconds_bucket",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
    }
    assert!(text.contains(r#"route="unmatched",status="404""#), "{text}");
    assert!(!text.contains("12345") && !text.contains("67890"), "{text}");

    // A dropped Redis subscriber shows up as a reconnect and a resync.
    redis
        .container
        .exec(ExecCommand::new([
            "redis-cli",
            "CLIENT",
            "KILL",
            "TYPE",
            "pubsub",
        ]))
        .await
        .unwrap();
    assert_eq!(client.next_json().await["type"], "resync");
    let text = page(&internal).await;
    assert!(text.contains("redis_reconnects_total 1"), "{text}");
    assert!(text.contains("ws_resyncs_total 1"), "{text}");
    assert!(text.contains("redis_connected 1"), "{text}");

    // When the client leaves, the gauges go back to zero.
    drop(client);
    let settled = eventually(Duration::from_secs(5), || async {
        let text = page(&internal).await;
        text.contains("ws_connections 0") && text.contains(r#"ws_subscriptions{kind="room"} 0"#)
    })
    .await;
    assert!(
        settled,
        "gauges didn't return to zero:\n{}",
        page(&internal).await
    );
}
