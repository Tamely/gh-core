//! Postgres connection pool and migrations.

use std::{str::FromStr, time::Duration};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use metrics::gauge;
use sqlx::{
    Connection, PgConnection, PgPool, Postgres,
    pool::PoolConnection,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::config::Config;

/// How long a request waits for a free connection before getting a 503.
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn connect(config: &Config) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect_with(connect_options(&config.database_url)?)
        .await
}

/// Takes a connection from the pool. Handlers should acquire through this so
/// the `db_pool_waiting` metric counts them while they wait.
pub async fn acquire(pool: &PgPool) -> Result<PoolConnection<Postgres>, sqlx::Error> {
    struct Waiting;
    impl Drop for Waiting {
        fn drop(&mut self) {
            gauge!("db_pool_waiting").decrement(1.0);
        }
    }
    gauge!("db_pool_waiting").increment(1.0);
    let _waiting = Waiting; // decrements on return, error, or cancellation
    pool.acquire().await
}

/// Every pooled connection starts with these limits, so one slow query or a
/// forgotten open transaction can't hold a connection for long.
pub fn connect_options(url: &str) -> Result<PgConnectOptions, sqlx::Error> {
    Ok(PgConnectOptions::from_str(url)?.options([
        ("statement_timeout", "2000"),
        ("idle_in_transaction_session_timeout", "5000"),
    ]))
}

/// Runs migrations on a dedicated connection without the 2s statement limit,
/// since creating extensions or indexes can take longer than that. When two
/// instances start together, sqlx's advisory lock makes one wait for the other.
pub async fn migrate(url: &str) -> Result<(), sqlx::migrate::MigrateError> {
    let mut conn = PgConnection::connect(url).await?;
    sqlx::migrate!().run(&mut conn).await?;
    conn.close().await?;
    Ok(())
}

/// Turns database errors into responses. Overload (no free connection, or a
/// query cut off by `statement_timeout`) becomes 503 so clients retry;
/// anything else is a 500.
#[derive(Debug)]
pub struct DbError(pub sqlx::Error);

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        Self(e)
    }
}

/// Postgres error code for a query cancelled by `statement_timeout`.
const QUERY_CANCELED: &str = "57014";

impl IntoResponse for DbError {
    fn into_response(self) -> Response {
        match &self.0 {
            sqlx::Error::PoolTimedOut => {
                tracing::warn!("no free database connection");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
            sqlx::Error::Database(e) if e.code().as_deref() == Some(QUERY_CANCELED) => {
                tracing::warn!("query hit statement_timeout");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
            e => {
                tracing::error!(error = %e, "database error");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}
