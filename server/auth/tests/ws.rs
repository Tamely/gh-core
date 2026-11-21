//! Step 7: who may open the socket, and which topics each caller may hear.

mod common;

use auth::{may_subscribe, ws_caller, ws_first_message, Caller, Claims, Role, Via};
use axum::http::{HeaderMap, HeaderValue};
use common::*;

fn signed(sub: &str, roles: &[Role], room: Option<&str>) -> Caller {
    let claims = Claims {
        sub: sub.into(),
        roles: roles.to_vec(),
        sid: "s".into(),
        iat: NOW,
        exp: NOW + DAY,
        room_id: room.map(Into::into),
    };
    Caller::Signed { claims, via: Via::Bearer }
}

#[test]
fn topic_rules() {
    let anon = Caller::Anonymous;
    let hacker = signed("u1", &[Role::Hacker], None);
    let mentor = signed("u2", &[Role::Mentor], None);
    let admin = signed("u3", &[Role::Admin], None);
    let judge_a = signed("u4", &[Role::JudgeRoom], Some("a"));

    let cases = [
        ("user:u1", &hacker, true),
        ("user:u2", &hacker, false),
        ("user:u1", &admin, false),
        ("user:u1", &anon, false),
        ("room:a", &judge_a, true),
        ("room:b", &judge_a, false),
        ("room:a", &admin, true),
        ("room:a", &hacker, false),
        ("team:1", &hacker, false),
        ("team:1", &admin, true),
        ("mentor-tag:rust", &mentor, false),
        ("mentor-tag:rust", &admin, true),
        ("unknown:x", &admin, false),
        ("nocolon", &admin, false),
    ];
    for (topic, caller, want) in cases {
        assert_eq!(may_subscribe(caller, topic), want, "{topic} as {caller:?}");
    }
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.insert(*k, HeaderValue::from_str(v).unwrap());
    }
    h
}

#[tokio::test]
async fn opening_the_socket() {
    let db = db("auth_ws_test").await;
    let state = state(db.clone(), redis().await, || NOW);
    state.sessions.reload(NOW).await.unwrap();
    let id = user(&db, "ws@example.com", &[Role::Hacker]).await;
    let token = state.keys.issue(&state.sessions.start(&id, NOW).await.unwrap());

    let cookie = format!("__Host-session={token}");
    let is_signed = |r: Result<Caller, _>| matches!(r, Ok(Caller::Signed { .. }));

    // Web: cookie on the upgrade, only from our page.
    assert!(is_signed(ws_caller(&state, &headers(&[("cookie", &cookie), ("origin", ORIGIN)])).await));
    let evil = ws_caller(&state, &headers(&[("cookie", &cookie), ("origin", "https://evil.example")])).await;
    assert_eq!(evil.unwrap_err(), 403);
    assert_eq!(ws_caller(&state, &headers(&[("cookie", &cookie)])).await.unwrap_err(), 403);

    // Native: no credentials on the upgrade, then the token as the first message.
    assert_eq!(ws_caller(&state, &HeaderMap::new()).await.unwrap(), Caller::Anonymous);
    assert!(is_signed(ws_first_message(&state, &token).await));
    assert_eq!(ws_first_message(&state, "garbage").await.unwrap(), Caller::Anonymous);

    // Revoked: anonymous, so it can't subscribe to anything.
    let sid = state.keys.verify(&token, NOW).unwrap().sid;
    state.sessions.revoke(&sid, NOW).await.unwrap();
    assert_eq!(ws_first_message(&state, &token).await.unwrap(), Caller::Anonymous);
}
