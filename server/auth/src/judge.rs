//! Judge room access: a judge scans the room's placard QR code and trades its
//! token for a `JUDGE_ROOM` session limited to that room, ending with judging.
//!
//! Token = base64url(room_id ‖ version ‖ HMAC-SHA256(K_judge, room_id ‖ version)),
//! unpadded. The last 32 bytes are the HMAC, the 4 before them the version (u32,
//! big-endian), the rest the room id. The database keeps only each room's current
//! version and the token's SHA-256, so a database leak can't print placards.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{rejection::JsonRejection, ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::{caller::cookie_with_age, routes::client_ip, AuthState, Claims, Role, Sessions};

const MAC_LEN: usize = 32;

/// K_judge, the placard HMAC key, from the secrets store. Separate from the
/// Ed25519 signing key. Deliberately no `Debug`.
pub struct JudgeKey([u8; 32]);

impl JudgeKey {
    pub fn new(key: [u8; 32]) -> Self {
        Self(key)
    }

    /// 32 random bytes as base64 or base64url, e.g. `openssl rand 32 | base64`.
    pub fn load(b64: &str) -> Result<Self, String> {
        let bytes = B64.decode(b64.trim().trim_end_matches('=').replace('+', "-").replace('/', "_")).map_err(|_| "JUDGE_KEY is not base64url")?;
        let key = bytes.try_into().map_err(|_| "JUDGE_KEY must be 32 bytes")?;
        Ok(Self(key))
    }

    fn mac(&self, msg: &[u8]) -> Hmac<Sha256> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC takes any key length");
        mac.update(msg);
        mac
    }

    /// The token on a room's placard at `version`.
    pub fn placard(&self, room_id: &str, version: u32) -> String {
        let mut msg = room_id.as_bytes().to_vec();
        msg.extend(version.to_be_bytes());
        let mac = self.mac(&msg).finalize().into_bytes();
        msg.extend(mac);
        B64.encode(msg)
    }

    /// Room and version, if we made this token. Rejections carry the room the
    /// token claimed (when it decoded) for the audit log.
    fn open(&self, token: &str) -> Result<(String, u32), (Option<String>, Outcome)> {
        let malformed = (None, Outcome::Malformed);
        let raw = B64.decode(token).map_err(|_| malformed.clone())?;
        if raw.len() <= 4 + MAC_LEN || raw.len() > 32 + 4 + MAC_LEN {
            return Err(malformed);
        }
        let (msg, mac) = raw.split_at(raw.len() - MAC_LEN);
        let (room, version) = msg.split_at(msg.len() - 4);
        // Same rule as the table, so a forged room id is safe to write to the audit log.
        if !room.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(malformed);
        }
        let room = String::from_utf8(room.to_vec()).expect("checked ASCII");
        let version = u32::from_be_bytes(version.try_into().expect("4 bytes"));
        // verify_slice compares in constant time.
        self.mac(msg).verify_slice(mac).map_err(|_| (Some(room.clone()), Outcome::BadSignature))?;
        Ok((room, version))
    }
}

/// Why an exchange was rejected, as stored in `judge_exchange_audit.outcome`.
/// The caller only ever sees 200 or one identical 401.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Malformed,
    BadSignature,
    UnknownRoom,
    OldVersion,
    OutsideWindow,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Malformed => "malformed",
            Outcome::BadSignature => "bad_signature",
            Outcome::UnknownRoom => "unknown_room",
            Outcome::OldVersion => "old_version",
            Outcome::OutsideWindow => "outside_window",
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ExchangeRequest {
    token: String,
}

/// Kept off the exchange response, so the placard URL never leaks to another site.
const NO_REFERRER: (header::HeaderName, &str) = (header::REFERRER_POLICY, "no-referrer");
const NO_STORE: (header::HeaderName, &str) = (header::CACHE_CONTROL, "no-store");

