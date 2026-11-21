//! Step 4: the token is read from the cookie or Bearer header, and cookie
//! requests that change data must come from our own page.

mod common;

use auth::{Role, SESSION_SECS};
use axum::Router;
use common::*;

#[tokio::test]
async fn identifies_the_caller() {
    let db = db("auth_caller_test").await;
    let state = state(db.clone(), redis().await, || NOW);
    state.sessions.reload(NOW).await.unwrap();
    let app = app(state.clone(), Router::new());
    let ricky = user(&db, "ricky@example.com", &[Role::Hacker]).await;
    let me = "/api/v1/me";
    let logout = "/api/v1/auth/logout";

    // (name, method, path, headers with {t} for a fresh token, expected status)
    let cases = [
        ("no token", "GET", me, vec![], 401),
        ("bearer", "GET", me, vec![("authorization", "Bearer {t}")], 200),
        ("cookie among others", "GET", me, vec![("cookie", "theme=dark; __Host-session={t}; other=1")], 200),
        ("garbage bearer", "GET", me, vec![("authorization", "Bearer nope")], 401),
        ("expired cookie", "GET", me, vec![("cookie", "__Host-session={expired}")], 401),
        ("cookie with another name", "GET", me, vec![("cookie", "session={t}")], 401),
        ("cookie POST from our page", "POST", logout, vec![("cookie", "__Host-session={t}"), ("origin", ORIGIN)], 204),
        ("cookie POST from another site", "POST", logout, vec![("cookie", "__Host-session={t}"), ("origin", "https://evil.example")], 403),
        ("cookie POST with no Origin", "POST", logout, vec![("cookie", "__Host-session={t}")], 403),
        ("bearer POST, no Origin (native)", "POST", logout, vec![("authorization", "Bearer {t}")], 204),
    ];

    for (name, method, path, headers, status) in cases {
        let mut claims = state.sessions.start(&ricky, NOW).await.unwrap();
        let t = state.keys.issue(&claims);
        claims.exp = NOW - SESSION_SECS;
        let expired = state.keys.issue(&claims);
        let headers: Vec<(&str, String)> =
            headers.iter().map(|(k, v)| (*k, v.replace("{t}", &t).replace("{expired}", &expired))).collect();
        let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(call(&app, method, path, &headers).await.status, status, "{name}");
    }
}
