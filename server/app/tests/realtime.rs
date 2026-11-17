//! WebSocket hub and Redis fan-out, end to end over real sockets.

mod common;

use std::{sync::Arc, time::Duration};

use axum::http::{HeaderMap, StatusCode};
use common::*;
use gh_server::{
    AppState, app,
    auth::{Authorizer, Identity},
    events::now_us,
    topic::{Topic, TopicKind},
    ws::{self, MAX_SUBSCRIPTIONS},
};
use serde_json::{Value, json};
use testcontainers_modules::testcontainers::core::ExecCommand;

/// One server instance on a random port, sharing `redis` with any others.
async fn instance(redis: &TestRedis) -> (String, AppState) {
    instance_with(redis, |_| {}).await
}

async fn instance_with(redis: &TestRedis, tweak: impl FnOnce(&mut AppState)) -> (String, AppState) {
    let mut state = state(offline_pool(), &redis.url).await;
    tweak(&mut state);
    let addr = serve_on_random_port(app(state.clone()), state.shutdown.clone()).await;
    (addr, state)
}

async fn publish(state: &AppState, topic: &str, kind: &str, payload: Value) -> u64 {
    state
        .events
        .publish(&Topic::parse(topic).unwrap(), kind, &payload)
        .await
        .unwrap()
}

#[tokio::test]
async fn subscriber_receives_events_with_increasing_seq() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("room:PFT-1200").await;

    assert_eq!(
        publish(&state, "room:PFT-1200", "queue.advanced", json!({"k": 1})).await,
        1
    );
    assert_eq!(
        publish(&state, "room:PFT-1200", "queue.advanced", json!({"k": 2})).await,
        2
    );

    assert_eq!(
        client.next_json().await,
        json!({"topic": "room:PFT-1200", "seq": 1, "type": "queue.advanced", "payload": {"k": 1}})
    );
    assert_eq!(client.next_json().await["seq"], 2);
}

#[tokio::test]
async fn seq_counts_separately_per_topic() {
    let redis = start_redis().await;
    let (_, state) = instance(&redis).await;
    assert_eq!(publish(&state, "room:a", "t", json!(null)).await, 1);
    assert_eq!(publish(&state, "room:a", "t", json!(null)).await, 2);
    assert_eq!(publish(&state, "room:b", "t", json!(null)).await, 1);
    assert_eq!(publish(&state, "team:a", "t", json!(null)).await, 1);
}

#[tokio::test]
async fn topics_are_isolated_and_unsubscribe_stops_delivery() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("team:1").await;

    publish(&state, "team:2", "t", json!("other team")).await;
    publish(&state, "user:1", "t", json!("same id, other kind")).await;
    publish(&state, "team:1", "t", json!("mine")).await;
    assert_eq!(client.next_json().await["payload"], "mine");

    let reply = client.command("unsubscribe", "team:1").await;
    assert_eq!(reply, json!({"type": "unsubscribed", "topic": "team:1"}));
    publish(&state, "team:1", "t", json!("after unsubscribe")).await;
    assert!(client.is_quiet(Duration::from_millis(500)).await);

    // The topic is forgotten once nobody on this instance subscribes.
    assert!(
        eventually(Duration::from_secs(2), || async {
            state.hub.topic_count() == 0
        })
        .await
    );
}

#[tokio::test]
async fn subscribing_twice_delivers_once() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("mentor-tag:C++").await;
    client.subscribe("mentor-tag:C++").await;

    publish(&state, "mentor-tag:C++", "ticket.opened", json!(1)).await;
    assert_eq!(client.next_json().await["seq"], 1);
    assert!(client.is_quiet(Duration::from_millis(500)).await);
}

