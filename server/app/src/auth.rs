//! Who may open a WebSocket and subscribe to which topic. The auth PR replaces
//! `AllowAll` with role- and room-based rules (e.g. a judge room session may
//! only subscribe to its own `room:{id}`).

use axum::http::HeaderMap;

use crate::topic::Topic;

/// Who is on the other end of a connection. The auth PR fills this in from
/// the session cookie or bearer token.
#[derive(Clone, Debug, Default)]
pub struct Identity {
    pub subject: Option<String>,
}

pub trait Authorizer: Send + Sync {
    /// Called once per WebSocket upgrade. `None` refuses it with 401.
    fn identify(&self, headers: &HeaderMap) -> Option<Identity>;

    /// Called for every subscribe request.
    fn can_subscribe(&self, who: &Identity, topic: &Topic) -> bool;
}

/// Development only: everyone may subscribe to everything.
pub struct AllowAll;

impl Authorizer for AllowAll {
    fn identify(&self, _: &HeaderMap) -> Option<Identity> {
        Some(Identity::default())
    }

    fn can_subscribe(&self, _: &Identity, _: &Topic) -> bool {
        true
    }
}
