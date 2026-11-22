//! Auth's HTTP routes. Their policies are in `policy::policies()`.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{session_cookie, user_for, AuthState, Caller, Dest, Keys};

/// The JWK Set: one entry per accepted key. Public parts only, never `d`.
pub fn jwks(keys: &Keys) -> Value {
    let keys: Vec<Value> = keys
        .public_keys()
        .map(|(kid, key)| {
            json!({
                "kty": "OKP",
                "crv": "Ed25519",
                "alg": "EdDSA",
                "use": "sig",
                "kid": kid,
                "x": B64.encode(key.as_bytes()),
            })
        })
        .collect();
    json!({ "keys": keys })
}

/// Merge into the app's router, under the `auth::layer`.
pub fn routes(state: Arc<AuthState>) -> Router {
    Router::new()
        .route("/.well-known/jwks.json", get(jwks_handler))
        .route("/api/v1/auth/code", post(send_code))
        .route("/api/v1/auth/code/verify", post(verify_code))
        .route("/api/v1/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/admin/users/{id}/revoke-sessions", post(revoke_user))
        .route("/api/v1/judge/exchange", post(crate::judge::exchange))
        .with_state(state)
}

async fn jwks_handler(State(state): State<Arc<AuthState>>) -> impl IntoResponse {
    // ponytail: 5 min cache; after a rotation, outside verifiers can miss the new kid for up to 5 min.
    ([(header::CACHE_CONTROL, "public, max-age=300")], Json(jwks(&state.keys)))
}

/// `{"email"}` or `{"phone"}` (E.164), plus `"code"` when verifying.
#[derive(Deserialize)]
struct CodeRequest {
    email: Option<String>,
    phone: Option<String>,
    code: Option<String>,
}

impl CodeRequest {
    fn dest(&self) -> Option<Dest> {
        match (&self.email, &self.phone) {
            (Some(email), None) => Dest::email(email),
            (None, Some(phone)) => Dest::phone(phone),
            _ => None,
        }
    }
}

/// The caller's IP, for rate limits: the last `X-Forwarded-For` entry (the one
/// our own proxy added; earlier ones are whatever the client sent), else the TCP peer.
/// `None` means the server is wired wrong (no proxy header and no
/// `into_make_service_with_connect_info`); guessing would put everyone in one bucket.
pub(crate) fn client_ip(headers: &HeaderMap, peer: Option<Extension<ConnectInfo<SocketAddr>>>) -> Option<String> {
    let forwarded = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .last()
        .map(|ip| ip.trim().to_string())
        .filter(|ip| !ip.is_empty());
    forwarded.or(peer.map(|p| p.0 .0.ip().to_string()))
}

/// 500 for a server that can't see client IPs, so it's caught on the first try.
const NO_IP: StatusCode = StatusCode::INTERNAL_SERVER_ERROR;

/// Sends a sign-in code. 202 whether or not the account exists (first sign-in
/// creates it), so this can't be used to find out who has one.
async fn send_code(
    State(state): State<Arc<AuthState>>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(req): Json<CodeRequest>,
) -> StatusCode {
    let Some(to) = req.dest() else { return StatusCode::BAD_REQUEST };
    let Some(ip) = client_ip(&headers, peer) else { return NO_IP };
    match state.codes.send(&state.sessions.redis, &to, &ip, (state.now)()).await {
        Ok(()) => StatusCode::ACCEPTED,
        Err(e) => e.status(),
    }
}