#[tokio::test]
async fn event_published_on_one_instance_reaches_clients_on_another() {
    let redis = start_redis().await;
    let (addr_a, state_a) = instance(&redis).await;
    let (addr_b, state_b) = instance(&redis).await;
    let mut on_a = Client::connect(&addr_a).await;
    let mut on_b = Client::connect(&addr_b).await;
    on_a.subscribe("room:9").await;
    on_b.subscribe("room:9").await;

    publish(&state_a, "room:9", "queue.advanced", json!("from A")).await;
    publish(&state_b, "room:9", "queue.advanced", json!("from B")).await;

    // Both clients see the same events with the same seq, whichever
    // instance published them.
    for client in [&mut on_a, &mut on_b] {
        let first = client.next_json().await;
        let second = client.next_json().await;
        assert_eq!(
            (first["seq"].clone(), first["payload"].clone()),
            (json!(1), json!("from A"))
        );
        assert_eq!(
            (second["seq"].clone(), second["payload"].clone()),
            (json!(2), json!("from B"))
        );
    }
}

#[tokio::test]
async fn concurrent_publishers_never_reorder_seq() {
    const PER_PUBLISHER: u64 = 200;
    let redis = start_redis().await;
    let (addr, state_a) = instance(&redis).await;
    let (_, state_b) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("room:race").await;

    let spam = |state: AppState| async move {
        for _ in 0..PER_PUBLISHER {
            publish(&state, "room:race", "t", json!(null)).await;
        }
    };
    tokio::join!(spam(state_a), spam(state_b));

    for expected in 1..=PER_PUBLISHER * 2 {
        assert_eq!(client.next_json().await["seq"], expected);
    }
}

