//! WebSocket load test for gh-server.
//!
//! Opens many WebSocket clients spread over rooms, teams, mentor tags and
//! users, publishes events through Redis at a fixed rate, and checks that
//! every client got every event in order, quickly, without being dropped.
//! Run it on the same machine as the server so both share one clock.
//!
//! ```sh
//! gh-bench --url ws://127.0.0.1:8080/api/v1/ws --redis redis://127.0.0.1:6379 \
//!          --clients 1000 --rate 100 --duration 600
//! ```
//! `--url` can be repeated; clients are spread over the URLs round-robin.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use gh_server::{events, events::now_us, topic::Topic};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

struct Args {
    urls: Vec<String>,
    redis: String,
    clients: usize,
    rooms: usize,
    teams: usize,
    tags: usize,
    rate: u64,
    duration: Duration,
    p99_limit: Duration,
}

fn parse_args() -> Args {
    let mut args = Args {
        urls: Vec::new(),
        redis: "redis://127.0.0.1:6379".into(),
        clients: 1000,
        rooms: 25,
        teams: 250,
        tags: 10,
        rate: 100,
        duration: Duration::from_secs(600),
        p99_limit: Duration::from_millis(5),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let value = it
            .next()
            .unwrap_or_else(|| usage(&format!("{flag} needs a value")));
        let number = || {
            value
                .parse::<u64>()
                .unwrap_or_else(|_| usage(&format!("bad {flag}")))
        };
        match flag.as_str() {
            "--url" => args.urls.push(value.clone()),
            "--redis" => args.redis = value.clone(),
            "--clients" => args.clients = number() as usize,
            "--rooms" => args.rooms = number() as usize,
            "--teams" => args.teams = number() as usize,
            "--tags" => args.tags = number() as usize,
            "--rate" => args.rate = number(),
            "--duration" => args.duration = Duration::from_secs(number()),
            "--p99-ms" => args.p99_limit = Duration::from_millis(number()),
            _ => usage(&format!("unknown flag {flag}")),
        }
    }
    if args.urls.is_empty() {
        args.urls.push("ws://127.0.0.1:8080/api/v1/ws".into());
    }
    if [args.clients, args.rooms, args.teams, args.tags].contains(&0) || args.rate == 0 {
        usage("counts and --rate must be above 0");
    }
    args
}

fn usage(problem: &str) -> ! {
    eprintln!(
        "{problem}\nusage: gh-bench [--url ws://...]... [--redis redis://...] [--clients N] \
         [--rooms N] [--teams N] [--tags N] [--rate EVENTS_PER_SEC] [--duration SECS] [--p99-ms MS]"
    );
    std::process::exit(2);
}

/// The topics client `i` subscribes to.
fn client_topics(i: usize, a: &Args) -> [String; 4] {
    [
        format!("room:r{}", i % a.rooms),
        format!("team:t{}", i % a.teams),
        format!("mentor-tag:tag{}", i % a.tags),
        format!("user:u{i}"),
    ]
}

/// The topic of the `k`th published event, cycling through every kind.
fn event_topic(k: u64, a: &Args) -> String {
    let n = (k / 4) as usize;
    match k % 4 {
        0 => format!("room:r{}", n % a.rooms),
        1 => format!("team:t{}", n % a.teams),
        2 => format!("mentor-tag:tag{}", n % a.tags),
        _ => format!("user:u{}", n % a.clients),
    }
}

/// How many clients subscribe to the `k`th event's topic.
fn subscribers(k: u64, a: &Args) -> u64 {
    let n = (k / 4) as usize;
    let share =
        |groups: usize, g: usize| (a.clients / groups + usize::from(g < a.clients % groups)) as u64;
    match k % 4 {
        0 => share(a.rooms, n % a.rooms),
        1 => share(a.teams, n % a.teams),
        2 => share(a.tags, n % a.tags),
        _ => 1,
    }
}

#[derive(Default)]
struct Counters {
    subscribed: AtomicU64,
    connected: AtomicU64,
    received: AtomicU64,
    gaps: AtomicU64,
    resyncs: AtomicU64,
    errors: AtomicU64,
    dropped: AtomicU64,
}

/// What one client saw: (topic, seq, microseconds from publish to arrival).
type Samples = Vec<(Arc<str>, u64, u64)>;

