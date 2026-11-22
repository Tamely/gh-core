//! Authentication and authorization for the gh-core API: session tokens, the
//! JWKS endpoint, sign-in codes, revocation, the per-route policy map and WebSocket topic rules.

mod caller;
mod codes;
mod oauth;
mod policy;
mod routes;
mod session;
mod token;
mod ws;

pub use caller::{identify, layer, session_cookie, AuthState, Caller, Via, COOKIE, RENEWED_TOKEN};
pub use codes::{user_for, CodeError, Codes, Dest, Sender, CODE_SECS, MAX_TRIES};
pub use oauth::OAuth;
pub use policy::{policies, Policies, Policy};
pub use routes::{jwks, routes};
pub use session::{Sessions, Unavailable, RENEW_SECS};
pub use token::{kid, Claims, Keys, Role, TokenError, SESSION_SECS};
pub use ws::{may_subscribe, ws_caller, ws_first_message};