#[tokio::test]
async fn bad_requests_get_errors_and_the_connection_stays_open() {
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;

    client.send_text("not json").await;
    assert_eq!(client.next_json().await["type"], "error");

    client.send_text(r#"{"op":"dance","topic":"room:1"}"#).await;
    assert_eq!(client.next_json().await["type"], "error");

    client.send_text(r#"{"op":"subscribe"}"#).await;
    assert_eq!(client.next_json().await["type"], "error");

    for topic in ["room", "lobby:1", "room:has space", "room:"] {
        let reply = client.command("subscribe", topic).await;
        assert_eq!(reply["type"], "error", "{topic}: {reply}");
        assert_eq!(reply["topic"], topic);
        assert!(reply["payload"]["reason"].is_string());
    }

    client.subscribe("room:1").await;
}

#[tokio::test]
async fn binary_message_closes_with_1003() {
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    use futures_util::SinkExt;
    client
        .ws
        .send(tokio_tungstenite::tungstenite::Message::binary(vec![
            1, 2, 3,
        ]))
        .await
        .unwrap();
    assert_eq!(client.read_until_close().await.1, Some(1003));
}

#[tokio::test]
async fn message_over_4kb_disconnects() {
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;

    // Exactly at the limit is fine.
    let padding =
        " ".repeat(ws::MAX_MESSAGE_BYTES - r#"{"op":"subscribe","topic":"room:1"}"#.len());
    client
        .send_text(&format!(
            r#"{{"op":"subscribe","topic":"room:1"}}{padding}"#
        ))
        .await;
    assert_eq!(client.next_json().await["type"], "subscribed");

    client
        .send_text(&"x".repeat(ws::MAX_MESSAGE_BYTES + 1))
        .await;
    let (_, code) = client.read_until_close().await;
    assert_ne!(
        code,
        Some(1000),
        "an oversized message must not look like a normal close"
    );
}

#[tokio::test]
async fn subscription_limit_is_enforced() {
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    for i in 0..MAX_SUBSCRIPTIONS {
        client.subscribe(&format!("user:{i}")).await;
    }
    // Re-subscribing to one you have doesn't count against the limit.
    client.subscribe("user:0").await;

    let reply = client.command("subscribe", "user:overflow").await;
    assert_eq!(reply["type"], "error");
    assert_eq!(reply["payload"]["reason"], "subscription limit reached");

    // Freeing one makes room again.
    client.command("unsubscribe", "user:0").await;
    client.subscribe("user:overflow").await;
}

struct RoomsOnly;

impl Authorizer for RoomsOnly {
    fn identify(&self, headers: &HeaderMap) -> Option<Identity> {
        (!headers.contains_key("x-reject")).then(Identity::default)
    }

    fn can_subscribe(&self, _: &Identity, topic: &Topic) -> bool {
        topic.kind() == TopicKind::Room
    }
}

#[tokio::test]
async fn authorizer_decides_who_connects_and_what_they_subscribe_to() {
    let redis = start_redis().await;
    let (addr, _) = instance_with(&redis, |s| s.authorizer = Arc::new(RoomsOnly)).await;

    let mut client = Client::connect(&addr).await;
    client.subscribe("room:1").await;
    let reply = client.command("subscribe", "user:1").await;
    assert_eq!(reply["payload"]["reason"], "not allowed");

    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = format!("ws://{addr}/api/v1/ws")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("x-reject", "1".parse().unwrap());
    let err = tokio_tungstenite::connect_async(request).await.unwrap_err();
    assert!(err.to_string().contains("401"), "{err}");
}

#[tokio::test]
async fn slow_client_gets_resync_then_1013_and_does_not_slow_others() {
    const EVENTS: u64 = 3000;
    let redis = start_redis().await;
    // A long close timeout lets the test read the close frame after the fact.
    let (addr, state) = instance_with(&redis, |s| {
        s.ws.close_timeout = Duration::from_secs(60);
    })
    .await;

    let mut fast = Client::connect(&addr).await;
    let mut slow = Client::connect_small_buffer(&addr).await;
    fast.subscribe("room:busy").await;
    slow.subscribe("room:busy").await;

    // The fast client reads as events arrive and measures how late they are.
    let reader = tokio::spawn(async move {
        let mut late_us = Vec::new();
        for seq in 1..=EVENTS {
            let event = fast.next_json().await;
            assert_eq!(event["seq"], seq, "fast client missed an event");
            late_us.push(now_us() - event["payload"]["t"].as_u64().unwrap());
        }
        late_us.sort();
        late_us
    });

    // 2KB payloads fill the slow client's buffers quickly. It never reads.
    let filler = "x".repeat(2048);
    for i in 0..EVENTS {
        publish(
            &state,
            "room:busy",
            "t",
            json!({"i": i, "t": now_us(), "pad": filler}),
        )
        .await;
    }

    let late_us = reader.await.unwrap();
    let p99 = late_us[late_us.len() * 99 / 100];
    assert!(p99 < 50_000, "fast client p99 delay {p99}µs");

    let (texts, code) = slow.read_until_close().await;
    assert_eq!(code, Some(1013));
    let resync_at = texts
        .iter()
        .position(|m| m["type"] == "resync" && m["topic"] == "room:busy")
        .expect("slow client should get resync before being closed");
    assert!(resync_at > 0);
    assert!(
        texts.len() < EVENTS as usize,
        "slow client got everything; it wasn't slow"
    );
}

#[tokio::test]
async fn client_that_stops_answering_pings_is_dropped() {
    let redis = start_redis().await;
    let (addr, _) = instance_with(&redis, |s| {
        s.ws.ping_interval = Duration::from_millis(200);
        s.ws.close_timeout = Duration::from_secs(30);
    })
    .await;

    let mut silent = Client::connect(&addr).await;
    let mut alive = Client::connect(&addr).await;
    alive.subscribe("room:1").await;

    // `alive` keeps reading (and so answering pings); `silent` doesn't.
    assert!(alive.is_quiet(Duration::from_millis(1500)).await);
    let (_, code) = silent.read_until_close().await;
    assert_eq!(code, Some(1001));
    alive.subscribe("room:2").await; // still connected
}

#[tokio::test]
async fn shutdown_closes_clients_with_1012_and_refuses_new_ones() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut clients = Vec::new();
    for i in 0..20 {
        let mut client = Client::connect(&addr).await;
        client.subscribe(&format!("team:{i}")).await;
        clients.push(client);
    }

    state.shutdown.cancel();
    for client in &mut clients {
        assert_eq!(client.read_until_close().await.1, Some(1012));
    }

    // During the shutdown delay the port is still open, but upgrades are
    // refused so clients reconnect to another instance.
    let err = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1/ws"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("503"), "{err}");
}

#[tokio::test]
async fn subscriber_reconnects_to_redis_and_clients_resync() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("room:1").await;

    // Drop the subscriber's connection, as a Redis restart would.
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

    let resync = client.next_json().await;
    assert_eq!(resync, json!({"type": "resync", "topic": "room:1"}));

    // `resync` is only sent once the subscription is back, so an event
    // published right after it must arrive.
    publish(&state, "room:1", "t", json!("after reconnect")).await;
    let event = client.next_json().await;
    assert_eq!(event["payload"], "after reconnect");
}

#[tokio::test]
async fn readyz_follows_redis() {
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let state = state(migrated_pool(&db, 2).await, &redis.url).await;
    let app = app(state);
    assert_eq!(send(&app, "/readyz").await.status(), StatusCode::OK);

    redis.container.pause().await.unwrap();
    assert_eq!(
        send(&app, "/readyz").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(send(&app, "/healthz").await.status(), StatusCode::OK);

    redis.container.unpause().await.unwrap();
    assert!(
        eventually(Duration::from_secs(10), || async {
            send(&app, "/readyz").await.status() == StatusCode::OK
        })
        .await
    );
}

#[tokio::test]
async fn burst_of_events_arrives_without_resync() {
    // Bulk operations (e.g. rebalancing a room queue) publish many events at
    // once. Up to a full send queue's worth must arrive, in order, without a
    // resync, for a client that reads normally.
    const BURST: u64 = ws::SEND_QUEUE as u64;
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("room:bulk").await;

    let topic = Topic::parse("room:bulk").unwrap();
    let publishes = (0..BURST).map(|_| state.events.publish(&topic, "t", &json!(null)));
    futures_util::future::join_all(publishes).await;

    for expected in 1..=BURST {
        let event = client.next_json().await;
        assert_eq!(event["seq"], expected, "got {event}");
    }
}

#[tokio::test]
async fn publish_fails_fast_while_redis_is_unreachable() {
    let redis = start_redis().await;
    let (_, state) = instance(&redis).await;
    redis.container.pause().await.unwrap();

    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        state
            .events
            .publish(&Topic::parse("room:1").unwrap(), "t", &json!(null)),
    )
    .await;
    redis.container.unpause().await.unwrap();

    let result = result.expect("publish hung while Redis was unreachable");
    assert!(result.is_err());
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn dropped_connection_releases_its_topics() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    for topic in ["room:1", "team:1", "mentor-tag:Rust", "user:1"] {
        client.subscribe(topic).await;
    }
    assert_eq!(state.hub.topic_count(), 4);

    // The socket goes away without a WebSocket close frame, like a phone
    // losing signal.
    drop(client);
    assert!(
        eventually(Duration::from_secs(5), || async {
            state.hub.topic_count() == 0
        })
        .await,
        "{} topics still held",
        state.hub.topic_count()
    );
}

#[tokio::test]
async fn connections_coming_and_going_leave_nothing_behind() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut keeper = Client::connect(&addr).await;
    keeper.subscribe("room:shared").await;

    for i in 0..50 {
        let mut client = Client::connect(&addr).await;
        client.subscribe("room:shared").await;
        client.subscribe(&format!("user:{i}")).await;
        if i % 2 == 0 {
            client.ws.close(None).await.unwrap();
        }
        // Odd ones just drop the socket.
    }

    // Only the long-lived client's topic is left, and it still works.
    assert!(
        eventually(Duration::from_secs(5), || async {
            state.hub.topic_count() == 1
        })
        .await
    );
    publish(&state, "room:shared", "t", json!("still here")).await;
    assert_eq!(keeper.next_json().await["payload"], "still here");
}

