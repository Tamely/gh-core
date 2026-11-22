//! WebSocket auth for Kenny's `/api/v1/ws`, replacing `auth::AllowAll`.
//!
//! Browsers can't add headers to a WebSocket, so web signs in with the cookie on
//! the upgrade request. Native connects without credentials and sends its token
//! as the first message (never in the URL, which ends up in logs).

use axum::http::{HeaderMap, StatusCode};

use crate::{
    caller::{cross_site, unrevoked},
    identify, AuthState, Caller, Role, Via,
};

/// On the upgrade request. 403 if the cookie comes from another site's page
/// (otherwise any site could open a socket as you); 503 if revocation can't be
/// checked. No credentials is `Anonymous`: native will send its token next.
pub async fn ws_caller(state: &AuthState, headers: &HeaderMap) -> Result<Caller, StatusCode> {
    let now = (state.now)();
    let caller = identify(headers, &state.keys, now);
    if cross_site(state, &caller, headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    unrevoked(state, caller, now).await
}

/// Native's first message is the bare token. A bad one is `Anonymous`.
pub async fn ws_first_message(state: &AuthState, token: &str) -> Result<Caller, StatusCode> {
    let now = (state.now)();
    let caller = match state.keys.verify(token.trim(), now) {
        Ok(claims) => Caller::Signed { claims, via: Via::Bearer },
        Err(_) => Caller::Anonymous,
    };
    unrevoked(state, caller, now).await
}

/// May this caller receive events on this topic?
/// ponytail: checked when subscribing only; an open socket keeps its topics after
/// a revoke until it reconnects. Re-check on each event if that matters.
pub fn may_subscribe(caller: &Caller, topic: &str) -> bool {
    let Caller::Signed { claims, .. } = caller else { return false };
    let admin = claims.roles.contains(&Role::Admin);
    match topic.split_once(':') {
        Some(("user", id)) => id == claims.sub,
        Some(("room", id)) => admin || (claims.roles.contains(&Role::JudgeRoom) && claims.room_id.as_deref() == Some(id)),
        // team:* and mentor-tag:* need membership tables that don't exist yet.
        Some(("team" | "mentor-tag", _)) => admin,
        _ => false,
    }
}
