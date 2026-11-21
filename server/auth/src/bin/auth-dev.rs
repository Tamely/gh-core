//! DEV ONLY. Never deployed: the server depends on the `auth` library, which
//! doesn't build this binary. Lets you try signed-in requests before sign-in exists.
//!
//!   auth-dev mint ADMIN [email]   create the user if needed, give it the role, print a token
//!   auth-dev serve                auth's routes on http://127.0.0.1:8080
//!
//! Both use a fixed dev signing key, so tokens from `mint` only work on `serve`.
//! Needs DATABASE_URL and REDIS_URL (the dev databases from docker-compose.yml).
//! Loads the auth migration into an empty dev database. AUTH_DEV_ADDR changes
//! the listen address (0.0.0.0:8080 when it runs inside Docker).

use std::{sync::Arc, time::Duration};

use auth::{layer, policies, routes, AuthState, Keys, Sessions};
use ed25519_dalek::SigningKey;
use fred::prelude::{Builder, ClientLike, Config};

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env = |name: &str| std::env::var(name).map_err(|_| format!("set {name}"));
    let db = sqlx::PgPool::connect(&env("DATABASE_URL")?).await?;
    let redis = Builder::from_config(Config::from_url(&env("REDIS_URL")?)?)
        .with_performance_config(|c| c.default_command_timeout = Duration::from_secs(1))
        .build()?;
    redis.init().await?;
    if sqlx::query_scalar::<_, Option<String>>("SELECT to_regclass('users')::text").fetch_one(&db).await?.is_none() {
        sqlx::raw_sql(include_str!("../../../migrations/0001_users_and_auth.sql")).execute(&db).await?;
    }
    let state = Arc::new(AuthState {
        keys: Keys::new(SigningKey::from_bytes(&[7; 32]), vec![]),
        origin: "http://127.0.0.1:8080".into(),
        now,
        sessions: Sessions { db, redis },
        policies: policies(),
    });

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["mint", role, ref rest @ ..] => {
            let email = rest.first().copied().unwrap_or("dev@example.com").to_lowercase();
            let db = &state.sessions.db;
            let id: String = sqlx::query_scalar(
                "INSERT INTO users (email) VALUES ($1)
                 ON CONFLICT (email) DO UPDATE SET email = EXCLUDED.email RETURNING id::text",
            )
            .bind(&email)
            .fetch_one(db)
            .await?;
            sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ($1::uuid, $2) ON CONFLICT DO NOTHING")
                .bind(&id)
                .bind(role.to_uppercase())
                .execute(db)
                .await?;
            let claims = state.sessions.start(&id, now()).await?;
            println!("{}", state.keys.issue(&claims));
        }
        ["serve"] => {
            state.sessions.reload(now()).await.map_err(|_| "can't load revocations")?;
            let app = routes(state.clone()).layer(axum::middleware::from_fn_with_state(state, layer));
            let addr = std::env::var("AUTH_DEV_ADDR").unwrap_or("127.0.0.1:8080".into());
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            eprintln!("auth-dev serving on http://{addr}");
            axum::serve(listener, app).await?;
        }
        _ => return Err("usage: auth-dev mint ROLE [email] | auth-dev serve".into()),
    }
    Ok(())
}
