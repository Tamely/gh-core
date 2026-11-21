//! `POST /api/v1/notify` against real Postgres and Redis.

mod common;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
    response::Response,
};
use common::*;
use gh_server::notify::{PER_IP_PER_HOUR, SITE_ORIGIN};
use tower::ServiceExt;

async fn post(app: &Router, body: &str, headers: &[(&str, &str)]) -> Response {
    let mut req = Request::post("/api/v1/notify").header("content-type", "application/json");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap()
}

async fn signup(app: &Router, email: &str, ip: &str) -> StatusCode {
    let body = serde_json::json!({ "email": email }).to_string();
    post(app, &body, &[("x-real-ip", ip)]).await.status()
}

#[tokio::test]
async fn stores_each_address_once_and_limits_each_client() {
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let pool = migrated_pool(&db, 4).await;
    let app = gh_server::app(state(pool.clone(), &redis.url).await);

    // Stored trimmed and lowercased; a repeat is a quiet success.
    assert_eq!(
        signup(&app, "  Kenny@LSU.edu ", "1.1.1.1").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        signup(&app, "kenny@lsu.edu", "1.1.1.1").await,
        StatusCode::NO_CONTENT
    );
    let stored: Vec<String> = sqlx::query_scalar("SELECT email FROM notify_signups")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(stored, ["kenny@lsu.edu"]);

    // Bad input never reaches the table.
    assert_eq!(
        signup(&app, "not-an-email", "1.1.1.1").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        post(&app, "{}", &[]).await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        post(&app, "nope", &[]).await.status(),
        StatusCode::BAD_REQUEST
    );
    let huge = serde_json::json!({ "email": format!("{}@x.io", "a".repeat(2000)) }).to_string();
    assert_eq!(
        post(&app, &huge, &[]).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let form = app
        .clone()
        .oneshot(
            Request::post("/api/v1/notify")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("email=a%40b.io"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(form.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        send(&app, "/api/v1/notify").await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );

    // One client gets PER_IP_PER_HOUR new addresses an hour (one is used above).
    for n in 1..PER_IP_PER_HOUR {
        let email = format!("person{n}@lsu.edu");
        assert_eq!(
            signup(&app, &email, "1.1.1.1").await,
            StatusCode::NO_CONTENT,
            "{email}"
        );
    }
    assert_eq!(
        signup(&app, "one-too-many@lsu.edu", "1.1.1.1").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Others aren't affected.
    assert_eq!(
        signup(&app, "friend@lsu.edu", "2.2.2.2").await,
        StatusCode::NO_CONTENT
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM notify_signups")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, PER_IP_PER_HOUR + 1);

    // Without the proxy's header, everyone shares one bucket: leaving the
    // header out can't get around the limit.
    for n in 0..PER_IP_PER_HOUR {
        let body = serde_json::json!({ "email": format!("direct{n}@lsu.edu") }).to_string();
        assert_eq!(
            post(&app, &body, &[]).await.status(),
            StatusCode::NO_CONTENT
        );
    }
    let body = serde_json::json!({ "email": "direct-extra@lsu.edu" }).to_string();
    assert_eq!(
        post(&app, &body, &[]).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn only_the_landing_page_may_post_from_a_browser() {
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let app = gh_server::app(state(migrated_pool(&db, 4).await, &redis.url).await);

    let preflight = |origin: &'static str| {
        let app = app.clone();
        async move {
            app.oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/v1/notify")
                    .header("origin", origin)
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "content-type")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        }
    };

    let ok = preflight(SITE_ORIGIN).await;
    assert!(ok.status().is_success(), "{}", ok.status());
    assert_eq!(ok.headers()["access-control-allow-origin"], SITE_ORIGIN);
    let methods = ok.headers()["access-control-allow-methods"]
        .to_str()
        .unwrap();
    assert!(methods.contains("POST"), "{methods}");

    // Any other site is told the only allowed origin, which doesn't match its
    // own, so the browser refuses to send the POST.
    let other = preflight("https://evil.example").await;
    assert_eq!(other.headers()["access-control-allow-origin"], SITE_ORIGIN);

    // The real POST carries the header the browser checks before letting the
    // page read the response.
    let body = serde_json::json!({ "email": "cors@lsu.edu" }).to_string();
    let res = post(
        &app,
        &body,
        &[("origin", SITE_ORIGIN), ("x-real-ip", "3.3.3.3")],
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(res.headers()["access-control-allow-origin"], SITE_ORIGIN);
}
