//! Runs the real `gh-server` binary: startup checks, failures, and ports.

mod common;

use std::{
    net::TcpListener as StdListener,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use common::*;

const BIN: &str = env!("CARGO_BIN_EXE_gh-server");

/// A command with none of our settings inherited from the test environment.
fn server() -> Command {
    let mut cmd = Command::new(BIN);
    for name in [
        "DATABASE_URL",
        "REDIS_URL",
        "HTTP_PORT",
        "METRICS_PORT",
        "DB_MAX_CONNECTIONS",
        "DB_MIN_CONNECTIONS",
        "SHUTDOWN_DELAY_MS",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Runs the binary until it exits on its own, failing if that takes longer
/// than `limit`.
fn run_until_exit(mut cmd: Command, limit: Duration) -> (Output, Duration) {
    let started = Instant::now();
    let mut child = cmd.spawn().unwrap();
    loop {
        if child.try_wait().unwrap().is_some() {
            return (child.wait_with_output().unwrap(), started.elapsed());
        }
        if started.elapsed() > limit {
            child.kill().unwrap();
            panic!("gh-server still running after {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn logs(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Kills and reaps the server when the test ends, even if an assertion fails,
/// so no stray process keeps its ports.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    StdListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn missing_settings_exit_1_listing_every_problem() {
    let (output, took) = run_until_exit(server(), Duration::from_secs(10));
    assert_eq!(output.status.code(), Some(1));
    let logs = logs(&output);
    assert!(logs.contains("DATABASE_URL is required"), "{logs}");
    assert!(logs.contains("REDIS_URL is required"), "{logs}");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn every_log_line_is_json() {
    let (output, _) = run_until_exit(server(), Duration::from_secs(10));
    let logs = logs(&output);
    assert!(!logs.trim().is_empty());
    for line in logs.lines() {
        let value: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"));
        assert!(
            value["timestamp"].is_string() && value["level"].is_string(),
            "{line}"
        );
    }
}

#[test]
fn wrong_url_schemes_and_ports_are_rejected_together() {
    let mut cmd = server();
    cmd.env("DATABASE_URL", "mysql://localhost/gh")
        .env("REDIS_URL", "http://localhost:6379")
        .env("HTTP_PORT", "0");
    let (output, _) = run_until_exit(cmd, Duration::from_secs(10));
    assert_eq!(output.status.code(), Some(1));
    let logs = logs(&output);
    for expected in ["DATABASE_URL", "REDIS_URL", "HTTP_PORT"] {
        assert!(logs.contains(expected), "no error about {expected}: {logs}");
    }
}

#[test]
fn postgres_refusing_connections_exits_quickly() {
    let mut cmd = server();
    cmd.env("DATABASE_URL", "postgres://nobody@127.0.0.1:1/none")
        .env("REDIS_URL", "redis://127.0.0.1:1");
    let (output, took) = run_until_exit(cmd, Duration::from_secs(15));
    assert_eq!(output.status.code(), Some(1));
    assert!(
        logs(&output).contains("migrations failed"),
        "{}",
        logs(&output)
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn postgres_that_never_answers_times_out_instead_of_hanging() {
    // 10.255.255.1 is unroutable: packets are dropped, so a connect attempt
    // waits for the OS timeout (a minute or more) unless we cut it off.
    let mut cmd = server();
    cmd.env("DATABASE_URL", "postgres://nobody@10.255.255.1:5432/none")
        .env("REDIS_URL", "redis://127.0.0.1:1");
    let (output, took) = run_until_exit(cmd, Duration::from_secs(30));
    assert_eq!(output.status.code(), Some(1));
    assert!(took < Duration::from_secs(15), "{took:?}");
}

#[tokio::test]
async fn unreachable_redis_exits_instead_of_waiting_forever() {
    let db = start_postgres().await;
    let mut cmd = server();
    cmd.env("DATABASE_URL", &db.url)
        .env("REDIS_URL", "redis://127.0.0.1:1");
    let (output, took) =
        tokio::task::spawn_blocking(move || run_until_exit(cmd, Duration::from_secs(30)))
            .await
            .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(logs(&output).contains("Redis"), "{}", logs(&output));
    assert!(took < Duration::from_secs(15), "{took:?}");
}

#[tokio::test]
async fn port_in_use_exits_1() {
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let taken = StdListener::bind("0.0.0.0:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let mut cmd = server();
    cmd.env("DATABASE_URL", &db.url)
        .env("REDIS_URL", &redis.url)
        .env("HTTP_PORT", port.to_string())
        .env("METRICS_PORT", free_port().to_string());
    let (output, _) =
        tokio::task::spawn_blocking(move || run_until_exit(cmd, Duration::from_secs(30)))
            .await
            .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        logs(&output).contains(&format!("can't listen on port {port}")),
        "{}",
        logs(&output)
    );
    drop(taken);
}

#[tokio::test]
async fn starts_migrates_and_keeps_metrics_off_the_public_port() {
    let (db, redis) = tokio::join!(start_postgres(), start_redis());
    let (http, metrics) = (free_port(), free_port());
    let _server = Running(
        server()
            .env("DATABASE_URL", &db.url)
            .env("REDIS_URL", &redis.url)
            .env("HTTP_PORT", http.to_string())
            .env("METRICS_PORT", metrics.to_string())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );

    let client = reqwest::Client::new();
    let get = |port: u16, path: &str| {
        let request = client.get(format!("http://127.0.0.1:{port}{path}"));
        async move { request.send().await.ok().map(|r| r.status().as_u16()) }
    };
    let ready = eventually(Duration::from_secs(30), || async {
        get(http, "/readyz").await == Some(200)
    })
    .await;
    assert!(ready, "server never became ready");

    assert_eq!(get(http, "/healthz").await, Some(200));
    assert_eq!(get(http, "/metrics").await, Some(404));
    let page = client
        .get(format!("http://127.0.0.1:{metrics}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("redis_connected 1"), "{page}");
    assert!(page.contains("db_pool_connections"), "{page}");

    // Migrations ran.
    let pool = sqlx::PgPool::connect(&db.url).await.unwrap();
    let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(applied, 1);
}
