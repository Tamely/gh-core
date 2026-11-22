//! Sessions: the `sessions` row behind every token, revocation and sliding renewal.
//!
//! Postgres is the truth. Redis caches revoked session IDs (`gh:revoked:{sid}`,
//! expiring when the session would have) so a revoke takes effect on the very
//! next request without a database query on every request.
//!
//! Redis keeps nothing on disk, so after it restarts the cache is empty. The
//! `gh:revoked:ready` key says "the cache is loaded": while it's missing, lookups
//! go to Postgres and one request reloads the cache.

use std::time::Duration;

use fred::prelude::{Client, KeysInterface};
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::{Claims, Role, SESSION_SECS};

/// Renew when less than this is left: 7 days.
pub const RENEW_SECS: i64 = 7 * 24 * 60 * 60;

const READY: &str = "gh:revoked:ready";

fn key(sid: &str) -> String {
    format!("gh:revoked:{sid}")
}

async fn cache(redis: &Client, sid: &str, ttl: i64) -> Result<(), fred::error::Error> {
    redis.set(key(sid), 1, Some(fred::types::Expiration::EX(ttl.max(1))), None, false).await
}

/// Neither Redis nor Postgres answered, so we can't tell if a session was
/// revoked. Callers return 503; never treat it as "not revoked".
#[derive(Debug, PartialEq, Eq)]
pub struct Unavailable;

pub struct Sessions {
    pub db: PgPool,
    /// Kenny's shared fred client (1s command timeout).
    pub redis: Client,
    /// Only one reload at a time; other requests ask Postgres meanwhile.
    reloading: Mutex<()>,
}

impl Sessions {
    pub fn new(db: PgPool, redis: Client) -> Self {
        Self { db, redis, reloading: Mutex::new(()) }
    }

    /// Opens a session for a user and returns the claims to sign.
    pub async fn start(&self, user_id: &str, now: i64) -> Result<Claims, sqlx::Error> {
        let roles = self.roles(user_id).await?;
        let exp = now + SESSION_SECS;
        let sid: String = sqlx::query_scalar(
            "INSERT INTO sessions (user_id, created_at, expires_at)
             VALUES ($1::uuid, to_timestamp($2), to_timestamp($3)) RETURNING id::text",
        )
        .bind(user_id)
        .bind(now)
        .bind(exp)
        .fetch_one(&self.db)
        .await?;
        Ok(Claims { sub: user_id.into(), roles, sid, iat: now, exp, room_id: None })
    }

    async fn roles(&self, user_id: &str) -> Result<Vec<Role>, sqlx::Error> {
        let roles: Vec<String> =
            sqlx::query_scalar("SELECT role FROM user_roles WHERE user_id = $1::uuid ORDER BY role")
                .bind(user_id)
                .fetch_all(&self.db)
                .await?;
        // The table's CHECK only allows real roles, so nothing is dropped here.
        Ok(roles.into_iter().filter_map(|r| serde_json::from_value(r.into()).ok()).collect())
    }

    /// Logout: revokes one session.
    pub async fn revoke(&self, sid: &str, now: i64) -> Result<(), Unavailable> {
        self.revoke_where("id = $1::uuid", sid, now).await
    }

    /// Admin cut-off: revokes every session a user has.
    pub async fn revoke_user(&self, user_id: &str, now: i64) -> Result<(), Unavailable> {
        self.revoke_where("user_id = $1::uuid", user_id, now).await
    }

    /// Placard rotated: revokes every judge session for that room.
    pub async fn revoke_room(&self, room_id: &str, now: i64) -> Result<(), Unavailable> {
        self.revoke_where("room_id = $1", room_id, now).await
    }

    /// `Unavailable` only if Postgres failed (nothing was revoked; retry). Once the
    /// database says revoked it's done: if Redis can't take it, "ready" is dropped
    /// so lookups go to Postgres, and the cache write is retried in the background.
    async fn revoke_where(&self, condition: &str, id: &str, now: i64) -> Result<(), Unavailable> {
        let revoked: Vec<(String, i64)> = sqlx::query_as(&format!(
            "UPDATE sessions SET revoked_at = COALESCE(revoked_at, to_timestamp($2))
             WHERE {condition} AND expires_at > to_timestamp($2)
             RETURNING id::text, extract(epoch FROM expires_at)::bigint"
        ))
        .bind(id)
        .bind(now)
        .fetch_all(&self.db)
        .await
        .map_err(|_| Unavailable)?;
        for (sid, exp) in revoked {
            if cache(&self.redis, &sid, exp - now).await.is_err() {
                let _: Result<(), _> = self.redis.del(READY).await;
                self.retry_cache(sid, exp - now);
            }
        }
        Ok(())
    }

