//! Judge room access: the placard exchange, every kind of rejection, room limits,
//! rotation and the audit log. One test with a moving fake clock, since the
//! steps build on each other.

mod common;

use std::sync::{
    atomic::{AtomicI64, Ordering::SeqCst},
    Arc,
};

use auth::{rotate, set_room, JudgeKey, Policy, Role};
use axum::{routing::get, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;

const HOUR: i64 = 60 * 60;
const START: i64 = NOW + HOUR;
const END: i64 = NOW + 4 * HOUR;
const ROOM: &str = "PFT-1200";
const OTHER: &str = "PFT-1300";
const UA: &str = "JudgePhone/1.0";
const IP: &str = "10.1.2.3";

static CLOCK: AtomicI64 = AtomicI64::new(NOW);
fn clock() -> i64 {
    CLOCK.load(SeqCst)
}
fn at(t: i64) {
    CLOCK.store(t, SeqCst);
}

async fn exchange(app: &Router, token: &str, extra: &[(&str, &str)]) -> Res {
    let mut headers = vec![("content-type", "application/json"), ("user-agent", UA), ("x-forwarded-for", IP)];
    headers.extend_from_slice(extra);
    call_body(app, "POST", "/api/v1/judge/exchange", &headers, &json!({ "token": token }).to_string()).await
}

/// The session token from a native exchange.
async fn session(app: &Router, placard: &str) -> String {
    let res = exchange(app, placard, &[]).await;
    assert_eq!(res.status, 200, "{}", res.body);
    body(&res)["token"].as_str().unwrap().into()
}

async fn as_caller(app: &Router, method: &str, path: &str, token: &str) -> u16 {
    call(app, method, path, &[("authorization", &format!("Bearer {token}"))]).await.status
}

fn body(res: &Res) -> Value {
    serde_json::from_str(&res.body).unwrap()
}

fn queue(room: &str) -> String {
    format!("/api/v1/rooms/{room}/queue")
}

#[tokio::test]
async fn judge_rooms() {
    let db = db("auth_judge_test").await;
    let mut state = state(db.clone(), redis().await, clock);
    // Stand-in judging route; the real ones come with Kenny's judging features.
    Arc::get_mut(&mut state).unwrap().policies.insert("GET /api/v1/rooms/{room_id}/queue".into(), Policy::RequireRoom);
    state.sessions.reload(NOW).await.unwrap();
    let app = app(state.clone(), Router::new().route("/api/v1/rooms/{room_id}/queue", get(|| async { "ok" })));
    let key = judge_key();
    for room in [ROOM, OTHER] {
        set_room(&db, &key, room, START, END).await.unwrap();
    }
    let placard = key.placard(ROOM, 1);

    // Native (no Origin): the token comes in the body, for this room, ending with judging.
    at(START + 60);
    let res = exchange(&app, &placard, &[]).await;
    assert_eq!(res.status, 200, "{}", res.body);
    assert_eq!(res.headers["cache-control"], "no-store");
    assert_eq!(res.headers["referrer-policy"], "no-referrer");
    assert!(res.headers.get("set-cookie").is_none());
    let b = body(&res);
    let judge = b["token"].as_str().unwrap().to_string();
    let claims = state.keys.verify(&judge, clock()).unwrap();
    assert_eq!((claims.roles.clone(), claims.room_id.as_deref(), claims.exp), (vec![Role::JudgeRoom], Some(ROOM), END));
    assert_eq!(claims.sub, format!("judge:{}", claims.sid));
    assert_eq!((b["roomId"].as_str(), b["exp"].as_i64()), (Some(ROOM), Some(END)));

    // Its own room's routes only. An admin passes every room.
    assert_eq!(as_caller(&app, "GET", &queue(ROOM), &judge).await, 200);
    assert_eq!(as_caller(&app, "GET", &queue(OTHER), &judge).await, 403);
    let admin_only = "/api/v1/admin/users/00000000-0000-0000-0000-000000000000/revoke-sessions";
    assert_eq!(as_caller(&app, "POST", admin_only, &judge).await, 403);
    let admin = user(&db, "admin@example.com", &[Role::Admin]).await;
    let admin = state.keys.issue(&state.sessions.start(&admin, clock()).await.unwrap());
    for room in [ROOM, OTHER] {
        assert_eq!(as_caller(&app, "GET", &queue(room), &admin).await, 200, "admin, {room}");
    }

    // Never renews, even in its last minute, and is dead at the close of judging.
    at(END - 60);
    let res = call(&app, "GET", "/api/v1/me", &[("authorization", &format!("Bearer {judge}"))]).await;
    assert_eq!(res.status, 200);
    assert!(res.headers.get("x-session-token").is_none(), "judge sessions never renew");
    at(END);
    assert_eq!(as_caller(&app, "GET", &queue(ROOM), &judge).await, 401);

    // Web: our own page gets the cookie, kept only until judging ends, and no token in the body.
    at(START + 60);
    let res = exchange(&app, &placard, &[("origin", ORIGIN)]).await;
    assert_eq!(res.status, 200);
    let cookie = res.headers["set-cookie"].to_str().unwrap();
    assert!(cookie.starts_with("__Host-session=") && cookie.contains("HttpOnly"), "{cookie}");
    assert!(cookie.ends_with(&format!("Max-Age={}", END - START - 60)), "{cookie}");
    assert!(body(&res).get("token").is_none());

    // Logout revokes a judge session like any other: 401 on its next request.
    let t = session(&app, &placard).await;
    assert_eq!(as_caller(&app, "POST", "/api/v1/auth/logout", &t).await, 204);
    assert_eq!(as_caller(&app, "GET", &queue(ROOM), &t).await, 401);

    // Every rejection is the same bare 401.
    let mut tampered = B64.decode(&placard).unwrap();
    tampered[0] ^= 1; // PFT-1200 -> QFT-1200, HMAC unchanged
    let rejected = [
        ("forged", JudgeKey::new([1; 32]).placard(ROOM, 1), START + 60),
        ("tampered", B64.encode(&tampered), START + 60),
        ("malformed", "not a token!".into(), START + 60),
        ("unknown room", key.placard("NOPE-1", 1), START + 60),
        ("before judging", placard.clone(), START - 1),
        ("after judging", placard.clone(), END),
    ];
    for (why, token, t) in &rejected {
        at(*t);
        let res = exchange(&app, token, &[]).await;
        assert_eq!((res.status, res.body.as_str()), (401, ""), "{why}");
        assert_eq!(res.headers["referrer-policy"], "no-referrer", "{why}");
        assert!(res.headers.get("set-cookie").is_none(), "{why}");
    }
    at(START);
    session(&app, &placard).await; // the window opens on the second

    // Rotation: the old placard fails, sessions it gave out are cut off, the new one works.
    at(START + 60);
    let before = session(&app, &placard).await;
    let fresh = rotate(&state.sessions, &key, ROOM, clock()).await.unwrap().unwrap();
    assert_eq!(fresh, key.placard(ROOM, 2));
    assert_eq!(exchange(&app, &placard, &[]).await.status, 401, "old placard");
    assert_eq!(as_caller(&app, "GET", &queue(ROOM), &before).await, 401, "session from the old placard");
    let after = session(&app, &fresh).await;
    assert_eq!(as_caller(&app, "GET", &queue(ROOM), &after).await, 200);
    session(&app, &key.placard(OTHER, 1)).await; // other rooms are untouched
    assert_eq!(rotate(&state.sessions, &key, "NOPE-1", clock()).await.unwrap(), None);
    let hashed: bool = sqlx::query_scalar("SELECT token_hash = sha256(convert_to($1, 'UTF8')) FROM presentation_rooms WHERE room_id = $2")
        .bind(&fresh)
        .bind(ROOM)
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(hashed, "the room stores the new placard's hash");

    // Every exchange is audited with room, time and device; sessions only on success.
    // room, outcome, user agent, IP, has a session, unix time
    type Row = (Option<String>, String, Option<String>, Option<String>, bool, i64);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT room_id, outcome, user_agent, ip, session_id IS NOT NULL, extract(epoch FROM at)::bigint
         FROM judge_exchange_audit ORDER BY id",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    let got: Vec<(Option<&str>, &str)> = rows.iter().map(|r| (r.0.as_deref(), r.1.as_str())).collect();
    let (room, other) = (Some(ROOM), Some(OTHER));
    assert_eq!(
        got,
        [
            (room, "ok"),
            (room, "ok"),
            (room, "ok"),
            (room, "bad_signature"),
            (Some("QFT-1200"), "bad_signature"),
            (None, "malformed"),
            (Some("NOPE-1"), "unknown_room"),
            (room, "outside_window"),
            (room, "outside_window"),
            (room, "ok"),
            (room, "ok"),
            (room, "old_version"),
            (room, "ok"),
            (other, "ok"),
        ]
    );
    for (_, outcome, ua, ip, has_session, _) in &rows {
        assert_eq!((ua.as_deref(), ip.as_deref()), (Some(UA), Some(IP)));
        assert_eq!(*has_session, outcome == "ok", "{outcome}");
    }
    assert_eq!(rows[7].5, START - 1, "the time of the exchange");

    // No placard or session token is stored anywhere.
    for secret in [&placard, &fresh, &judge, &before, &after] {
        assert_eq!(stored(&db, secret).await, 0, "{secret}");
    }
}

/// How many rows in the judge and session tables contain `text`.
async fn stored(db: &PgPool, text: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM judge_exchange_audit t WHERE strpos(t::text, $1) > 0)
              + (SELECT count(*) FROM sessions t WHERE strpos(t::text, $1) > 0)
              + (SELECT count(*) FROM presentation_rooms t WHERE strpos(t::text, $1) > 0)",
    )
    .bind(text)
    .fetch_one(db)
    .await
    .unwrap()
}

