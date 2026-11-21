//! Who is calling, and may they? One layer in front of every route:
//!
//! 1. Read the token from the cookie (web) or `Authorization: Bearer` (native).
//! 2. Cookie requests that change data must come from our own page (`Origin`).
//! 3. Look up the route's policy; no entry means 403.
//! 4. Drop revoked sessions (Redis, else Postgres, else 503).
//! 5. Check the policy, run the handler, renew the session if it's due.

use std::sync::Arc;

use axum::{
    extract::{MatchedPath, RawPathParams, Request, State},
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    RequestExt,
};

use crate::{Claims, Codes, Keys, OAuth, Policies, Policy, Sessions, SESSION_SECS};

/// Web session cookie. `__Host-` makes browsers insist on Secure, Path=/ and no Domain.
pub const COOKIE: &str = "__Host-session";

/// Native clients read a renewed token from this response header and store it.
pub const RENEWED_TOKEN: HeaderName = HeaderName::from_static("x-session-token");

#[derive(Debug, Clone, PartialEq)]
pub enum Caller {
    /// No token, or one that's invalid, expired or revoked.
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
    pub sessions: Sessions,
    /// Start from `auth::policies()` and add every server route.
    pub policies: Policies,
    /// Email and SMS sign-in codes.
    pub codes: Codes,
    pub oauth: OAuth,
}

/// `Set-Cookie` value for a session token. `Max-Age=0` with an empty token clears it.
pub fn session_cookie(token: &str) -> HeaderValue {
    let max_age = if token.is_empty() { 0 } else { SESSION_SECS };
    let cookie = format!("{COOKIE}={token}; Secure; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age}");
    HeaderValue::from_str(&cookie).expect("a JWT is valid in a header")
}

/// Signature and expiry only, no revocation. Bearer wins if both are present:
/// only native sends it, and it can't be forged cross-site.
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

/// A revoked session becomes `Anonymous`; 503 if revocation can't be checked.
pub(crate) async fn unrevoked(state: &AuthState, caller: Caller, now: i64) -> Result<Caller, StatusCode> {
    let Caller::Signed { claims, .. } = &caller else { return Ok(caller) };
    match state.sessions.is_revoked(&claims.sid, now).await {
        Ok(false) => Ok(caller),
        Ok(true) => Ok(Caller::Anonymous),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// True if a cookie caller's `Origin` isn't our web app's.
pub(crate) fn cross_site(state: &AuthState, caller: &Caller, headers: &HeaderMap) -> bool {
    matches!(caller, Caller::Signed { via: Via::Cookie, .. })
        && headers.get(header::ORIGIN).is_none_or(|o| o != state.origin.as_str())
}

/// Axum middleware for the whole app (`Router::layer`, so it sees the matched route):
/// `.layer(axum::middleware::from_fn_with_state(state, auth::layer))`.
/// Handlers read the caller with `Extension<Caller>`.
///
/// Call it **last**, after every `.route`/`.merge`/`.nest`: `Router::layer` only
/// wraps routes that already exist, so a route added afterwards skips auth.
/// Use `nest(Router)`, not `nest_service`: nested services have no matched
/// route, so everything under them is 403.
pub async fn layer(State(state): State<Arc<AuthState>>, mut req: Request, next: Next) -> Response {
    let now = (state.now)();
    let caller = identify(req.headers(), &state.keys, now);

    // SameSite=Lax already stops most cross-site requests; this is the second lock.
    let changes_data = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    // Apple's form_post callback only relays code/state to the client; it never
    // changes a session. Completion still requires the client's retained verifier.
    let apple_relay = *req.method() == Method::POST
        && req.extensions().get::<MatchedPath>().is_some_and(|p| p.as_str() == "/auth/callback");
    if changes_data && !apple_relay && cross_site(&state, &caller, req.headers()) {
        return StatusCode::FORBIDDEN.into_response();
    }

    let method = if req.method() == Method::HEAD { &Method::GET } else { req.method() };
    let route = req.extensions().get::<MatchedPath>().map(|p| format!("{method} {}", p.as_str()));
    let Some(policy) = route.and_then(|r| state.policies.get(&r)) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if *policy == Policy::Public {
        req.extensions_mut().insert(Caller::Anonymous);
        return next.run(req).await;
    }

    let caller = match unrevoked(&state, caller, now).await {
        Ok(caller) => caller,
        Err(status) => return status.into_response(),
    };
    let params = req.extract_parts::<RawPathParams>().await.ok();
    let room_id = params.as_ref().and_then(|p| p.iter().find(|(k, _)| *k == "room_id").map(|(_, v)| v));
    if let Err(status) = policy.check(&caller, room_id) {
        return status.into_response();
    }

    req.extensions_mut().insert(caller.clone());
    let mut res = next.run(req).await;

    if let Caller::Signed { claims, via } = caller {
        // A failed renewal isn't worth failing the request for; the next one retries.
        if let Ok(Some(renewed)) = state.sessions.renew(&claims, now).await {
            let token = state.keys.issue(&renewed);
            let headers = res.headers_mut();
            if via == Via::Cookie {
                headers.append(header::SET_COOKIE, session_cookie(&token));
            } else {
                headers.insert(RENEWED_TOKEN, HeaderValue::from_str(&token).expect("a JWT is valid in a header"));
            }
        }
    }
    res
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    // The scheme name is case-insensitive (RFC 7235): "bearer x" works too.
    let (scheme, token) = headers.get(header::AUTHORIZATION)?.to_str().ok()?.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then_some(token)
}

fn cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| c.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}
