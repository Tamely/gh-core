//! Postgres behaviour: migrations, timeouts, pool exhaustion and readiness.

mod common;

use std::time::{Duration, Instant};

use axum::{
    Router,
    extract::State,
    http::{StatusCode, header::RETRY_AFTER},
    routing::get,
};
use common::*;
use gh_server::{AppState, app, db::DbError, routes, with_middleware};

#[tokio::test]
async fn migrations_apply_to_empty_database_and_rerun_cleanly() {
    let db = start_postgres().await;
    gh_server::db::migrate(&db.url).await.unwrap();
    gh_server::db::migrate(&db.url).await.unwrap(); // a restart runs them again

    let pool = sqlx::PgPool::connect(&db.url).await.unwrap();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations'
         ORDER BY table_name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        [
            "entitlement_scans",
            "hardware_loans",
            "judging_queue",
            "mentor_tickets",
            "notification_dispatches",
            "presentation_rooms",
            "teams",
            "users",
        ]
    );

    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(applied, 1);

    let vector: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_extension WHERE extname = 'vector'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(vector, 1);
}

#[tokio::test]
async fn two_instances_migrating_at_once_both_succeed() {
    let db = start_postgres().await;
    let (a, b) = tokio::join!(
        gh_server::db::migrate(&db.url),
        gh_server::db::migrate(&db.url)
    );
    a.unwrap();
    b.unwrap();
}

#[tokio::test]
async fn face_embedding_accepts_512_dimensions_only() {
    let db = start_postgres().await;
    let pool = schema_pool(&db).await;
    let insert =
        "INSERT INTO users (email, phone_e164, full_name, school, qr_hmac_sig, face_embedding)
                  VALUES ($1, '+15555550100', 'Test', 'LSU', '\\x00', $2::vector)";
    let ok = format!("[{}]", vec!["0.1"; 512].join(","));
    let bad = format!("[{}]", vec!["0.1"; 511].join(","));

    sqlx::query(insert)
        .bind("a@example.com")
        .bind(&ok)
        .execute(&pool)
        .await
        .unwrap();
    let err = sqlx::query(insert)
        .bind("b@example.com")
        .bind(&bad)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("512"), "{err}");
}

