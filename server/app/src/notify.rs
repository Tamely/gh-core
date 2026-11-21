//! `POST /api/v1/notify`: the landing page's "tell me when registration
//! opens" form. Stores the address; the email itself is sent later.

use std::convert::Infallible;

use axum::{
    Json,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header::CONTENT_TYPE},
    routing::{MethodRouter, post},
};
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::{AppState, db::DbError};

/// The landing page posts here cross-origin from this site.
pub const SITE_ORIGIN: &str = "https://geauxhack.com";
/// New addresses one client address may add per hour.
pub const PER_IP_PER_HOUR: i64 = 10;

#[derive(Deserialize)]
pub struct Signup {
    email: String,
}

pub fn route() -> MethodRouter<AppState> {
    post(signup)
        .layer::<_, Infallible>(DefaultBodyLimit::max(1024))
        .layer::<_, Infallible>(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_static(SITE_ORIGIN))
                .allow_methods([Method::POST])
                .allow_headers([CONTENT_TYPE])
                .max_age(std::time::Duration::from_secs(86_400)),
        )
}

/// 204 whether or not the address was already on the list, so the form can't
/// be used to check who signed up.
async fn signup(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Signup>,
) -> Result<StatusCode, DbError> {
    let Some(email) = normalize(&body.email) else {
        return Ok(StatusCode::BAD_REQUEST);
    };
    // The reverse proxy sets X-Real-IP, overwriting anything the client sent.
    // Without it (no proxy, e.g. local dev) every request shares one bucket,
    // so a missing header can't be used to dodge the limit.
    // ponytail: per-address only; a botnet with many addresses gets through. Add a
    // global cap if the table ever fills with junk.
    let ip = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 64)
        .unwrap_or("unknown");

    let mut conn = crate::db::acquire(&state.db).await?;
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notify_signups WHERE ip = $1 AND created_at > now() - interval '1 hour'",
    )
    .bind(ip)
    .fetch_one(&mut *conn)
    .await?;
    if recent >= PER_IP_PER_HOUR {
        return Ok(StatusCode::TOO_MANY_REQUESTS);
    }
    sqlx::query(
        "INSERT INTO notify_signups (email, ip) VALUES ($1, $2) ON CONFLICT (email) DO NOTHING",
    )
    .bind(&email)
    .bind(ip)
    .execute(&mut *conn)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Trimmed, lowercased, and shaped like an address (`a@b.c`, at most 254
/// characters, no spaces). Whether it really receives mail is only known when
/// the email is sent.
fn normalize(raw: &str) -> Option<String> {
    let email = raw.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    let ok = email.len() <= 254
        && !local.is_empty()
        && local.len() <= 64
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then_some(email)
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalizes_and_rejects_addresses() {
        assert_eq!(
            normalize("  Kenny@LSU.edu ").as_deref(),
            Some("kenny@lsu.edu")
        );
        assert_eq!(normalize("a+tag@x.io").as_deref(), Some("a+tag@x.io"));
        for bad in [
            "",
            "no-at-sign",
            "@lsu.edu",
            "kenny@",
            "kenny@lsu",
            "kenny@.edu",
            "kenny@lsu.",
            "a@b@c.com",
            "ken ny@lsu.edu",
            "ken\nny@lsu.edu",
        ] {
            assert_eq!(normalize(bad), None, "{bad:?}");
        }
        assert_eq!(normalize(&format!("{}@x.io", "a".repeat(65))), None);
        assert_eq!(normalize(&format!("a@{}.io", "b".repeat(250))), None);
    }
}