/// Postgres error code: 23514 = CHECK failed, 23503 = no such row to reference.
async fn code(db: &PgPool, sql: &str) -> Option<String> {
    let err = sqlx::query(sql).execute(db).await.err()?;
    err.as_database_error().and_then(|e| e.code()).map(|c| c.into_owned())
}

#[tokio::test]
async fn judge_rooms_migration() {
    let db = db("auth_judge_migration_test").await;
    let check = Some("23514");

    // Rooms: ids safe in URLs, file names and topics; the window runs forwards.
    assert_eq!(code(&db, "INSERT INTO presentation_rooms (room_id) VALUES ('a/b')").await.as_deref(), check);
    let backwards = "INSERT INTO presentation_rooms VALUES ('R0', 1, NULL, now(), now() - interval '1 hour')";
    assert_eq!(code(&db, backwards).await.as_deref(), check);
    assert_eq!(code(&db, "INSERT INTO presentation_rooms (room_id) VALUES ('R1')").await, None);

    // A session is a user's or a room's, never both or neither.
    let day = "now() + interval '1 day'";
    user(&db, "ricky@example.com", &[]).await;
    assert_eq!(code(&db, &format!("INSERT INTO sessions (expires_at) VALUES ({day})")).await.as_deref(), check);
    let both = format!("INSERT INTO sessions (user_id, room_id, expires_at) VALUES ((SELECT id FROM users), 'R1', {day})");
    assert_eq!(code(&db, &both).await.as_deref(), check);
    assert_eq!(code(&db, &format!("INSERT INTO sessions (room_id, expires_at) VALUES ('R1', {day})")).await, None);
    let nowhere = format!("INSERT INTO sessions (room_id, expires_at) VALUES ('R9', {day})");
    assert_eq!(code(&db, &nowhere).await.as_deref(), Some("23503"));

    // Audit: known outcomes only, and a rejection never has a session.
    let audit = |outcome: &str, session: &str| {
        format!("INSERT INTO judge_exchange_audit (at, outcome, session_id) VALUES (now(), '{outcome}', {session})")
    };
    let a_session = "(SELECT id FROM sessions LIMIT 1)";
    assert_eq!(code(&db, &audit("maybe", "NULL")).await.as_deref(), check);
    assert_eq!(code(&db, &audit("bad_signature", a_session)).await.as_deref(), check);
    assert_eq!(code(&db, &audit("ok", a_session)).await, None);

    // Deleting a room ends its sessions; the audit row stays, without the session.
    sqlx::query("DELETE FROM presentation_rooms WHERE room_id = 'R1'").execute(&db).await.unwrap();
    let left: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM sessions WHERE room_id IS NOT NULL), (SELECT count(*) FROM judge_exchange_audit WHERE session_id IS NULL)",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(left, (0, 1));
}

