//! Who may call each route. Every route needs an entry in the policy map, keyed
//! `"METHOD /matched/path"` (e.g. `"GET /api/v1/me"`); a route with no entry is 403.

use std::collections::HashMap;

use axum::http::StatusCode;

use crate::{Caller, Role};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Policy {
    /// Anyone. The handler always sees `Caller::Anonymous`, so it can't
    /// accidentally depend on who's calling.
    Public,
    /// Any signed-in, unrevoked session.
    SignedIn,
    /// Signed in with at least one of these roles.
    RequireRole(Vec<Role>),
    /// A judge session for the route's `{room_id}`, or an admin.
    RequireRoom,
}

pub type Policies = HashMap<String, Policy>;

impl Policy {
    /// 401 if nobody is signed in, 403 if they are but aren't allowed.
    pub fn check(&self, caller: &Caller, room_id: Option<&str>) -> Result<(), StatusCode> {
        let claims = match (self, caller) {
            (Policy::Public, _) => return Ok(()),
            (_, Caller::Anonymous) => return Err(StatusCode::UNAUTHORIZED),
            (_, Caller::Signed { claims, .. }) => claims,
        };
        let has = |role| claims.roles.contains(&role);
        let allowed = match self {
            Policy::Public | Policy::SignedIn => true,
            Policy::RequireRole(roles) => roles.iter().any(|&r| has(r)),
            Policy::RequireRoom => {
                has(Role::Admin)
                    || (has(Role::JudgeRoom) && room_id.is_some() && claims.room_id.as_deref() == room_id)
            }
        };
        allowed.then_some(()).ok_or(StatusCode::FORBIDDEN)
    }
}

/// Policies for auth's own routes, plus Kenny's health checks and WebSocket.
/// The server extends this map with an entry for each route it adds.
pub fn policies() -> Policies {
    [
        ("GET /.well-known/jwks.json", Policy::Public),
        ("GET /healthz", Policy::Public),
        ("GET /readyz", Policy::Public),
        // The socket checks the caller itself (cookie on upgrade, or the first
        // message on native) with `auth::ws_caller` / `auth::ws_first_message`.
        ("GET /api/v1/ws", Policy::Public),
        // Sign-in by code. Rate limited inside, per address and per IP.
        ("POST /api/v1/auth/code", Policy::Public),
        ("POST /api/v1/auth/code/verify", Policy::Public),
        ("GET /api/v1/me", Policy::SignedIn),
        ("POST /api/v1/auth/logout", Policy::SignedIn),
        ("POST /api/v1/admin/users/{id}/revoke-sessions", Policy::RequireRole(vec![Role::Admin])),
    ]
    .into_iter()
    .map(|(route, policy)| (route.to_string(), policy))
    .collect()
}
