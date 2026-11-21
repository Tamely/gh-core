//! Admin tool for judge room placards. Runs against the real database.
//!
//!   judge-admin room ROOM_ID STARTS ENDS   create a room, or move its judging window
//!                                          (times as Postgres reads them, e.g. "2027-01-24 13:00-06")
//!   judge-admin print [DIR]                every room's placard URL; QR codes as DIR/ROOM_ID.svg (default ./placards)
//!   judge-admin rotate ROOM_ID             new placard; the old one and every session it gave out stop working
//!
//! Needs DATABASE_URL and JUDGE_KEY (K_judge, from the secrets store), plus
//! REDIS_URL for `rotate`. APP_URL defaults to https://app.geauxhack.com.
//! The server applies migrations; this tool doesn't.
//! The printed URLs are credentials: anyone with one can judge that room.

use std::time::Duration;

use auth::{rotate, set_room, JudgeKey, Sessions};
use fred::prelude::{Builder, ClientLike, Config};
use qrcode::{render::svg, QrCode};

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let usage = "usage: judge-admin room ROOM_ID STARTS ENDS | print [DIR] | rotate ROOM_ID";
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if !matches!(args[..], ["room", _, _, _] | ["print"] | ["print", _] | ["rotate", _]) {
        return Err(usage.into());
    }
    let env = |name: &str| std::env::var(name).map_err(|_| format!("set {name}"));
    let key = JudgeKey::load(&env("JUDGE_KEY")?)?;
    let app = std::env::var("APP_URL").unwrap_or("https://app.geauxhack.com".into());
    let url = |token: &str| format!("{}/judge/{token}", app.trim_end_matches('/'));
    let db = sqlx::PgPool::connect(&env("DATABASE_URL")?).await?;

    match args[..] {
        ["room", room, starts, ends] => {
            // Postgres parses the times, so any format it takes works here.
            let (starts, ends): (i64, i64) = sqlx::query_as(
                "SELECT floor(extract(epoch FROM $1::timestamptz))::bigint, floor(extract(epoch FROM $2::timestamptz))::bigint",
            )
            .bind(starts)
            .bind(ends)
            .fetch_one(&db)
            .await?;
            set_room(&db, &key, room, starts, ends).await?;
            println!("{room}: judging window set. Run `judge-admin print` for its placard.");
        }
        ["print", ref dir @ ..] => {
            let dir = dir.first().copied().unwrap_or("placards");
            std::fs::create_dir_all(dir)?;
            let rooms: Vec<(String, i32, Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT room_id, token_version, judging_starts_at::text, judging_ends_at::text
                 FROM presentation_rooms ORDER BY room_id",
            )
            .fetch_all(&db)
            .await?;
            for (room, version, starts, ends) in rooms {
                let link = url(&key.placard(&room, version as u32));
                let qr = QrCode::new(link.as_bytes())?.render::<svg::Color>().min_dimensions(600, 600).build();
                // Room ids are letters, digits, - and _ (the table checks), so safe as file names.
                std::fs::write(format!("{dir}/{room}.svg"), qr)?;
                let window = match (starts, ends) {
                    (Some(s), Some(e)) => format!("{s} to {e}"),
                    _ => "no judging window yet: the placard won't work".into(),
                };
                println!("{room}  v{version}  {window}\n  {link}");
            }
        }
        ["rotate", room] => {
            let redis = Builder::from_config(Config::from_url(&env("REDIS_URL")?)?)
                .with_performance_config(|c| c.default_command_timeout = Duration::from_secs(1))
                .build()?;
            redis.init().await?;
            match rotate(&Sessions::new(db, redis), &key, room, now()).await? {
                Some(token) => println!("{room}: new placard; the old one no longer works.\n  {}", url(&token)),
                None => return Err(format!("no room {room:?}").into()),
            }
        }
        _ => unreachable!("checked above"),
    }
    Ok(())
}
