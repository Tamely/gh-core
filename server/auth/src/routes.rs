//! Auth's HTTP routes. Their policies are in `policy::policies()`.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use serde_json::{json, Value};

use crate::{session_cookie, AuthState, Caller, Keys};

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
        .route("/api/v1/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/admin/users/{id}/revoke-sessions", post(revoke_user))
        .with_state(state)
}

async fn jwks_handler(State(state): State<Arc<AuthState>>) -> impl IntoResponse {
    // ponytail: 5 min cache; after a rotation, outside verifiers can miss the new kid for up to 5 min.
    ([(header::CACHE_CONTROL, "public, max-age=300")], Json(jwks(&state.keys)))
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
