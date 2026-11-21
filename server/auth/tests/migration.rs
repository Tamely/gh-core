//! The first migration loads and its rules hold.
//!
//! Needs Postgres:
//!   docker compose -f server/docker-compose.yml up -d
//!   DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/postgres cargo test -p auth

use sqlx::{Connection, PgConnection};

/// Postgres error code of a failed statement: 23514 = CHECK failed, 23505 = duplicate.
async fn code(db: &mut PgConnection, sql: &str, arg: &str) -> Option<String> {
    let err = sqlx::query(sql).bind(arg).execute(db).await.err()?;
    err.as_database_error().and_then(|e| e.code()).map(|c| c.into_owned())
}

#[tokio::test]
async fn users_and_auth_migration() {
    let url = std::env::var("DATABASE_URL").expect("set DATABASE_URL (see the top of this file)");
    let mut db = PgConnection::connect(&url).await.unwrap();

    // Own schema so the test never touches real tables.
    sqlx::raw_sql(
        "DROP SCHEMA IF EXISTS auth_migration_test CASCADE;
         CREATE SCHEMA auth_migration_test;
         SET search_path TO auth_migration_test;",
    )
    .execute(&mut db)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../../migrations/0001_users_and_auth.sql"))
        .execute(&mut db)
        .await
        .unwrap();

    // users
    let by_email = "INSERT INTO users (email) VALUES ($1)";
    let by_phone = "INSERT INTO users (phone_e164) VALUES ($1)";
    assert_eq!(code(&mut db, by_email, "ricky@example.com").await, None);
    assert_eq!(code(&mut db, by_email, "ricky@example.com").await.as_deref(), Some("23505"));
    assert_eq!(code(&mut db, by_email, "Ricky@Example.com").await.as_deref(), Some("23514"));
    assert_eq!(code(&mut db, by_phone, "+12255550100").await, None);
    assert_eq!(code(&mut db, by_phone, "225-555-0100").await.as_deref(), Some("23514"));
    let neither = sqlx::query("INSERT INTO users DEFAULT VALUES").execute(&mut db).await.unwrap_err();
    assert_eq!(neither.as_database_error().unwrap().code().as_deref(), Some("23514"));

    let user: String = sqlx::query_scalar("SELECT id::text FROM users WHERE email = 'ricky@example.com'")
        .fetch_one(&mut db)
        .await
        .unwrap();

    // user_roles
    let role = |r: &str| format!("INSERT INTO user_roles VALUES ($1::uuid, '{r}')");
    assert_eq!(code(&mut db, &role("HACKER"), &user).await, None);
    assert_eq!(code(&mut db, &role("ADMIN"), &user).await, None);
    assert_eq!(code(&mut db, &role("HACKER"), &user).await.as_deref(), Some("23505"));
    assert_eq!(code(&mut db, &role("SUPERUSER"), &user).await.as_deref(), Some("23514"));
    // JUDGE_ROOM only exists in judge sessions, never as an account role.
    assert_eq!(code(&mut db, &role("JUDGE_ROOM"), &user).await.as_deref(), Some("23514"));

    // sessions
    let session = |expires: &str| {
        format!("INSERT INTO sessions (user_id, expires_at) VALUES ($1::uuid, now() + interval '{expires}')")
    };
    assert_eq!(code(&mut db, &session("14 days"), &user).await, None);
    assert_eq!(code(&mut db, &session("-1 day"), &user).await.as_deref(), Some("23514"));

    // Deleting a user removes their roles and sessions.
    sqlx::query("DELETE FROM users WHERE id = $1::uuid").bind(&user).execute(&mut db).await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM user_roles) + (SELECT count(*) FROM sessions)")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(left, 0);

    sqlx::raw_sql("DROP SCHEMA auth_migration_test CASCADE").execute(&mut db).await.unwrap();
}