/// Checks the code and signs in: `{id, roles, exp}` like `/me`. Our own web page
/// gets the session cookie; anything else (native) gets `token` in the body.
/// A page on another site can't get either: no cookie is set for its `Origin`,
/// and the browser won't let it read the body.
async fn verify_code(
    State(state): State<Arc<AuthState>>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    Json(req): Json<CodeRequest>,
) -> Response {
    let (Some(to), Some(code)) = (req.dest(), &req.code) else { return StatusCode::BAD_REQUEST.into_response() };
    let Some(ip) = client_ip(&headers, peer) else { return NO_IP.into_response() };
    let now = (state.now)();
    match state.codes.check(&state.sessions.redis, &to, code, &ip, now).await {
        Ok(true) => {}
        Ok(false) => return StatusCode::UNAUTHORIZED.into_response(),
        Err(e) => return e.status().into_response(),
    }
    // The code is used up by now; if the database fails, the user asks for a new one.
    let Ok(id) = user_for(&state.sessions.db, &to).await else { return StatusCode::SERVICE_UNAVAILABLE.into_response() };
    let Ok(claims) = state.sessions.start(&id, now).await else { return StatusCode::SERVICE_UNAVAILABLE.into_response() };
    let token = state.keys.issue(&claims);
    let mut body = json!({ "id": claims.sub, "roles": claims.roles, "exp": claims.exp });
    let no_store = (header::CACHE_CONTROL, "no-store");
    if headers.get(header::ORIGIN).is_some_and(|o| o == state.origin.as_str()) {
        ([no_store], [(header::SET_COOKIE, session_cookie(&token))], Json(body)).into_response()
    } else {
        body["token"] = token.into();
        ([no_store], Json(body)).into_response()
    }
}

/// The signed-in user, for the client's profile snapshot. More fields come with sign-in.
async fn me(Extension(caller): Extension<Caller>) -> impl IntoResponse {
    let Caller::Signed { claims, .. } = caller else { return StatusCode::UNAUTHORIZED.into_response() };
    let mut body = json!({ "id": claims.sub, "roles": claims.roles, "exp": claims.exp });
    if let Some(room) = claims.room_id {
        body["roomId"] = room.into();
    }
    Json(body).into_response()
}

/// Revokes this session and clears the web cookie. 503 means try again.
async fn logout(State(state): State<Arc<AuthState>>, Extension(caller): Extension<Caller>) -> impl IntoResponse {
    let Caller::Signed { claims, .. } = caller else { return StatusCode::UNAUTHORIZED.into_response() };
    match state.sessions.revoke(&claims.sid, (state.now)()).await {
        Ok(()) => (StatusCode::NO_CONTENT, [(header::SET_COOKIE, session_cookie(""))]).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// Admin: signs a user out everywhere. 204 even if they had no sessions.
async fn revoke_user(State(state): State<Arc<AuthState>>, Path(id): Path<String>) -> StatusCode {
    // Checked here so a typo is a 400, not a database error.
    if !is_uuid(&id) {
        return StatusCode::BAD_REQUEST;
    }
    match state.sessions.revoke_user(&id, (state.now)()).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_ip_is_the_entry_our_proxy_added() {
        let peer = || Some(Extension(ConnectInfo(SocketAddr::from(([10, 0, 0, 9], 4000)))));
        let ip = |xff: &[&str]| {
            let mut headers = HeaderMap::new();
            for v in xff {
                headers.append("x-forwarded-for", v.parse().unwrap());
            }
            client_ip(&headers, peer())
        };
        let some = |s: &str| Some(s.to_string());
        // A client can put anything in front; the proxy's entry is last.
        assert_eq!(ip(&["1.1.1.1"]), some("1.1.1.1"));
        assert_eq!(ip(&["6.6.6.6, 1.1.1.1"]), some("1.1.1.1"));
        assert_eq!(ip(&["6.6.6.6", "1.1.1.1"]), some("1.1.1.1"));
        assert_eq!(ip(&[]), some("10.0.0.9"), "no proxy: the TCP peer");
        assert_eq!(ip(&["1.1.1.1, "]), some("10.0.0.9"), "an empty entry isn't an IP");
        // Wired wrong: no shared "" bucket for everyone.
        assert_eq!(client_ip(&HeaderMap::new(), None), None);
    }
}
