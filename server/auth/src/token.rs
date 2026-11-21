//! Session tokens: Ed25519-signed JWTs (`alg: EdDSA`, with a `kid` header).
//!
//! A token is `header.payload.signature`, each part base64url without padding.
//! The signature covers the exact bytes `header.payload`.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use ed25519_dalek::{pkcs8::DecodePrivateKey, Signature, Signer, SigningKey, VerifyingKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Session lifetime: 14 days.
pub const SESSION_SECS: i64 = 14 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    Hacker,
    Volunteer,
    Mentor,
    JudgeRoom,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claims {
    /// User ID.
    pub sub: String,
    pub roles: Vec<Role>,
    /// Session ID: the `sessions` row, used for revocation.
    pub sid: String,
    /// Issued at, unix seconds.
    pub iat: i64,
    /// Expires at, unix seconds.
    pub exp: i64,
    /// Only on `JUDGE_ROOM` sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
    /// Not three base64url parts, or the JSON inside is wrong.
    Malformed,
    /// Header `alg` isn't `EdDSA` (e.g. `none` or `HS256`).
    WrongAlgorithm,
    /// Header `kid` isn't one of our current or old keys.
    UnknownKey,
    BadSignature,
    Expired,
}

/// The current signing key plus every public key we still accept, by `kid`.
/// Deliberately no `Debug`: it holds the private key.
pub struct Keys {
    signing: SigningKey,
    signing_kid: String,
    verifying: BTreeMap<String, VerifyingKey>,
}

