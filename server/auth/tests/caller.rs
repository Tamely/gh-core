//! Step 4: every request becomes a known caller or anonymous, and cookie
//! requests that change data must come from our own page.

use std::sync::Arc;

use auth::{layer, AuthState, Caller, Claims, Keys, Role, Via, SESSION_SECS};
use axum::{
    body::Body,
    http::{Method, Request},
    middleware::from_fn_with_state,
    routing::get,
    Extension, Router,
};
use ed25519_dalek::SigningKey;
use tower::ServiceExt;

const NOW: i64 = 1_800_000_000;
const ORIGIN: &str = "https://app.geauxhack.com";

fn keys() -> Keys {
    Keys::new(SigningKey::from_bytes(&[1; 32]), vec![])
}

fn token(exp: i64) -> String {
    keys().issue(&Claims {
        sub: "user-1".into(),
        roles: vec![Role::Hacker],
        sid: "session-1".into(),
        iat: NOW,
        exp,
        room_id: None,
    })
}

/// Stand-in app: one route that says who it thinks is calling.
fn app() -> Router {
    let state = Arc::new(AuthState { keys: keys(), origin: ORIGIN.into(), now: || NOW });
    let whoami = |Extension(caller): Extension<Caller>| async move {
        match caller {
            Caller::Anonymous => "anonymous".to_string(),
            Caller::Signed { claims, via: Via::Cookie } => format!("{} via cookie", claims.sub),
            Caller::Signed { claims, via: Via::Bearer } => format!("{} via bearer", claims.sub),
        }
    };
    Router::new()
        .route("/whoami", get(whoami).post(whoami))
        .layer(from_fn_with_state(state, layer))
}

async fn call(method: Method, headers: &[(&str, &str)]) -> (u16, String) {
    let mut req = Request::builder().method(method).uri("/whoami");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let res = app().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn identifies_the_caller() {
    let good = token(NOW + SESSION_SECS);
    let expired = token(NOW);
    let bearer = format!("Bearer {good}");
    let cookie = format!("theme=dark; __Host-session={good}; other=1");
    let expired_cookie = format!("__Host-session={expired}");
    let wrong_name = format!("session={good}");

    let get = Method::GET;
    let post = Method::POST;
    let cases: Vec<(&str, Method, Vec<(&str, &str)>, (u16, &str))> = vec![
        ("no token", get.clone(), vec![], (200, "anonymous")),
        ("bearer", get.clone(), vec![("authorization", &bearer)], (200, "user-1 via bearer")),
        ("cookie among others", get.clone(), vec![("cookie", &cookie)], (200, "user-1 via cookie")),
        ("garbage bearer", get.clone(), vec![("authorization", "Bearer nope")], (200, "anonymous")),
        ("expired cookie", get.clone(), vec![("cookie", &expired_cookie)], (200, "anonymous")),
        ("cookie with another name", get.clone(), vec![("cookie", &wrong_name)], (200, "anonymous")),
        (
            "cookie POST from our page",
            post.clone(),
            vec![("cookie", &cookie), ("origin", ORIGIN)],
            (200, "user-1 via cookie"),
        ),
        (
            "cookie POST from another site",
            post.clone(),
            vec![("cookie", &cookie), ("origin", "https://evil.example")],
            (403, ""),
        ),
        ("cookie POST with no Origin", post.clone(), vec![("cookie", &cookie)], (403, "")),
        ("bearer POST, no Origin (native)", post.clone(), vec![("authorization", &bearer)], (200, "user-1 via bearer")),
    ];

    for (name, method, headers, (status, body)) in cases {
        assert_eq!(call(method, &headers).await, (status, body.to_string()), "{name}");
    }
}