/// `POST /api/v1/judge/exchange` `{"token"}`: placard token in, room session out,
/// as `{id, roles, exp, roomId}` like `/me`. Our own web page gets the cookie,
/// anything else `token` in the body (same as sign-in by code). Every rejection
/// is the same 401, so it can't tell a forger which check failed.
pub(crate) async fn exchange(
    State(state): State<Arc<AuthState>>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    req: Result<Json<ExchangeRequest>, JsonRejection>,
) -> Response {
    let now = (state.now)();
    let db = &state.sessions.db;
    let user_agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(|s| s.chars().take(512).collect());
    let audit = Audit { now, user_agent, ip: client_ip(&headers, peer) };

    // A body that isn't `{"token": "..."}` is rejected like a bad token.
    let token = req.map(|Json(r)| r.token).unwrap_or_default();
    let (room_id, version) = match state.judge.open(&token) {
        Ok(opened) => opened,
        Err((room, outcome)) => return audit.reject(db, room, outcome).await,
    };
    let down = || StatusCode::SERVICE_UNAVAILABLE.into_response();
    let Ok(mut tx) = db.begin().await else { return down() };
    // FOR SHARE holds off a rotate until this session is written, so the
    // rotate's revoke sees it; a rotate that got there first shows its new version.
    let room: Option<(i32, Option<i64>, Option<i64>)> = match sqlx::query_as(
        "SELECT token_version,
                floor(extract(epoch FROM judging_starts_at))::bigint,
                floor(extract(epoch FROM judging_ends_at))::bigint
         FROM presentation_rooms WHERE room_id = $1 FOR SHARE",
    )
    .bind(&room_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(room) => room,
        Err(_) => return down(),
    };
    let rejected = match room {
        None => Err(Outcome::UnknownRoom),
        Some((current, ..)) if u32::try_from(current) != Ok(version) => Err(Outcome::OldVersion),
        Some((_, Some(starts), Some(ends))) if starts <= now && now < ends => Ok(ends),
        Some(_) => Err(Outcome::OutsideWindow),
    };
    let ends = match rejected {
        Ok(ends) => ends,
        Err(outcome) => {
            drop(tx);
            return audit.reject(db, Some(room_id), outcome).await;
        }
    };

    // Session and audit row in one statement, so one is never written without the other.
    let sid: String = match sqlx::query_scalar(
        "WITH s AS (
             INSERT INTO sessions (room_id, created_at, expires_at)
             VALUES ($1, to_timestamp($2), to_timestamp($3)) RETURNING id
         )
         INSERT INTO judge_exchange_audit (room_id, at, user_agent, ip, outcome, session_id)
         SELECT $1, to_timestamp($2), $4, $5, 'ok', id FROM s
         RETURNING session_id::text",
    )
    .bind(&room_id)
    .bind(now)
    .bind(ends)
    .bind(&audit.user_agent)
    .bind(&audit.ip)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(sid) => sid,
        Err(_) => return down(),
    };
    if tx.commit().await.is_err() {
        return down();
    }

    // `sub` must be set and a judge has no user: `judge:` keeps it from passing for a user id.
    let claims =
        Claims { sub: format!("judge:{sid}"), roles: vec![Role::JudgeRoom], sid, iat: now, exp: ends, room_id: Some(room_id) };
    let token = state.keys.issue(&claims);
    let mut body = json!({ "id": claims.sub, "roles": claims.roles, "exp": claims.exp, "roomId": claims.room_id });
    if headers.get(header::ORIGIN).is_some_and(|o| o == state.origin.as_str()) {
        ([NO_STORE, NO_REFERRER], [(header::SET_COOKIE, cookie_with_age(&token, ends - now))], Json(body)).into_response()
    } else {
        body["token"] = token.into();
        ([NO_STORE, NO_REFERRER], Json(body)).into_response()
    }
}

/// The device and time of one exchange. Never the token.
struct Audit {
    now: i64,
    user_agent: Option<String>,
    ip: Option<String>,
}