impl Keys {
    /// `private_pem`: the current private key as PKCS#8 PEM
    /// (`openssl genpkey -algorithm ed25519`).
    /// `old_public`: comma-separated base64url public keys (their JWKS `x`) that
    /// signed tokens which haven't expired yet. Empty if there was no rotation.
    pub fn load(private_pem: &str, old_public: &str) -> Result<Self, String> {
        let signing = SigningKey::from_pkcs8_pem(private_pem)
            .map_err(|e| format!("signing key is not an Ed25519 PKCS#8 PEM: {e}"))?;
        let old = old_public
            .split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| {
                B64.decode(x)
                    .ok()
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                    .and_then(|b| VerifyingKey::from_bytes(&b).ok())
                    .ok_or_else(|| format!("old public key {x:?} is not a base64url Ed25519 key"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(signing, old))
    }

    pub fn new(signing: SigningKey, old: Vec<VerifyingKey>) -> Self {
        let current = signing.verifying_key();
        let signing_kid = kid(&current);
        let verifying = std::iter::once(current).chain(old).map(|k| (kid(&k), k)).collect();
        Self { signing, signing_kid, verifying }
    }

    /// Every public key we accept, by `kid`, current one included.
    pub fn public_keys(&self) -> impl Iterator<Item = (&str, &VerifyingKey)> {
        self.verifying.iter().map(|(kid, key)| (kid.as_str(), key))
    }

    pub fn issue(&self, claims: &Claims) -> String {
        let header = serde_json::json!({ "alg": "EdDSA", "typ": "JWT", "kid": self.signing_kid });
        let payload = serde_json::to_vec(claims).expect("claims always serialize");
        let signed = format!("{}.{}", B64.encode(header.to_string()), B64.encode(payload));
        let signature = self.signing.sign(signed.as_bytes());
        format!("{signed}.{}", B64.encode(signature.to_bytes()))
    }

    /// Checks the signature and expiry. `now` is unix seconds, passed in so tests
    /// can use a fake clock. Doesn't check revocation; that needs the database.
    pub fn verify(&self, token: &str, now: i64) -> Result<Claims, TokenError> {
        #[derive(Deserialize)]
        struct Header {
            alg: String,
            kid: String,
        }

        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(TokenError::Malformed);
        };

        // Only EdDSA is ever accepted, whatever the token claims about itself.
        let h: Header = decode_json(header)?;
        if h.alg != "EdDSA" {
            return Err(TokenError::WrongAlgorithm);
        }
        let key = self.verifying.get(&h.kid).ok_or(TokenError::UnknownKey)?;

        let signature = B64
            .decode(signature)
            .ok()
            .and_then(|b| Signature::from_slice(&b).ok())
            .ok_or(TokenError::Malformed)?;
        let signed = &token[..header.len() + 1 + payload.len()];
        key.verify_strict(signed.as_bytes(), &signature)
            .map_err(|_| TokenError::BadSignature)?;

        let claims: Claims = decode_json(payload)?;
        if now >= claims.exp {
            return Err(TokenError::Expired);
        }
        Ok(claims)
    }
}

/// Key ID: the RFC 7638 thumbprint, i.e. SHA-256 of the key's JWK members
/// (sorted, no spaces), base64url. Same key, same `kid`, nothing to configure.
pub fn kid(key: &VerifyingKey) -> String {
    let jwk = format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#, B64.encode(key.as_bytes()));
    B64.encode(Sha256::digest(jwk))
}

fn decode_json<T: DeserializeOwned>(part: &str) -> Result<T, TokenError> {
    let bytes = B64.decode(part).map_err(|_| TokenError::Malformed)?;
    serde_json::from_slice(&bytes).map_err(|_| TokenError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::pkcs8::{spki::der::pem::LineEnding, EncodePrivateKey};
    use TokenError::*;

    const NOW: i64 = 1_800_000_000;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn claims() -> Claims {
        Claims {
            sub: "user-1".into(),
            roles: vec![Role::Hacker],
            sid: "session-1".into(),
            iat: NOW,
            exp: NOW + SESSION_SECS,
            room_id: None,
        }
    }

    /// Builds a token by hand, so tests can make ones `issue` never would.
    fn forge(header: serde_json::Value, claims: &Claims, signer: &SigningKey) -> String {
        let signed = format!(
            "{}.{}",
            B64.encode(header.to_string()),
            B64.encode(serde_json::to_vec(claims).unwrap())
        );
        format!("{signed}.{}", B64.encode(signer.sign(signed.as_bytes()).to_bytes()))
    }

    #[test]
    fn verify_accepts_only_good_tokens() {
        let a = Keys::new(key(1), vec![]);
        let b = Keys::new(key(2), vec![]);
        let kid_a = kid(&key(1).verifying_key());
        let good = a.issue(&claims());

        let mut admin = claims();
        admin.roles = vec![Role::Admin];
        let [h, _, s]: [&str; 3] = good.split('.').collect::<Vec<_>>().try_into().unwrap();
        let tampered = format!("{h}.{}.{s}", B64.encode(serde_json::to_vec(&admin).unwrap()));

        let cases: Vec<(&str, String, i64, Result<Claims, TokenError>)> = vec![
            ("valid", good.clone(), NOW, Ok(claims())),
            ("one second before expiry", good.clone(), NOW + SESSION_SECS - 1, Ok(claims())),
            ("at expiry", good.clone(), NOW + SESSION_SECS, Err(Expired)),
            ("payload edited to ADMIN", tampered, NOW, Err(BadSignature)),
            ("signed by a key we don't know", b.issue(&claims()), NOW, Err(UnknownKey)),
            (
                "our kid, someone else's signature",
                forge(serde_json::json!({ "alg": "EdDSA", "kid": kid_a }), &claims(), &key(2)),
                NOW,
                Err(BadSignature),
            ),
            (
                "alg none",
                format!(
                    "{}.{}.",
                    B64.encode(serde_json::json!({ "alg": "none", "kid": kid_a }).to_string()),
                    B64.encode(serde_json::to_vec(&claims()).unwrap())
                ),
                NOW,
                Err(WrongAlgorithm),
            ),
            (
                "alg HS256",
                forge(serde_json::json!({ "alg": "HS256", "kid": kid_a }), &claims(), &key(1)),
                NOW,
                Err(WrongAlgorithm),
            ),
            (
                "no kid",
                forge(serde_json::json!({ "alg": "EdDSA" }), &claims(), &key(1)),
                NOW,
                Err(Malformed),
            ),
            ("not a JWT", "hello".into(), NOW, Err(Malformed)),
            ("four parts", format!("{good}.x"), NOW, Err(Malformed)),
            ("padded signature", format!("{good}=="), NOW, Err(Malformed)),
        ];

        for (name, token, now, want) in cases {
            assert_eq!(a.verify(&token, now), want, "{name}");
        }
    }

    #[test]
    fn tokens_from_an_old_key_still_verify_after_rotation() {
        let before = Keys::new(key(1), vec![]);
        let token = before.issue(&claims());

        let after = Keys::new(key(2), vec![key(1).verifying_key()]);
        assert_eq!(after.verify(&token, NOW), Ok(claims()));
        // New tokens are signed with the new key.
        assert_eq!(Keys::new(key(2), vec![]).verify(&after.issue(&claims()), NOW), Ok(claims()));
    }

    #[test]
    fn room_id_is_only_in_judge_tokens() {
        let keys = Keys::new(key(1), vec![]);
        let payload = |c: &Claims| {
            let t = keys.issue(c);
            String::from_utf8(B64.decode(t.split('.').nth(1).unwrap()).unwrap()).unwrap()
        };
        assert!(!payload(&claims()).contains("room_id"));

        let mut judge = claims();
        judge.roles = vec![Role::JudgeRoom];
        judge.room_id = Some("PFT-1200".into());
        assert!(payload(&judge).contains(r#""roles":["JUDGE_ROOM"]"#));
        assert_eq!(keys.verify(&keys.issue(&judge), NOW), Ok(judge));
    }

    #[test]
    fn load_reads_pem_and_old_keys() {
        let pem = key(1).to_pkcs8_pem(LineEnding::LF).unwrap();
        let old = B64.encode(key(2).verifying_key().as_bytes());

        let keys = Keys::load(&pem, &format!(" {old} ,")).unwrap();
        assert_eq!(keys.public_keys().count(), 2);
        assert_eq!(keys.verify(&Keys::new(key(2), vec![]).issue(&claims()), NOW), Ok(claims()));

        assert!(Keys::load(&pem, "").unwrap().public_keys().count() == 1);
        assert!(Keys::load("not a pem", "").is_err());
        assert!(Keys::load(&pem, "c2hvcnQ").is_err());
    }

    #[test]
    fn kid_matches_the_rfc_example() {
        // RFC 8037 appendix A.3: the thumbprint of the RFC's example Ed25519 key.
        let x: [u8; 32] = B64.decode("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo").unwrap().try_into().unwrap();
        let key = VerifyingKey::from_bytes(&x).unwrap();
        assert_eq!(kid(&key), "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k");
    }
}
