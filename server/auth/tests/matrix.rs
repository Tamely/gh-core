//! Steps 6 and 8: every route × every kind of caller, in one table.
//! Fails if a route in the policy map has no row here, so a new route can't
//! skip this table.

mod common;

use std::{collections::BTreeSet, sync::Arc};

use auth::{Claims, Policy, Role, SESSION_SECS};
use axum::{routing::get, Router};
use common::*;

const CALLERS: [&str; 10] =
    ["anon", "hacker", "volunteer", "mentor", "admin", "judge A", "judge B", "no roles", "revoked", "expired"];

// Expected status per caller, in CALLERS order.
const PUBLIC: [u16; 10] = [200; 10];
const SIGNED_IN: [u16; 10] = [401, 200, 200, 200, 200, 200, 200, 200, 401, 401];
const ADMIN_ONLY: [u16; 10] = [401, 403, 403, 403, 204, 403, 403, 403, 401, 401];
// Room A: its judges and admins.
const ROOM_A: [u16; 10] = [401, 403, 403, 403, 200, 200, 403, 403, 401, 401];
const NO_POLICY: [u16; 10] = [403; 10];
// Public, so every caller reaches the handler, which rejects the matrix's empty
// body (no JSON content type). Sign-in itself is tested in tests/codes.rs.
const PUBLIC_JSON: [u16; 10] = [415; 10];

#[tokio::test]
async fn route_by_role_matrix() {
    let db = db("auth_matrix_test").await;
    let mut state = state(db.clone(), redis().await, || NOW);
    // Stand-in route that needs a room; the real ones come with judging.
    Arc::get_mut(&mut state).unwrap().policies.insert("GET /api/v1/rooms/{room_id}/scores".into(), Policy::RequireRoom);
    state.sessions.reload(NOW).await.unwrap();
    for room in ["room-a", "room-b"] {
        auth::set_room(&db, &judge_key(), room, NOW - DAY, NOW + DAY).await.unwrap();
    }

    // Kenny's routes, stood in by handlers that just say OK, plus one with no policy.
    let ok = get(|| async { "ok" });
    let extra = Router::new()
        .route("/healthz", ok.clone())
        .route("/readyz", ok.clone())
        .route("/api/v1/ws", ok.clone())
        .route("/api/v1/rooms/{room_id}/scores", ok.clone())
        .route("/api/v1/forgotten", ok);
    let app = app(state.clone(), extra);

    let target = user(&db, "target@example.com", &[Role::Hacker]).await;
    let revoke = format!("/api/v1/admin/users/{target}/revoke-sessions");

    // (policy-map key, method, path, expected per caller)
    let rows: Vec<(&str, &str, &str, [u16; 10])> = vec![
        ("GET /.well-known/jwks.json", "GET", "/.well-known/jwks.json", PUBLIC),
        ("GET /healthz", "GET", "/healthz", PUBLIC),
        ("GET /readyz", "GET", "/readyz", PUBLIC),
        ("GET /api/v1/ws", "GET", "/api/v1/ws", PUBLIC),
        ("POST /api/v1/auth/code", "POST", "/api/v1/auth/code", PUBLIC_JSON),
        ("POST /api/v1/auth/code/verify", "POST", "/api/v1/auth/code/verify", PUBLIC_JSON),
        ("POST /api/v1/judge/exchange", "POST", "/api/v1/judge/exchange", PUBLIC_JSON),
        ("GET /api/v1/me", "GET", "/api/v1/me", SIGNED_IN),
        ("POST /api/v1/auth/logout", "POST", "/api/v1/auth/logout", SIGNED_IN.map(|s| if s == 200 { 204 } else { s })),
        ("POST /api/v1/admin/users/{id}/revoke-sessions", "POST", &revoke, ADMIN_ONLY),
        ("GET /api/v1/rooms/{room_id}/scores", "GET", "/api/v1/rooms/room-a/scores", ROOM_A),
        ("(none)", "GET", "/api/v1/forgotten", NO_POLICY),
        ("(none)", "GET", "/api/v1/no-such-route", NO_POLICY),
        ("(none)", "DELETE", "/api/v1/me", NO_POLICY),
    ];

    let covered: BTreeSet<&str> = rows.iter().map(|r| r.0).collect();
    for route in state.policies.keys() {
        assert!(covered.contains(route.as_str()), "{route} is in the policy map but has no row in this table");
    }

    let mut failures = vec![];
    for (_, method, path, expected) in &rows {
        for (who, want) in CALLERS.iter().zip(expected) {
            // A fresh session per cell, since logout and revoke use theirs up.
            let token = caller_token(&state, &db, &app, who).await;
            let headers: Vec<(&str, String)> = token.iter().map(|t| ("authorization", format!("Bearer {t}"))).collect();
            let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let got = call(&app, method, path, &headers).await.status;
            if got != *want {
                failures.push(format!("{method} {path} as {who}: got {got}, want {want}"));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

async fn caller_token(state: &auth::AuthState, db: &sqlx::PgPool, app: &Router, who: &str) -> Option<String> {
    let role = match who {
        "anon" => return None,
        "judge A" | "judge B" => return Some(judge_token(app, if who == "judge A" { "room-a" } else { "room-b" }).await),
        "hacker" | "revoked" | "expired" => vec![Role::Hacker],
        "volunteer" => vec![Role::Volunteer],
        "mentor" => vec![Role::Mentor],
        "admin" => vec![Role::Admin],
        _ => vec![],
    };
    let email = format!("{}@example.com", sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users").fetch_one(db).await.unwrap());
    let id = user(db, &email, &role).await;
    let mut claims: Claims = state.sessions.start(&id, NOW).await.unwrap();
    match who {
        "revoked" => state.sessions.revoke(&claims.sid, NOW).await.unwrap(),
        "expired" => claims.exp = NOW - SESSION_SECS,
        _ => {}
    }
    Some(state.keys.issue(&claims))
}

/// A real judge session, from the placard exchange.
async fn judge_token(app: &Router, room: &str) -> String {
    let body = format!(r#"{{"token":"{}"}}"#, judge_key().placard(room, 1));
    let res = call_body(app, "POST", "/api/v1/judge/exchange", &[("content-type", "application/json")], &body).await;
    assert_eq!(res.status, 200, "{}", res.body);
    serde_json::from_str::<serde_json::Value>(&res.body).unwrap()["token"].as_str().unwrap().into()
}
