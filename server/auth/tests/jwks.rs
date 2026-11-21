//! Step 3: someone with only the JWKS can check our tokens.

mod common;

use std::{sync::Arc, time::SystemTime};

use auth::{policies, AuthState, Claims, Keys, Role, Sessions, SESSION_SECS};
use axum::Router;
use common::*;
use ed25519_dalek::SigningKey;

#[tokio::test]
async fn jwks_lets_another_library_verify_our_tokens() {
    let old = SigningKey::from_bytes(&[2; 32]).verifying_key();
    let keys = Keys::new(SigningKey::from_bytes(&[1; 32]), vec![old]);
    // Public route: works with both databases down.
    let sessions = Sessions::new(dead_db(), dead_redis().await);
    let state = Arc::new(AuthState { keys, origin: ORIGIN.into(), now: || NOW, sessions, policies: policies() });

    let res = call(&app(state.clone(), Router::new()), "GET", "/.well-known/jwks.json", &[]).await;
    assert_eq!(res.status, 200);
    assert_eq!(res.headers["cache-control"], "public, max-age=300");
    let body = res.body.as_bytes();
    let keys = &state.keys;

    // Current key and old key, public parts only.
    let set: serde_json::Value = serde_json::from_slice(body).unwrap();
    // Exactly the current and old key, each with the right kid and x, and nothing secret.
    let b64 = |k: &ed25519_dalek::VerifyingKey| base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, k.as_bytes());
    let current = SigningKey::from_bytes(&[1; 32]).verifying_key();
    let mut want: Vec<(String, String)> = [current, old].iter().map(|k| (auth::kid(k), b64(k))).collect();
    let mut got: Vec<(String, String)> = set["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| (k["kid"].as_str().unwrap().into(), k["x"].as_str().unwrap().into()))
        .collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    assert!(set["keys"].as_array().unwrap().iter().all(|k| k.get("d").is_none()));

    // Verify with jsonwebtoken, using nothing but the JWKS response.
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i64;
    let claims = Claims {
        sub: "user-1".into(),
        roles: vec![Role::Volunteer],
        sid: "session-1".into(),
        iat: now,
        exp: now + SESSION_SECS,
        room_id: None,
    };
    let token = keys.issue(&claims);

    let set: jsonwebtoken::jwk::JwkSet = serde_json::from_slice(body).unwrap();
    let kid = jsonwebtoken::decode_header(&token).unwrap().kid.unwrap();
    let jwk = set.find(&kid).expect("token's kid is in the JWKS");
    let key = jsonwebtoken::DecodingKey::from_jwk(jwk).unwrap();
    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::EdDSA);
    let decoded = jsonwebtoken::decode::<Claims>(&token, &key, &validation).unwrap();
    assert_eq!(decoded.claims, claims);
}
