//! Sessions: the `sessions` row behind every token, revocation and sliding renewal.
//!
//! Postgres is the truth. Redis caches revoked session IDs (`gh:revoked:{sid}`,
//! expiring when the session would have) so a revoke takes effect on the very
//! next request without a database query on every request.
//!
//! Redis keeps nothing on disk, so after it restarts the cache is empty. The
//! `gh:revoked:ready` key says "the cache is loaded": while it's missing, lookups
//! go to Postgres and the cache is reloaded.

use fred::prelude::{Client, KeysInterface};
use sqlx::PgPool;

use crate::{Claims, Role, SESSION_SECS};

/// Renew when less than this is left: 7 days.
pub const RENEW_SECS: i64 = 7 * 24 * 60 * 60;

const READY: &str = "gh:revoked:ready";

fn key(sid: &str) -> String {
    format!("gh:revoked:{sid}")
}

/// Neither Redis nor Postgres answered, so we can't tell if a session was
/// revoked. Callers return 503; never treat it as "not revoked".
#[derive(Debug, PartialEq, Eq)]
pub struct Unavailable;

pub struct Sessions {
    pub db: PgPool,
    /// Kenny's shared fred client (1s command timeout).
    pub redis: Client,
}

impl Sessions {
    /// Opens a session for a user and returns the claims to sign.
    pub async fn start(&self, user_id: &str, now: i64) -> Result<Claims, sqlx::Error> {
        let roles: Vec<String> =
            sqlx::query_scalar("SELECT role FROM user_roles WHERE user_id = $1::uuid ORDER BY role")
                .bind(user_id)
                .fetch_all(&self.db)
                .await?;
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
        Ok(Claims {
            sub: user_id.into(),
            // The table's CHECK only allows real roles, so nothing is dropped here.
            roles: roles.into_iter().filter_map(|r| serde_json::from_value::<Role>(r.into()).ok()).collect(),
            sid,
            iat: now,
            exp,
            room_id: None,
        })
    }

    /// Logout: revokes one session.
    pub async fn revoke(&self, sid: &str, now: i64) -> Result<(), Unavailable> {
        self.revoke_where("id = $1::uuid", sid, now).await
    }

    /// Admin cut-off: revokes every session a user has.
    pub async fn revoke_user(&self, user_id: &str, now: i64) -> Result<(), Unavailable> {
        self.revoke_where("user_id = $1::uuid", user_id, now).await
    }

    /// Safe to retry: an already-revoked session keeps its time and is cached again.
    /// `Unavailable` if Postgres failed, or the cache couldn't be updated or
    /// marked stale (a revoked session could still pass): the caller says retry.
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
            if self.cache(&sid, exp - now).await.is_err() {
                // The database says revoked but the cache doesn't. Drop "ready" so
                // lookups go to Postgres until the next reload.
                self.redis.del::<(), _>(READY).await.map_err(|_| Unavailable)?;
            }
        }
        Ok(())
    }

    async fn cache(&self, sid: &str, ttl: i64) -> Result<(), fred::error::Error> {
        self.redis.set(key(sid), 1, Some(fred::types::Expiration::EX(ttl.max(1))), None, false).await
    }

    /// Fills the cache from Postgres. Runs on startup and whenever Redis lost it.
    pub async fn reload(&self, now: i64) -> Result<(), Unavailable> {
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
            self.cache(&sid, exp - now).await.map_err(|_| Unavailable)?;
        }
        self.redis.set::<(), _, _>(READY, 1, None, None, false).await.map_err(|_| Unavailable)
    }

    /// Redis first; Postgres if Redis errors or lost its data. A session we have
    /// no row for counts as revoked.
    pub async fn is_revoked(&self, sid: &str, now: i64) -> Result<bool, Unavailable> {
        match self.redis.mget::<Vec<Option<String>>, _>(vec![key(sid), READY.into()]).await {
            Ok(v) if v.get(1).is_some_and(Option::is_some) => return Ok(v[0].is_some()),
            // ponytail: every request reloads until one finishes; fine for a few hundred revocations.
            Ok(_) => _ = self.reload(now).await,
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
    /// from now and return new claims to sign. `None` if it isn't due, was
    /// revoked, or is a judge session (those end with judging).
    pub async fn renew(&self, claims: &Claims, now: i64) -> Result<Option<Claims>, sqlx::Error> {
        if claims.exp - now >= RENEW_SECS || claims.room_id.is_some() {
            return Ok(None);
        }
        let exp = now + SESSION_SECS;
        let renewed = sqlx::query("UPDATE sessions SET expires_at = to_timestamp($2) WHERE id = $1::uuid AND revoked_at IS NULL")
            .bind(&claims.sid)
            .bind(exp)
            .execute(&self.db)
            .await?
            .rows_affected();
        Ok((renewed == 1).then(|| Claims { iat: now, exp, ..claims.clone() }))
    }
}
