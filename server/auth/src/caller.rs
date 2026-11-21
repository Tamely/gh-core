//! Who is calling: read the session token from the cookie (web) or the
//! `Authorization: Bearer` header (native), and put a [`Caller`] on the request.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{Claims, Keys};

/// Web session cookie. `__Host-` makes browsers insist on Secure, Path=/ and no Domain.
pub const COOKIE: &str = "__Host-session";

#[derive(Debug, Clone, PartialEq)]
pub enum Caller {
    /// No token, or one that's invalid or expired.
    Anonymous,
    Signed { claims: Claims, via: Via },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Cookie,
    Bearer,
}

pub struct AuthState {
    pub keys: Keys,
    /// The web app's origin, e.g. `https://app.geauxhack.com`.
    pub origin: String,
    /// Unix seconds. Real servers pass the system clock; tests pass a fixed time.
    pub now: fn() -> i64,
}

/// Bearer wins if both are present: only native sends it, and it can't be forged cross-site.
pub fn identify(headers: &HeaderMap, keys: &Keys, now: i64) -> Caller {
    let (token, via) = match (bearer(headers), cookie(headers)) {
        (Some(t), _) => (t, Via::Bearer),
        (None, Some(t)) => (t, Via::Cookie),
        (None, None) => return Caller::Anonymous,
    };
    match keys.verify(token, now) {
        Ok(claims) => Caller::Signed { claims, via },
        Err(_) => Caller::Anonymous,
    }
}

/// Axum middleware: `.layer(axum::middleware::from_fn_with_state(state, auth::layer))`.
/// Handlers read the result with `Extension<Caller>`.
///
/// Cookie requests that change data must come from our own page (`Origin` matches).
/// SameSite=Lax already stops most cross-site requests; this is the second lock.
pub async fn layer(State(state): State<Arc<AuthState>>, mut req: Request, next: Next) -> Response {
    let caller = identify(req.headers(), &state.keys, (state.now)());
    let changes_data = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let from_our_page = req.headers().get(header::ORIGIN).is_some_and(|o| o == state.origin.as_str());
    if matches!(caller, Caller::Signed { via: Via::Cookie, .. }) && changes_data && !from_our_page {
        return StatusCode::FORBIDDEN.into_response();
    }
    req.extensions_mut().insert(caller);
    next.run(req).await
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

fn cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}