#[tokio::test]
async fn websocket_outlives_the_http_request_timeout() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("room:1").await;

    tokio::time::sleep(gh_server::REQUEST_TIMEOUT + Duration::from_millis(500)).await;
    publish(&state, "room:1", "t", json!("later")).await;
    assert_eq!(client.next_json().await["payload"], "later");
}

#[tokio::test]
async fn payloads_pass_through_unchanged() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client.subscribe("room:1").await;

    let payloads = [
        json!(null),
        json!(0),
        json!(-1.5e300),
        json!("emoji 🎉, accents é, quotes \" and \\ backslash, newline\n, tab\t"),
        json!({"nested": {"deep": [1, [2, [3]]]}, "empty": {}, "list": []}),
        json!("x".repeat(64 * 1024)), // outgoing events have no 4KB limit
    ];
    for payload in &payloads {
        publish(&state, "room:1", "t", payload.clone()).await;
    }
    for payload in &payloads {
        assert_eq!(&client.next_json().await["payload"], payload);
    }
}

#[tokio::test]
async fn mentor_tags_with_symbols_round_trip_through_redis() {
    let redis = start_redis().await;
    let (addr, state) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    for tag in [
        "mentor-tag:C++",
        "mentor-tag:C#",
        "mentor-tag:node.js",
        "mentor-tag:Vulkan_1-3",
    ] {
        client.subscribe(tag).await;
        publish(&state, tag, "ticket.opened", json!(tag)).await;
        let event = client.next_json().await;
        assert_eq!(
            (event["topic"].as_str(), event["payload"].as_str()),
            (Some(tag), Some(tag))
        );
    }
}

