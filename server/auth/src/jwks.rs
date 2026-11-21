//! `GET /.well-known/jwks.json`: the public keys anyone can use to check our tokens.

use std::sync::Arc;

use axum::{extract::State, http::header, response::IntoResponse, routing::get, Json, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use serde_json::{json, Value};

use crate::Keys;

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

/// Merge into the app's router. Policy: Public.
pub fn routes(keys: Arc<Keys>) -> Router {
    Router::new().route("/.well-known/jwks.json", get(handler)).with_state(keys)
}

async fn handler(State(keys): State<Arc<Keys>>) -> impl IntoResponse {
    // ponytail: 5 min cache; after a rotation, outside verifiers can miss the new kid for up to 5 min.
    ([(header::CACHE_CONTROL, "public, max-age=300")], Json(jwks(&keys)))
}