impl Audit {
    async fn reject(&self, db: &PgPool, room: Option<String>, outcome: Outcome) -> Response {
        // ponytail: one write per rejected attempt; add a per-IP limit if junk floods the table.
        // A failed write still rejects: the answer is 401 either way.
        let _ = sqlx::query(
            "INSERT INTO judge_exchange_audit (room_id, at, user_agent, ip, outcome) VALUES ($1, to_timestamp($2), $3, $4, $5)",
        )
        .bind(room)
        .bind(self.now)
        .bind(&self.user_agent)
        .bind(&self.ip)
        .bind(outcome.as_str())
        .execute(db)
        .await;
        (StatusCode::UNAUTHORIZED, [NO_STORE, NO_REFERRER]).into_response()
    }
}

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Admin: creates a room (placard version 1), or moves its judging window.
/// Times are unix seconds; judges are let in from `starts` until `ends`.
pub async fn set_room(db: &PgPool, key: &JudgeKey, room_id: &str, starts: i64, ends: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO presentation_rooms (room_id, token_version, token_hash, judging_starts_at, judging_ends_at)
         VALUES ($1, 1, $2, to_timestamp($3), to_timestamp($4))
         ON CONFLICT (room_id) DO UPDATE
         SET judging_starts_at = EXCLUDED.judging_starts_at, judging_ends_at = EXCLUDED.judging_ends_at",
    )
    .bind(room_id)
    .bind(hash(&key.placard(room_id, 1)))
    .bind(starts)
    .bind(ends)
    .execute(db)
    .await
    .map(drop)
}

/// Admin: a new placard for a room. The old one stops working at once, and so
/// does every judge session it handed out (a lost phone is cut off by the same
/// command). Returns the new token, or `None` for an unknown room.
pub async fn rotate(sessions: &Sessions, key: &JudgeKey, room_id: &str, now: i64) -> Result<Option<String>, String> {
    let db = &sessions.db;
    let Some(current): Option<i32> = sqlx::query_scalar("SELECT token_version FROM presentation_rooms WHERE room_id = $1")
        .bind(room_id)
        .fetch_optional(db)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    let next = current + 1;
    let token = key.placard(room_id, next as u32);
    // Only if nobody rotated in between; otherwise this placard is already stale.
    let moved = sqlx::query(
        "UPDATE presentation_rooms SET token_version = $2, token_hash = $3 WHERE room_id = $1 AND token_version = $2 - 1",
    )
    .bind(room_id)
    .bind(next)
    .bind(hash(&token))
    .execute(db)
    .await
    .map_err(|e| e.to_string())?
    .rows_affected();
    if moved != 1 {
        return Err(format!("{room_id} was rotated by someone else at the same time; run it again"));
    }
    sessions.revoke_room(room_id, now).await.map_err(|_| {
        format!("{room_id} has a new placard (judge-admin print shows it), but its old sessions couldn't be revoked; run rotate again")
    })?;
    Ok(Some(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placards_open_only_with_our_key_and_untouched() {
        let key = JudgeKey::new([9; 32]);
        let token = key.placard("PFT-1200", 7);
        assert_eq!(key.open(&token), Ok(("PFT-1200".into(), 7)));
        assert!(!token.contains('=') && !token.contains('+') && !token.contains('/'), "unpadded base64url");

        // Someone else's key.
        let forged = JudgeKey::new([1; 32]).placard("PFT-1200", 7);
        assert_eq!(key.open(&forged), Err((Some("PFT-1200".into()), Outcome::BadSignature)));

        // Any changed byte: room, version or HMAC.
        let raw = B64.decode(&token).unwrap();
        for i in [0, raw.len() - MAC_LEN - 1, raw.len() - 1] {
            let mut bad = raw.clone();
            bad[i] ^= 1;
            assert_eq!(key.open(&B64.encode(&bad)).unwrap_err().1, Outcome::BadSignature, "byte {i}");
        }

        for junk in ["", "not a token!", &B64.encode([0u8; 36]), &format!("{token}=")] {
            assert_eq!(key.open(junk), Err((None, Outcome::Malformed)), "{junk:?}");
        }
        // Room ids the table wouldn't allow never reach the audit log.
        assert_eq!(key.open(&key.placard("../etc", 1)), Err((None, Outcome::Malformed)));
        assert_eq!(key.open(&key.placard(&"A".repeat(33), 1)), Err((None, Outcome::Malformed)));
    }

    #[test]
    fn key_loads_from_base64url() {
        let b64 = B64.encode([5u8; 32]);
        assert_eq!(JudgeKey::load(&b64).unwrap().0, [5; 32]);
        assert_eq!(JudgeKey::load(&format!("{b64}=\n")).unwrap().0, [5; 32], "padding and newline from openssl");
        assert!(JudgeKey::load(&B64.encode([5u8; 16])).is_err(), "too short");
        assert!(JudgeKey::load("not base64!").is_err());
    }
}
