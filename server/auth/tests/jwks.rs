//! Step 3: someone with only the JWKS can check our tokens.

use std::{sync::Arc, time::SystemTime};

use auth::{routes, Claims, Keys, Role, SESSION_SECS};
use axum::{body::Body, http::Request};
use ed25519_dalek::SigningKey;
use tower::ServiceExt;

#[tokio::test]
async fn jwks_lets_another_library_verify_our_tokens() {
    let old = SigningKey::from_bytes(&[2; 32]).verifying_key();
    let keys = Arc::new(Keys::new(SigningKey::from_bytes(&[1; 32]), vec![old]));

    let res = routes(keys.clone())
        .oneshot(Request::get("/.well-known/jwks.json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["cache-control"], "public, max-age=300");
    let body = axum::body::to_bytes(res.into_body(), 1 << 16).await.unwrap();

    // Current key and old key, public parts only.
    let set: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(set["keys"].as_array().unwrap().len(), 2);
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

    let set: jsonwebtoken::jwk::JwkSet = serde_json::from_slice(&body).unwrap();
    let kid = jsonwebtoken::decode_header(&token).unwrap().kid.unwrap();
    let jwk = set.find(&kid).expect("token's kid is in the JWKS");
    let key = jsonwebtoken::DecodingKey::from_jwk(jwk).unwrap();
    let validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::EdDSA);
    let decoded = jsonwebtoken::decode::<Claims>(&token, &key, &validation).unwrap();
    assert_eq!(decoded.claims, claims);
}