/// `/judge/exchange` server time under load: 2,000 exchanges from 16 callers
/// (JUDGE_BENCH_CALLERS to change). Not run by default:
/// `cargo test -p auth --test judge -- --ignored --nocapture exchange_p99`.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn exchange_p99() {
    let db = db("auth_judge_bench").await;
    set_room(&db, &judge_key(), ROOM, NOW - HOUR, NOW + HOUR).await.unwrap();
    let app = app(state(db, redis().await, || NOW), Router::new());
    let body = json!({ "token": judge_key().placard(ROOM, 1) }).to_string();
    let mut callers = tokio::task::JoinSet::new();
    let callers_n: usize = std::env::var("JUDGE_BENCH_CALLERS").ok().and_then(|n| n.parse().ok()).unwrap_or(16);
    for _ in 0..callers_n {
        let (app, body) = (app.clone(), body.clone());
        callers.spawn(async move {
            let headers = [("content-type", "application/json"), ("x-forwarded-for", IP)];
            let mut times = vec![];
            for _ in 0..2000 / callers_n {
                let t = std::time::Instant::now();
                let res = call_body(&app, "POST", "/api/v1/judge/exchange", &headers, &body).await;
                times.push(t.elapsed());
                assert_eq!(res.status, 200, "{}", res.body);
            }
            times
        });
    }
    let mut times = vec![];
    while let Some(caller) = callers.join_next().await {
        times.extend(caller.unwrap());
    }
    times.sort();
    let pct = |p: usize| times[times.len() * p / 100 - 1];
    println!("judge exchange: n={} p50={:?} p95={:?} p99={:?} max={:?}", times.len(), pct(50), pct(95), pct(99), times.last().unwrap());
}