#[tokio::test]
async fn unsubscribing_from_something_never_subscribed_is_acknowledged() {
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    let reply = client.command("unsubscribe", "room:never").await;
    assert_eq!(
        reply,
        json!({"type": "unsubscribed", "topic": "room:never"})
    );
}

#[tokio::test]
async fn extra_fields_are_ignored_but_ops_are_case_sensitive() {
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;

    client
        .send_text(r#"{"op":"subscribe","topic":"room:1","client_version":"1.2.0"}"#)
        .await;
    assert_eq!(client.next_json().await["type"], "subscribed");

    client
        .send_text(r#"{"op":"Subscribe","topic":"room:2"}"#)
        .await;
    assert_eq!(client.next_json().await["type"], "error");

    client.send_text(r#"{"op":"subscribe","topic":42}"#).await;
    assert_eq!(client.next_json().await["type"], "error");

    client.send_text("[]").await;
    assert_eq!(client.next_json().await["type"], "error");
}

#[tokio::test]
async fn plain_http_request_to_the_websocket_route_is_a_client_error() {
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let status = reqwest::get(format!("http://{addr}/api/v1/ws"))
        .await
        .unwrap()
        .status();
    assert!(status.is_client_error(), "{status}");
}

#[tokio::test]
async fn client_ping_gets_a_pong() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let redis = start_redis().await;
    let (addr, _) = instance(&redis).await;
    let mut client = Client::connect(&addr).await;
    client
        .ws
        .send(Message::Ping(b"are you there".to_vec().into()))
        .await
        .unwrap();
    let reply = tokio::time::timeout(RECV_TIMEOUT, client.ws.next())
        .await
        .unwrap();
    assert!(
        matches!(reply, Some(Ok(Message::Pong(ref data))) if &data[..] == b"are you there"),
        "{reply:?}"
    );
}

#[tokio::test]
async fn client_moving_to_another_instance_keeps_counting_seq() {
    // The rolling-deploy path: instance A restarts, the client reconnects to
    // B, and `seq` continues so it can tell whether it missed anything.
    let redis = start_redis().await;
    let (addr_a, state_a) = instance(&redis).await;
    let (addr_b, state_b) = instance(&redis).await;

    let mut client = Client::connect(&addr_a).await;
    client.subscribe("room:1").await;
    publish(&state_a, "room:1", "t", json!("before")).await;
    assert_eq!(client.next_json().await["seq"], 1);

    state_a.shutdown.cancel();
    assert_eq!(client.read_until_close().await.1, Some(1012));

    let mut client = Client::connect(&addr_b).await;
    client.subscribe("room:1").await;
    publish(&state_b, "room:1", "t", json!("after")).await;
    let event = client.next_json().await;
    assert_eq!(
        (event["seq"].clone(), event["payload"].clone()),
        (json!(2), json!("after"))
    );
}

#[tokio::test]
async fn seq_survives_a_server_restart() {
    let redis = start_redis().await;
    {
        let (_, old) = instance(&redis).await;
        publish(&old, "team:1", "t", json!(null)).await;
        publish(&old, "team:1", "t", json!(null)).await;
        old.shutdown.cancel();
    }
    let (_, new) = instance(&redis).await;
    assert_eq!(publish(&new, "team:1", "t", json!(null)).await, 3);
}