#[tokio::test]
async fn queue_positions_can_be_swapped_but_not_duplicated() {
    let db = start_postgres().await;
    let pool = schema_pool(&db).await;
    sqlx::raw_sql(
        "INSERT INTO presentation_rooms VALUES ('PFT-1200', 'GENERAL', 1, 0, 0, 'token');
         INSERT INTO teams (id, name) VALUES
           ('00000000-0000-0000-0000-00000000000a', 'A'),
           ('00000000-0000-0000-0000-00000000000b', 'B');
         INSERT INTO judging_queue (room_id, team_id, track, seq_index) VALUES
           ('PFT-1200', '00000000-0000-0000-0000-00000000000a', 'GENERAL', 1),
           ('PFT-1200', '00000000-0000-0000-0000-00000000000b', 'GENERAL', 2);",
    )
    .execute(&pool)
    .await
    .unwrap();

    // "Swap with On-Deck Team" in one statement.
    sqlx::query(
        "UPDATE judging_queue SET seq_index = 3 - seq_index
         WHERE room_id = 'PFT-1200' AND seq_index IN (1, 2)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let first: String = sqlx::query_scalar(
        "SELECT t.name FROM judging_queue q JOIN teams t ON t.id = q.team_id
         WHERE q.room_id = 'PFT-1200' AND q.seq_index = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(first, "B");

    // The constraint still holds at commit.
    let err = sqlx::query("UPDATE judging_queue SET seq_index = 1 WHERE room_id = 'PFT-1200'")
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("judging_queue_room_id_seq_index_key"),
        "{err}"
    );
}

#[tokio::test]
async fn slow_query_is_cancelled_at_two_seconds_and_connection_is_reused() {
    let db = start_postgres().await;
    let pool = migrated_pool(&db, 1).await;

    let started = Instant::now();
    let err = sqlx::query("SELECT pg_sleep(3)")
        .execute(&pool)
        .await
        .unwrap_err();
    let took = started.elapsed();
    let code = err.as_database_error().and_then(|e| e.code()).unwrap();
    assert_eq!(code, "57014", "{err}");
    // Postgres times the 2s on its own clock. Docker Desktop's VM clock can
    // drift a few percent from the host's (1.89s seen), so the window is
    // loose: the point is "cut off near 2s", not at the query's full 3s.
    assert!(
        took >= Duration::from_millis(1500) && took < Duration::from_millis(2900),
        "{took:?}"
    );

    // Only one connection exists, so this proves it went back into the pool usable.
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(one, 1);
    assert_eq!(pool.size(), 1);
}

#[tokio::test]
async fn idle_transaction_is_killed_after_five_seconds_and_pool_recovers() {
    let db = start_postgres().await;
    let pool = migrated_pool(&db, 1).await;

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT 1").execute(&mut *tx).await.unwrap();
    // Postgres times the 5s on Docker's VM clock, which drifts several
    // percent from the host's (6.7% seen), so wait well past it.
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert!(sqlx::query("SELECT 1").execute(&mut *tx).await.is_err());
    drop(tx);

    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(one, 1);
}

async fn slow_query(State(state): State<AppState>) -> Result<StatusCode, DbError> {
    sqlx::query("SELECT pg_sleep(10)")
        .execute(&state.db)
        .await?;
    Ok(StatusCode::OK)
}

async fn fast_query(State(state): State<AppState>) -> Result<StatusCode, DbError> {
    sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(StatusCode::OK)
}

fn test_app(state: AppState) -> Router {
    with_middleware(
        routes()
            .route("/test/slow", get(slow_query))
            .route("/test/fast", get(fast_query))
            .with_state(state),
    )
}

#[tokio::test]
async fn exhausted_pool_answers_503_stays_ready_and_recovers() {
    const MAX: u32 = 4;
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let app = test_app(state(migrated_pool(&db, MAX).await, &redis.url).await);

    let started = Instant::now();
    let requests: Vec<_> = (0..MAX * 5)
        .map(|_| {
            let app = app.clone();
            tokio::spawn(async move {
                let res = send(&app, "/test/slow").await;
                (res, started.elapsed())
            })
        })
        .collect();

    // While the pool is saturated the instance is busy, not broken: it must
    // stay ready so the load balancer doesn't pull every instance at once.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let ready_at = Instant::now();
    assert_eq!(send(&app, "/readyz").await.status(), StatusCode::OK);
    assert!(ready_at.elapsed() < Duration::from_millis(500));

    // Most requests give up waiting for a connection at 2s. A request that
    // gets a connection right at 2s can still run until the 3s request
    // timeout, so 3s is the worst case.
    let mut times = Vec::new();
    for request in requests {
        let (res, took) = request.await.unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(res.headers()[RETRY_AFTER], "1");
        assert!(took < Duration::from_millis(3300), "took {took:?}");
        times.push(took);
    }
    times.sort();
    let median = times[times.len() / 2];
    assert!(median < Duration::from_millis(2500), "median {median:?}");

    // Load is gone. Queries cut off by the 3s request timeout keep their
    // connection until Postgres's 2s statement_timeout stops them, so the
    // pool can need up to 2s more before a connection is free again.
    let started = Instant::now();
    assert_eq!(send(&app, "/test/fast").await.status(), StatusCode::OK);
    assert_eq!(send(&app, "/readyz").await.status(), StatusCode::OK);
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn readyz_follows_postgres_while_healthz_stays_up() {
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let app = app(state(migrated_pool(&db, 4).await, &redis.url).await);
    assert_eq!(send(&app, "/readyz").await.status(), StatusCode::OK);

    db.container.pause().await.unwrap();
    let started = Instant::now();
    let res = send(&app, "/readyz").await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(started.elapsed() < Duration::from_millis(900));
    assert_eq!(send(&app, "/healthz").await.status(), StatusCode::OK);

    db.container.unpause().await.unwrap();
    let recovered = eventually(Duration::from_secs(10), || async {
        send(&app, "/readyz").await.status() == StatusCode::OK
    })
    .await;
    assert!(recovered, "/readyz never returned to 200");
}

#[tokio::test]
async fn pool_replaces_connections_that_postgres_drops() {
    // What a Postgres restart or failover looks like to the pool: every
    // server-side connection disappears at once.
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let app = test_app(state(migrated_pool(&db, 4).await, &redis.url).await);
    assert_eq!(send(&app, "/test/fast").await.status(), StatusCode::OK);

    let admin = sqlx::PgPool::connect(&db.url).await.unwrap();
    let killed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (
           SELECT pg_terminate_backend(pid) FROM pg_stat_activity
           WHERE datname = current_database() AND pid <> pg_backend_pid()
         ) t",
    )
    .fetch_one(&admin)
    .await
    .unwrap();
    assert!(killed >= 1);

    let recovered = eventually(Duration::from_secs(5), || async {
        send(&app, "/test/fast").await.status() == StatusCode::OK
    })
    .await;
    assert!(recovered, "pool never replaced the dropped connections");
    assert_eq!(send(&app, "/readyz").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn unexpected_database_error_is_500() {
    async fn bad_sql(State(state): State<AppState>) -> Result<StatusCode, DbError> {
        sqlx::query("SELECT * FROM no_such_table")
            .execute(&state.db)
            .await?;
        Ok(StatusCode::OK)
    }
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let app = with_middleware(
        routes()
            .route("/test/bad", get(bad_sql))
            .with_state(state(migrated_pool(&db, 2).await, &redis.url).await),
    );
    let res = send(&app, "/test/bad").await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!res.headers().contains_key(RETRY_AFTER));
}