    /// Keeps trying once a second until Redis takes the key or the session would
    /// have expired anyway. Covers a Redis blip where even dropping "ready" failed.
    fn retry_cache(&self, sid: String, ttl: i64) {
        let redis = self.redis.clone();
        tokio::spawn(async move {
            for left in (1..ttl).rev() {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if cache(&redis, &sid, left).await.is_ok() {
                    return;
                }
            }
        });
    }

    /// Fills the cache from Postgres. Runs on startup and whenever Redis lost it.
    pub async fn reload(&self, now: i64) -> Result<(), Unavailable> {
        let _one = self.reloading.lock().await;
        self.reload_locked(now).await
    }

    async fn reload_locked(&self, now: i64) -> Result<(), Unavailable> {
        self.cache_revoked(now).await?;
        self.redis.set::<(), _, _>(READY, 1, None, None, false).await.map_err(|_| Unavailable)?;
        // Second pass: a revoke that committed during the first pass, failed to
        // cache, and dropped "ready" before we set it again is picked up here.
        self.cache_revoked(now).await
    }

    async fn cache_revoked(&self, now: i64) -> Result<(), Unavailable> {
        let revoked: Vec<(String, i64)> = sqlx::query_as(
            "SELECT id::text, extract(epoch FROM expires_at)::bigint FROM sessions
             WHERE revoked_at IS NOT NULL AND expires_at > to_timestamp($1)",
        )
        .bind(now)
        .fetch_all(&self.db)
        .await
        .map_err(|_| Unavailable)?;
        // ponytail: one SET per revoked session; pipeline it if revocations reach the thousands.
        for (sid, exp) in revoked {
            cache(&self.redis, &sid, exp - now).await.map_err(|_| Unavailable)?;
        }
        Ok(())
    }

    /// Redis first; Postgres if Redis errors or lost its data. A session we have
    /// no row for counts as revoked.
    pub async fn is_revoked(&self, sid: &str, now: i64) -> Result<bool, Unavailable> {
        match self.redis.mget::<Vec<Option<String>>, _>(vec![key(sid), READY.into()]).await {
            Ok(v) if v.get(1).is_some_and(Option::is_some) => return Ok(v[0].is_some()),
            Ok(_) => {
                if let Ok(_one) = self.reloading.try_lock() {
                    let _ = self.reload_locked(now).await;
                }
            }
            Err(_) => {}
        }
        sqlx::query_scalar::<_, bool>("SELECT revoked_at IS NULL FROM sessions WHERE id = $1::uuid")
            .bind(sid)
            .fetch_optional(&self.db)
            .await
            .map(|alive| alive != Some(true))
            .map_err(|_| Unavailable)
    }

    /// Sliding renewal: with under 7 days left, push the session out to 14 days
    /// from now and return new claims to sign, with roles re-read so role changes
    /// reach long-lived sessions. `None` if it isn't due, was revoked, or is a
    /// judge session (those end with judging).
    pub async fn renew(&self, claims: &Claims, now: i64) -> Result<Option<Claims>, sqlx::Error> {
        if claims.exp - now >= RENEW_SECS || claims.room_id.is_some() {
            return Ok(None);
        }
        let exp = now + SESSION_SECS;
        // GREATEST: two renewals racing must never move the end backwards.
        let renewed = sqlx::query("UPDATE sessions SET expires_at = GREATEST(expires_at, to_timestamp($2)) WHERE id = $1::uuid AND revoked_at IS NULL")
            .bind(&claims.sid)
            .bind(exp)
            .execute(&self.db)
            .await?
            .rows_affected();
        if renewed != 1 {
            return Ok(None);
        }
        Ok(Some(Claims { roles: self.roles(&claims.sub).await?, iat: now, exp, ..claims.clone() }))
    }
}