async fn client(
    url: String,
    topics: [String; 4],
    counters: Arc<Counters>,
    stop: CancellationToken,
) -> Samples {
    let mut samples = Samples::new();
    let mut ws = match tokio_tungstenite::connect_async_with_config(&url, None, true).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            eprintln!("connect failed: {e}");
            counters.errors.fetch_add(1, Relaxed);
            return samples;
        }
    };
    counters.connected.fetch_add(1, Relaxed);
    for topic in &topics {
        let subscribe = json!({ "op": "subscribe", "topic": topic }).to_string();
        if ws.send(Message::text(subscribe)).await.is_err() {
            counters.errors.fetch_add(1, Relaxed);
            return samples;
        }
    }

    let mut last_seq: HashMap<Arc<str>, u64> = HashMap::new();
    loop {
        let message = tokio::select! {
            () = stop.cancelled() => {
                let _ = ws.close(None).await;
                return samples;
            }
            message = ws.next() => message,
        };
        let text = match message {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(frame))) => {
                eprintln!("server closed a client: {frame:?}");
                counters.dropped.fetch_add(1, Relaxed);
                return samples;
            }
            Some(Ok(_)) => continue, // pings are answered automatically
            Some(Err(e)) => {
                eprintln!("client read error: {e}");
                counters.dropped.fetch_add(1, Relaxed);
                return samples;
            }
            None => {
                counters.dropped.fetch_add(1, Relaxed);
                return samples;
            }
        };
        let arrived = now_us();
        let Ok(m) = serde_json::from_str::<Value>(&text) else {
            counters.errors.fetch_add(1, Relaxed);
            continue;
        };
        match m["type"].as_str() {
            Some("subscribed") => {
                counters.subscribed.fetch_add(1, Relaxed);
            }
            Some("resync") => {
                counters.resyncs.fetch_add(1, Relaxed);
            }
            Some("error") => {
                eprintln!("server error: {m}");
                counters.errors.fetch_add(1, Relaxed);
            }
            _ => {
                let (Some(topic), Some(seq), Some(sent)) = (
                    m["topic"].as_str(),
                    m["seq"].as_u64(),
                    m["payload"]["t"].as_u64(),
                ) else {
                    counters.errors.fetch_add(1, Relaxed);
                    continue;
                };
                let topic: Arc<str> = last_seq
                    .keys()
                    .find(|k| &***k == topic)
                    .cloned()
                    .unwrap_or_else(|| topic.into());
                if let Some(prev) = last_seq.insert(topic.clone(), seq)
                    && seq != prev + 1
                {
                    counters.gaps.fetch_add(1, Relaxed);
                }
                counters.received.fetch_add(1, Relaxed);
                samples.push((topic, seq, arrived.saturating_sub(sent)));
            }
        }
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let counters = Arc::new(Counters::default());
    let stop = CancellationToken::new();

    println!(
        "connecting {} clients to {} server(s): {} rooms, {} teams, {} mentor tags, 1 user topic each",
        args.clients,
        args.urls.len(),
        args.rooms,
        args.teams,
        args.tags
    );
    let mut clients = Vec::with_capacity(args.clients);
    for i in 0..args.clients {
        let url = args.urls[i % args.urls.len()].clone();
        clients.push(tokio::spawn(client(
            url,
            client_topics(i, &args),
            counters.clone(),
            stop.clone(),
        )));
        // Ramp up instead of opening every socket in the same millisecond.
        if i % 50 == 49 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    let want = (args.clients * 4) as u64;
    let deadline = Instant::now() + Duration::from_secs(60);
    while counters.subscribed.load(Relaxed) < want {
        if Instant::now() > deadline || counters.errors.load(Relaxed) > 0 {
            eprintln!(
                "only {}/{want} subscriptions confirmed, {} errors",
                counters.subscribed.load(Relaxed),
                counters.errors.load(Relaxed)
            );
            std::process::exit(1);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!(
        "all {} clients connected and subscribed ({want} subscriptions)",
        args.clients
    );

    let events = events::publisher(&args.redis)
        .await
        .expect("connect to Redis");
    let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / args.rate as f64));
    let started = Instant::now();
    let mut published = 0u64;
    let mut expected = 0u64;
    let mut last_report = Instant::now();
    while started.elapsed() < args.duration {
        tick.tick().await;
        let topic = Topic::parse(&event_topic(published, &args)).unwrap();
        events
            .publish(&topic, "bench", &json!({ "t": now_us() }))
            .await
            .expect("publish");
        expected += subscribers(published, &args);
        published += 1;
        if last_report.elapsed() >= Duration::from_secs(10) {
            last_report = Instant::now();
            println!(
                "{:>4}s  connected {}  published {published}  delivered {}/{expected}  dropped {}  gaps {}  resyncs {}",
                started.elapsed().as_secs(),
                counters.connected.load(Relaxed) - counters.dropped.load(Relaxed),
                counters.received.load(Relaxed),
                counters.dropped.load(Relaxed),
                counters.gaps.load(Relaxed),
                counters.resyncs.load(Relaxed),
            );
        }
    }

    // Let the last events arrive, then close every client.
    let settle = Instant::now() + Duration::from_secs(5);
    while counters.received.load(Relaxed) < expected && Instant::now() < settle {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stop.cancel();
    let mut per_delivery = Vec::new();
    let mut per_event: HashMap<(Arc<str>, u64), u64> = HashMap::new();
    for client in clients {
        for (topic, seq, latency) in client.await.unwrap() {
            per_delivery.push(latency);
            let worst = per_event.entry((topic, seq)).or_default();
            *worst = (*worst).max(latency);
        }
    }
    per_delivery.sort_unstable();
    let mut per_event: Vec<u64> = per_event.into_values().collect();
    per_event.sort_unstable();

    let received = counters.received.load(Relaxed);
    let dropped = counters.dropped.load(Relaxed);
    let gaps = counters.gaps.load(Relaxed);
    let errors = counters.errors.load(Relaxed);
    let p99_all = percentile(&per_event, 0.99);

    println!("\n== results ==");
    println!(
        "clients: {} held for {:.0}s, {dropped} dropped, {errors} errors",
        args.clients,
        started.elapsed().as_secs_f64()
    );
    println!(
        "events: {published} published at {}/s, {received}/{expected} deliveries, {gaps} gaps, {} resyncs",
        args.rate,
        counters.resyncs.load(Relaxed)
    );
    for (label, sorted) in [
        ("publish -> one client", &per_delivery),
        ("publish -> every subscriber", &per_event),
    ] {
        println!(
            "latency {label:<28} p50 {:>6}µs  p99 {:>6}µs  max {:>6}µs",
            percentile(sorted, 0.50),
            percentile(sorted, 0.99),
            sorted.last().copied().unwrap_or(0),
        );
    }

    let mut failures = Vec::new();
    if dropped > 0 || errors > 0 {
        failures.push(format!("{dropped} dropped, {errors} errors"));
    }
    if received != expected {
        failures.push(format!("{received}/{expected} deliveries"));
    }
    if gaps > 0 {
        failures.push(format!("{gaps} seq gaps"));
    }
    if p99_all > args.p99_limit.as_micros() as u64 {
        failures.push(format!(
            "p99 {p99_all}µs over {}ms",
            args.p99_limit.as_millis()
        ));
    }
    if failures.is_empty() {
        println!("PASS");
    } else {
        println!("FAIL: {}", failures.join("; "));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(clients: usize) -> Args {
        Args {
            urls: vec![],
            redis: String::new(),
            clients,
            rooms: 25,
            teams: 250,
            tags: 10,
            rate: 100,
            duration: Duration::ZERO,
            p99_limit: Duration::ZERO,
        }
    }

    /// The expected-delivery count must match who actually subscribes.
    #[test]
    fn subscriber_counts_match_client_topics() {
        for clients in [1000, 7, 333] {
            let a = args(clients);
            let topics: Vec<[String; 4]> = (0..clients).map(|i| client_topics(i, &a)).collect();
            for k in 0..2000 {
                let topic = event_topic(k, &a);
                let actual = topics.iter().filter(|t| t.contains(&topic)).count() as u64;
                assert_eq!(
                    subscribers(k, &a),
                    actual,
                    "event {k} on {topic} with {clients} clients"
                );
            }
        }
    }

    #[test]
    fn every_generated_topic_is_valid() {
        let a = args(1000);
        for i in 0..1000 {
            for topic in client_topics(i, &a) {
                Topic::parse(&topic).unwrap();
            }
        }
    }

    #[test]
    fn percentile_picks_nearest_rank() {
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&v, 0.5), 51);
        assert_eq!(percentile(&v, 0.99), 99);
        assert_eq!(percentile(&[], 0.99), 0);
    }
}
