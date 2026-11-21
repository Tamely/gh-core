//! DEV ONLY. Never deployed: the server depends on the `auth` library, which
//! doesn't build this binary. Lets you try signed-in requests before sign-in exists.
//!
//!   auth-dev mint ADMIN [email]   create the user if needed, give it the role, print a token
//!   auth-dev serve                auth's routes on http://127.0.0.1:8080; sign-in codes
//!                                 go out through Postmark / Twilio when their variables
//!                                 are set (server/.env.example), else print here
//!
//! Both use a fixed dev signing key, so tokens from `mint` only work on `serve`.
//! Needs DATABASE_URL and REDIS_URL (the dev databases from docker-compose.yml).
//! Only runs against a local database, and applies the migrations in
//! server/migrations first (sqlx's runner). AUTH_DEV_ADDR changes
//! the listen address (0.0.0.0:8080 when it runs inside Docker).

use std::{sync::Arc, time::Duration};

use auth::{layer, policies, routes, user_for, AuthState, Codes, Dest, JudgeKey, Keys, Role, Sessions};
use ed25519_dalek::SigningKey;
use fred::prelude::{Builder, ClientLike, Config};

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let usage = "usage: auth-dev mint ROLE [email] | auth-dev serve";
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if !matches!(args[..], ["mint", _] | ["mint", _, _] | ["serve"]) {
        return Err(usage.into());
    }
    if let ["mint", role, ..] = args[..] {
        match serde_json::from_value::<Role>(role.to_uppercase().into()) {
            Ok(Role::JudgeRoom) => return Err("JUDGE_ROOM sessions come from POST /api/v1/judge/exchange (see judge-admin)".into()),
            Ok(_) => {}
            Err(_) => return Err(format!("unknown role {role:?}; use HACKER, VOLUNTEER, MENTOR or ADMIN").into()),
        }
    }

    if !cfg!(debug_assertions) {
        return Err("auth-dev is a debug build only (cargo run, not --release)".into());
    }
    let env = |name: &str| std::env::var(name).map_err(|_| format!("set {name}"));
    let url = env("DATABASE_URL")?;
    // Refuse anything but the compose database (this machine, port 5432), so a
    // droplet URL left in the shell can't get dev admins written into it. Parsed
    // by sqlx itself, so `?host=` tricks can't slip past. A tunnel the droplet
    // is reachable through also listens on 127.0.0.1, so keep tunnels off 5432.
    let opts: sqlx::postgres::PgConnectOptions = url.parse()?;
    let (host, port) = (opts.get_host(), opts.get_port());
    if !["127.0.0.1", "localhost", "host.docker.internal", "::1", "[::1]"].contains(&host) || port != 5432 {
        return Err(format!("auth-dev only runs against the local compose database, not {host}:{port}").into());
    }
    let db = sqlx::PgPool::connect_with(opts).await?;
    // Same migration runner and bookkeeping the server will use.
    sqlx::migrate!("../migrations").run(&db).await?;
    let redis = Builder::from_config(Config::from_url(&env("REDIS_URL")?)?)
        .with_performance_config(|c| c.default_command_timeout = Duration::from_secs(1))
        .build()?;
    redis.init().await?;
    let state = Arc::new(AuthState {
        keys: Keys::new(SigningKey::from_bytes(&[7; 32]), vec![]),
        origin: "http://127.0.0.1:8080".into(),
        now,
        sessions: Sessions::new(db, redis),
        policies: policies(),
        codes: Codes::from_env()?,
        // Set JUDGE_KEY to the same value for judge-admin to try placards here.
        judge: match std::env::var("JUDGE_KEY") {
            Ok(k) => JudgeKey::load(&k)?,
            Err(_) => JudgeKey::new([8; 32]),
        },
    });

    match args[..] {
        ["mint", role, ref rest @ ..] => {
            let role = role.to_uppercase();
            let email = rest.first().copied().unwrap_or("dev@example.com");
            let to = Dest::email(email).ok_or(format!("not an email: {email}"))?;
            let db = &state.sessions.db;
            let id = user_for(db, &to).await?;
            sqlx::query("INSERT INTO user_roles (user_id, role) VALUES ($1::uuid, $2) ON CONFLICT DO NOTHING")
                .bind(&id)
                .bind(&role)
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
            // With connect info, so rate limits see each caller's IP.
            axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
        }
        _ => unreachable!("checked above"),
    }
    Ok(())
}
