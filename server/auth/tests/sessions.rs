//! Step 5: logout and admin revoke take effect on the next request, even when
//! Redis loses its data or is down; renewal slides a session forward.
//! One test so the Redis steps (which share keys) run in order.

mod common;

use auth::{Keys, Role, Sessions};
use axum::Router;
use common::*;
use fred::prelude::KeysInterface;

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn me(app: &Router, token: &str) -> u16 {
    call(app, "GET", "/api/v1/me", &[("authorization", &bearer(token))]).await.status
}

async fn token(sessions: &Sessions, keys: &Keys, user: &str, now: i64) -> String {
    keys.issue(&sessions.start(user, now).await.unwrap())
}

#[tokio::test]
async fn sessions_and_revocation() {
    let db = db("auth_sessions_test").await;
    let redis = redis().await;
    let state = state(db.clone(), redis.clone(), || NOW);
    state.sessions.reload(NOW).await.unwrap();
    let app = app(state.clone(), Router::new());
    let (s, k) = (&state.sessions, &state.keys);

    let ricky = user(&db, "ricky@example.com", &[Role::Hacker]).await;
    let admin = user(&db, "admin@example.com", &[Role::Admin]).await;

    // /me shows the session's user and roles.
    let t = token(s, k, &ricky, NOW).await;
    let res = call(&app, "GET", "/api/v1/me", &[("authorization", &bearer(&t))]).await;
    assert_eq!(res.status, 200);
    assert!(res.body.contains(&ricky) && res.body.contains("\"HACKER\""), "{}", res.body);

    // Logout: the same token is 401 on the next request, and the cookie is cleared.
    let res = call(&app, "POST", "/api/v1/auth/logout", &[("authorization", &bearer(&t))]).await;
    assert_eq!(res.status, 204);
    assert!(res.headers["set-cookie"].to_str().unwrap().contains("Max-Age=0"));
    assert_eq!(me(&app, &t).await, 401, "logged-out token");

    // Admin revoke: every session the user has is cut off. A hacker can't do it.
    let (phone, laptop) = (token(s, k, &ricky, NOW).await, token(s, k, &ricky, NOW).await);
    let a = token(s, k, &admin, NOW).await;
    let revoke = format!("/api/v1/admin/users/{ricky}/revoke-sessions");
    assert_eq!(call(&app, "POST", &revoke, &[("authorization", &bearer(&phone))]).await.status, 403);
    assert_eq!(me(&app, &phone).await, 200, "a refused revoke changes nothing");
    assert_eq!(call(&app, "POST", &revoke, &[("authorization", &bearer(&a))]).await.status, 204);
    assert_eq!((me(&app, &phone).await, me(&app, &laptop).await), (401, 401), "both devices");
    assert_eq!(me(&app, &a).await, 200, "the admin's own session is untouched");
    let bad = "/api/v1/admin/users/not-a-uuid/revoke-sessions";
    assert_eq!(call(&app, "POST", bad, &[("authorization", &bearer(&a))]).await.status, 400);

    // Redis restarts and loses everything: still 401 (Postgres answers and refills the cache).
    let wiped: Vec<String> = vec!["gh:revoked:ready".into()];
    let _: () = redis.del(wiped).await.unwrap();
    for sid in sqlx::query_scalar::<_, String>("SELECT id::text FROM sessions").fetch_all(&db).await.unwrap() {
        let _: () = redis.del(format!("gh:revoked:{sid}")).await.unwrap();
    }
    assert_eq!(me(&app, &phone).await, 401, "after Redis lost its data");
    let ready: Option<String> = redis.get("gh:revoked:ready").await.unwrap();
    assert!(ready.is_some(), "cache reloaded");

    // Redis down: Postgres answers. Both down: 503, never let a maybe-revoked token in.
    let fresh = token(s, k, &ricky, NOW).await;
    let no_redis = common::app(common::state(db.clone(), dead_redis().await, || NOW), Router::new());
    assert_eq!((me(&no_redis, &phone).await, me(&no_redis, &fresh).await), (401, 200), "Redis down");
    let neither = common::app(common::state(dead_db(), dead_redis().await, || NOW), Router::new());
    assert_eq!(me(&neither, &fresh).await, 503, "Redis and Postgres down");
    assert_eq!(call(&neither, "GET", "/.well-known/jwks.json", &[]).await.status, 200, "public routes still work");

    // Sliding renewal with a fake clock. Issued day 0, expires day 14.
    let t = token(s, k, &ricky, NOW).await;
    let at = |now: fn() -> i64| common::app(common::state(db.clone(), redis.clone(), now), Router::new());
    let day6 = call(&at(|| NOW + 6 * DAY), "GET", "/api/v1/me", &[("authorization", &bearer(&t))]).await;
    assert_eq!(day6.status, 200);
    assert!(!day6.headers.contains_key("x-session-token"), "8 days left: no renewal");

    let day8 = call(&at(|| NOW + 8 * DAY), "GET", "/api/v1/me", &[("authorization", &bearer(&t))]).await;
    assert_eq!(day8.status, 200);
    let renewed = day8.headers["x-session-token"].to_str().unwrap().to_string();
    let claims = k.verify(&renewed, NOW + 8 * DAY).unwrap();
    assert_eq!(claims.exp, NOW + 22 * DAY, "6 days left: renewed to day 22");

    let day15 = at(|| NOW + 15 * DAY);
    assert_eq!(me(&day15, &t).await, 401, "the old token expired on day 14");
    assert_eq!(me(&day15, &renewed).await, 200, "the renewed one is good until day 22");
    // Renewal moved the session's end in the database too: revoking on day 15
    // (after the original end) still finds it.
    s.revoke(&claims.sid, NOW + 15 * DAY).await.unwrap();
    assert_eq!(me(&day15, &renewed).await, 401, "renewed session can still be revoked");

    // Roles are re-read on renewal, so a removed role doesn't live on.
    let boss = user(&db, "boss@example.com", &[Role::Admin, Role::Mentor]).await;
    let b = token(s, k, &boss, NOW).await;
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1::uuid AND role = 'ADMIN'").bind(&boss).execute(&db).await.unwrap();
    let res = call(&at(|| NOW + 8 * DAY), "GET", "/api/v1/me", &[("authorization", &bearer(&b))]).await;
    let roles = k.verify(res.headers["x-session-token"].to_str().unwrap(), NOW + 8 * DAY).unwrap().roles;
    assert_eq!(roles, vec![Role::Mentor], "ADMIN dropped at renewal");

    // Judge sessions never renew: they end with judging.
    let mut judge = s.start(&ricky, NOW).await.unwrap();
    judge.roles = vec![Role::JudgeRoom];
    judge.room_id = Some("room-a".into());
    let res = call(&at(|| NOW + 8 * DAY), "GET", "/api/v1/me", &[("authorization", &bearer(&k.issue(&judge)))]).await;
    assert_eq!(res.status, 200);
    assert!(!res.headers.contains_key("x-session-token"), "judge session renewed");

    // Web gets the renewed token as a cookie instead.
    let c = token(s, k, &ricky, NOW).await;
    let cookie = format!("__Host-session={c}");
    let res = call(&at(|| NOW + 8 * DAY), "GET", "/api/v1/me", &[("cookie", &cookie)]).await;
    let set = res.headers["set-cookie"].to_str().unwrap();
    assert!(set.starts_with("__Host-session=ey") && set.contains("HttpOnly") && set.contains("Max-Age=1209600"), "{set}");

    // A revoked session is refused, and renew() itself won't extend it either.
    let revoked = k.verify(&c, NOW).unwrap();
    s.revoke(&revoked.sid, NOW).await.unwrap();
    assert_eq!(call(&at(|| NOW + 8 * DAY), "GET", "/api/v1/me", &[("cookie", &cookie)]).await.status, 401);
    assert_eq!(s.renew(&revoked, NOW + 8 * DAY).await.unwrap(), None);
}
