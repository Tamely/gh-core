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
//!
//! `--reconnect` makes clients behave like the app during a deploy: when the
//! server closes them (or refuses them), they wait a random 0-2s and try the
//! next URL. Gaps in `seq` right after a reconnect or a `resync` are counted
//! as missed-while-away (the app reloads over REST then); any other gap fails
//! the run.
//!
//! `--slow N` adds N clients that subscribe to every room and mentor tag and
//! then never read. The normal clients must still pass, and the server must
//! cut the slow ones off. `--pad BYTES` makes events bigger so a stuck client
//! backs up faster, and `--metrics http://host:9090/metrics` reads the
//! server's counters at the end to show why the slow clients were cut off.
//!
//! `--prefix P` puts every topic id under `P` (e.g. `room:Pr3`), so two bench
//! runs against the same server don't see each other's events.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use gh_server::{events, events::now_us, topic::Topic};
use serde_json::{Value, json};
use tokio::net::{TcpSocket, TcpStream};
use tokio_tungstenite::{MaybeTlsStream, tungstenite::Message};
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
    slow: usize,
    pad: usize,
    metrics: Option<String>,
    reconnect: bool,
    prefix: String,
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
        slow: 0,
        pad: 0,
        metrics: None,
        reconnect: false,
        prefix: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        if flag == "--reconnect" {
            args.reconnect = true;
            continue;
        }
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
            "--slow" => args.slow = number() as usize,
            "--pad" => args.pad = number() as usize,
            "--metrics" => args.metrics = Some(value.clone()),
            "--prefix" => args.prefix = value.clone(),
            _ => usage(&format!("unknown flag {flag}")),
        }
    }
    if args.urls.is_empty() {
        args.urls.push("ws://127.0.0.1:8080/api/v1/ws".into());
    }
    if [args.clients, args.rooms, args.teams, args.tags].contains(&0) || args.rate == 0 {
        usage("counts and --rate must be above 0");
    }
    if Topic::parse(&format!("room:{}r0", args.prefix)).is_err() {
        usage("--prefix may only use letters, digits and -_.");
    }
    args
}

fn usage(problem: &str) -> ! {
    eprintln!(
        "{problem}\nusage: gh-bench [--url ws://...]... [--redis redis://...] [--clients N] \
         [--rooms N] [--teams N] [--tags N] [--rate EVENTS_PER_SEC] [--duration SECS] \
         [--p99-ms MS] [--slow N] [--pad BYTES] [--metrics URL] [--reconnect] [--prefix P]"
    );
    std::process::exit(2);
}

/// The topics client `i` subscribes to.
fn client_topics(i: usize, a: &Args) -> [String; 4] {
    let p = &a.prefix;
    [
        format!("room:{p}r{}", i % a.rooms),
        format!("team:{p}t{}", i % a.teams),
        format!("mentor-tag:{p}tag{}", i % a.tags),
        format!("user:{p}u{i}"),
    ]
}

/// The topic of the `k`th published event, cycling through every kind.
fn event_topic(k: u64, a: &Args) -> String {
    let n = (k / 4) as usize;
    let p = &a.prefix;
    match k % 4 {
        0 => format!("room:{p}r{}", n % a.rooms),
        1 => format!("team:{p}t{}", n % a.teams),
        2 => format!("mentor-tag:{p}tag{}", n % a.tags),
        _ => format!("user:{p}u{}", n % a.clients),
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

/// What one event's `seq` says about the stream, judged the way the app would.
#[derive(Debug, PartialEq)]
enum SeqCheck {
    /// The next number, or the first event on the topic.
    InOrder,
    /// After a reconnect or `resync`: this many events went by while away.
    /// The app reloads the topic over REST.
    MissedWhileAway(u64),
    /// After a reconnect or `resync`, `seq` went down: Redis lost its
    /// counters (restart without persistence). Also a reload.
    Restarted,
    /// A jump nobody announced. Always a bug.
    Gap,
}

/// Last `seq` per topic, plus whether a jump is expected next.
#[derive(Default)]
struct SeqTracker {
    topics: HashMap<Arc<str>, (u64, bool)>,
}

impl SeqTracker {
    /// The client reconnected: any topic may have moved on.
    fn rejoined(&mut self) {
        for (_, expect_jump) in self.topics.values_mut() {
            *expect_jump = true;
        }
    }

    fn resync(&mut self, topic: &str) {
        if let Some((_, expect_jump)) = self.topics.get_mut(topic) {
            *expect_jump = true;
        }
    }

    /// Records an event. Returns the shared topic name and the verdict.
    fn observe(&mut self, topic: &str, seq: u64) -> (Arc<str>, SeqCheck) {
        let key: Arc<str> = self
            .topics
            .get_key_value(topic)
            .map_or_else(|| topic.into(), |(k, _)| k.clone());
        let check = match self.topics.insert(key.clone(), (seq, false)) {
            None => SeqCheck::InOrder,
            Some((prev, _)) if seq == prev + 1 => SeqCheck::InOrder,
            Some((prev, true)) if seq > prev => SeqCheck::MissedWhileAway(seq - prev - 1),
            Some((_, true)) => SeqCheck::Restarted,
            Some((_, false)) => SeqCheck::Gap,
        };
        (key, check)
    }
}

#[derive(Default)]
struct Counters {
    subscribed: AtomicU64,
    live: AtomicI64,
    received: AtomicU64,
    gaps: AtomicU64,
    missed_while_away: AtomicU64,
    counter_restarts: AtomicU64,
    resyncs: AtomicU64,
    errors: AtomicU64,
    dropped: AtomicU64,
    reconnects: AtomicU64,
    refused: AtomicU64,
}

/// What one client saw.
#[derive(Default)]
struct Report {
    /// (topic, seq, microseconds from publish to arrival)
    samples: Vec<(Arc<str>, u64, u64)>,
    /// Microseconds from being disconnected to having every topic back.
    blind: Vec<u64>,
}

/// A cheap per-client random number, for reconnect jitter.
fn jitter(state: &mut u64, max: Duration) -> Duration {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    Duration::from_millis(*state % (max.as_millis() as u64 + 1))
}

const RECONNECT_JITTER: Duration = Duration::from_secs(2);

struct Setup {
    id: usize,
    urls: Arc<Vec<String>>,
    topics: [String; 4],
    counters: Arc<Counters>,
    stop: CancellationToken,
    reconnect: bool,
}

async fn client(c: Setup) -> Report {
    let mut report = Report::default();
    let mut seqs = SeqTracker::default();
    let mut rng = 0x9E37_79B9_7F4A_7C15 ^ (c.id as u64 + 1);
    let mut url_index = c.id;
    let mut away_since: Option<Instant> = None;
    let counters = &c.counters;

    'connect: loop {
        if c.stop.is_cancelled() {
            break;
        }
        let url = &c.urls[url_index % c.urls.len()];
        let mut ws = match tokio_tungstenite::connect_async_with_config(url, None, true).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                if !c.reconnect {
                    eprintln!("connect failed: {e}");
                    counters.errors.fetch_add(1, Relaxed);
                    break;
                }
                // Refused (server down, or 503 while it drains): try the next.
                counters.refused.fetch_add(1, Relaxed);
                url_index += 1;
                let wait =
                    Duration::from_millis(100) + jitter(&mut rng, Duration::from_millis(400));
                tokio::select! {
                    () = c.stop.cancelled() => break,
                    () = tokio::time::sleep(wait) => continue,
                }
            }
        };
        counters.live.fetch_add(1, Relaxed);
        for topic in &c.topics {
            let subscribe = json!({ "op": "subscribe", "topic": topic }).to_string();
            if ws.send(Message::text(subscribe)).await.is_err() {
                counters.errors.fetch_add(1, Relaxed);
            }
        }
        let mut confirmed = 0;

        loop {
            let message = tokio::select! {
                () = c.stop.cancelled() => {
                    let _ = ws.close(None).await;
                    counters.live.fetch_sub(1, Relaxed);
                    break 'connect;
                }
                message = ws.next() => message,
            };
            let text = match message {
                Some(Ok(Message::Text(text))) => text,
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                    counters.live.fetch_sub(1, Relaxed);
                    if !c.reconnect {
                        eprintln!("client {} disconnected: {message:?}", c.id);
                        counters.dropped.fetch_add(1, Relaxed);
                        break 'connect;
                    }
                    counters.reconnects.fetch_add(1, Relaxed);
                    away_since.get_or_insert_with(Instant::now);
                    seqs.rejoined();
                    url_index += 1;
                    tokio::select! {
                        () = c.stop.cancelled() => break 'connect,
                        () = tokio::time::sleep(jitter(&mut rng, RECONNECT_JITTER)) => continue 'connect,
                    }
                }
                Some(Ok(_)) => continue, // pings are answered automatically
            };
            let arrived = now_us();
            let Ok(m) = serde_json::from_str::<Value>(&text) else {
                counters.errors.fetch_add(1, Relaxed);
                continue;
            };
            match m["type"].as_str() {
                Some("subscribed") => {
                    counters.subscribed.fetch_add(1, Relaxed);
                    confirmed += 1;
                    if confirmed == c.topics.len()
                        && let Some(since) = away_since.take()
                    {
                        report.blind.push(since.elapsed().as_micros() as u64);
                    }
                }
                Some("resync") => {
                    counters.resyncs.fetch_add(1, Relaxed);
                    seqs.resync(m["topic"].as_str().unwrap_or_default());
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
                    let (topic, check) = seqs.observe(topic, seq);
                    match check {
                        SeqCheck::InOrder => {}
                        SeqCheck::MissedWhileAway(n) => {
                            counters.missed_while_away.fetch_add(n, Relaxed);
                        }
                        SeqCheck::Restarted => {
                            counters.counter_restarts.fetch_add(1, Relaxed);
                        }
                        SeqCheck::Gap => {
                            counters.gaps.fetch_add(1, Relaxed);
                        }
                    }
                    counters.received.fetch_add(1, Relaxed);
                    report
                        .samples
                        .push((topic, seq, arrived.saturating_sub(sent)));
                }
            }
        }
    }
    report
}

type Ws = tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Topics a slow client subscribes to: every room and mentor tag (the
/// busiest ones), up to the server's 50-subscription limit.
fn slow_topics(a: &Args) -> Vec<String> {
    let p = &a.prefix;
    (0..a.rooms)
        .map(|r| format!("room:{p}r{r}"))
        .chain((0..a.tags).map(|t| format!("mentor-tag:{p}tag{t}")))
        .take(50)
        .collect()
}

/// Connects with a 4KB receive buffer, subscribes, and returns the socket
/// without reading from it again.
async fn slow_client(url: &str, topics: &[String]) -> Ws {
    let host = url.trim_start_matches("ws://").split('/').next().unwrap();
    let addr = tokio::net::lookup_host(host).await.unwrap().next().unwrap();
    let socket = if addr.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .unwrap();
    socket.set_recv_buffer_size(4096).unwrap();
    let stream = socket.connect(addr).await.expect("slow client connect");
    let (mut ws, _) = tokio_tungstenite::client_async(url, MaybeTlsStream::Plain(stream))
        .await
        .expect("slow client upgrade");
    for topic in topics {
        let subscribe = json!({ "op": "subscribe", "topic": topic }).to_string();
        ws.send(Message::text(subscribe)).await.unwrap();
    }
    let mut confirmed = 0;
    while confirmed < topics.len() {
        match ws.next().await {
            Some(Ok(Message::Text(t))) if t.contains("\"subscribed\"") => confirmed += 1,
            Some(Ok(Message::Text(t))) if t.contains("\"error\"") => panic!("slow client: {t}"),
            Some(Ok(_)) => {}
            other => panic!("slow client lost before subscribing: {other:?}"),
        }
    }
    ws
}

/// Reads what a slow client was left with and says how it ended.
async fn slow_outcome(mut ws: Ws) -> &'static str {
    let mut resync = false;
    loop {
        match tokio::time::timeout(Duration::from_secs(10), ws.next()).await {
            Err(_) => return "still connected",
            Ok(Some(Ok(Message::Text(t)))) => resync |= t.contains("\"resync\""),
            Ok(Some(Ok(Message::Close(Some(f))))) if u16::from(f.code) == 1013 => {
                return if resync {
                    "resync then 1013"
                } else {
                    "1013 without resync"
                };
            }
            Ok(Some(Ok(Message::Close(_)))) => return "closed with another code",
            Ok(Some(Ok(_))) => {}
            // The server gave up delivering the close frame to a client that
            // wasn't reading, and dropped the socket.
            Ok(Some(Err(_)) | None) => return "cut off (socket dropped)",
        }
    }
}

/// Fetches a Prometheus page over plain HTTP/1.0.
async fn scrape(url: &str) -> std::io::Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.trim_start_matches("http://");
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut stream = TcpStream::connect(host).await?;
    let request = format!("GET /{path} HTTP/1.0\r\nHost: {host}\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut body = String::new();
    stream.read_to_string(&mut body).await?;
    Ok(body)
}

fn metric(page: &str, name: &str) -> Option<f64> {
    page.lines().find_map(|line| {
        line.strip_prefix(name)?
            .strip_prefix(' ')?
            .trim()
            .parse()
            .ok()
    })
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

fn print_latency(label: &str, sorted: &[u64]) {
    println!(
        "{label:<36} p50 {:>7}µs  p99 {:>7}µs  max {:>7}µs",
        percentile(sorted, 0.50),
        percentile(sorted, 0.99),
        sorted.last().copied().unwrap_or(0),
    );
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let counters = Arc::new(Counters::default());
    let stop = CancellationToken::new();
    let urls = Arc::new(args.urls.clone());

    println!(
        "connecting {} clients to {} server URL(s): {} rooms, {} teams, {} mentor tags, 1 user topic each{}",
        args.clients,
        urls.len(),
        args.rooms,
        args.teams,
        args.tags,
        if args.reconnect { ", reconnecting" } else { "" },
    );
    let mut clients = Vec::with_capacity(args.clients);
    for i in 0..args.clients {
        clients.push(tokio::spawn(client(Setup {
            id: i,
            urls: urls.clone(),
            topics: client_topics(i, &args),
            counters: counters.clone(),
            stop: stop.clone(),
            reconnect: args.reconnect,
        })));
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

    let topics = slow_topics(&args);
    let mut slow = Vec::with_capacity(args.slow);
    for i in 0..args.slow {
        slow.push(slow_client(&urls[i % urls.len()], &topics).await);
    }
    if args.slow > 0 {
        println!(
            "{} slow client(s) subscribed to {} topics each and stopped reading",
            args.slow,
            topics.len()
        );
    }
    let pad = "x".repeat(args.pad);

    let events = events::publisher(&args.redis)
        .await
        .expect("connect to Redis");
    let mut tick = tokio::time::interval(Duration::from_secs_f64(1.0 / args.rate as f64));
    let started = Instant::now();
    let mut published = 0u64;
    let mut publish_failures = 0u64;
    let mut expected = 0u64;
    let mut last_report = Instant::now();
    while started.elapsed() < args.duration {
        tick.tick().await;
        let topic = Topic::parse(&event_topic(published, &args)).unwrap();
        let payload = json!({ "t": now_us(), "pad": pad });
        match events.publish(&topic, "bench", &payload).await {
            Ok(_) => expected += subscribers(published, &args),
            // Redis is down (e.g. during a restart test). The app would retry
            // or surface the error; the bench just counts it.
            Err(_) => publish_failures += 1,
        }
        published += 1;
        if last_report.elapsed() >= Duration::from_secs(10) {
            last_report = Instant::now();
            println!(
                "{:>4}s  live {}  published {published} ({publish_failures} failed)  delivered {}/{expected}  \
                 dropped {}  reconnects {}  refused {}  missed-while-away {}  gaps {}  resyncs {}",
                started.elapsed().as_secs(),
                counters.live.load(Relaxed),
                counters.received.load(Relaxed),
                counters.dropped.load(Relaxed),
                counters.reconnects.load(Relaxed),
                counters.refused.load(Relaxed),
                counters.missed_while_away.load(Relaxed),
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
    let live_at_end = counters.live.load(Relaxed);
    stop.cancel();
    let mut per_delivery = Vec::new();
    let mut blind = Vec::new();
    let mut per_event: HashMap<(Arc<str>, u64), u64> = HashMap::new();
    for client in clients {
        let report = client.await.unwrap();
        blind.extend(report.blind);
        for (topic, seq, latency) in report.samples {
            per_delivery.push(latency);
            let worst = per_event.entry((topic, seq)).or_default();
            *worst = (*worst).max(latency);
        }
    }
    per_delivery.sort_unstable();
    blind.sort_unstable();
    let mut per_event: Vec<u64> = per_event.into_values().collect();
    per_event.sort_unstable();

    let received = counters.received.load(Relaxed);
    let dropped = counters.dropped.load(Relaxed);
    let gaps = counters.gaps.load(Relaxed);
    let errors = counters.errors.load(Relaxed);
    let p99_all = percentile(&per_event, 0.99);

    println!("\n== results ==");
    println!(
        "clients: {} for {:.0}s, {live_at_end} connected at the end, {dropped} dropped, {errors} errors",
        args.clients,
        started.elapsed().as_secs_f64()
    );
    println!(
        "events: {published} published at {}/s ({publish_failures} failed), {received}/{expected} deliveries, \
         {gaps} gaps, {} resyncs",
        args.rate,
        counters.resyncs.load(Relaxed)
    );
    if args.reconnect {
        println!(
            "reconnects: {} disconnections, {} refused attempts, {} events missed while away, \
             {} seq counter restarts",
            counters.reconnects.load(Relaxed),
            counters.refused.load(Relaxed),
            counters.missed_while_away.load(Relaxed),
            counters.counter_restarts.load(Relaxed),
        );
        if !blind.is_empty() {
            print_latency("time without updates per reconnect", &blind);
        }
    }
    print_latency("latency publish -> one client", &per_delivery);
    print_latency("latency publish -> every subscriber", &per_event);

    let mut failures = Vec::new();
    if dropped > 0 || errors > 0 {
        failures.push(format!("{dropped} dropped, {errors} errors"));
    }
    if gaps > 0 {
        failures.push(format!("{gaps} unannounced seq gaps"));
    }
    if args.reconnect {
        // Clients may legitimately miss events while away; they must all be
        // back by the end.
        if live_at_end < args.clients as i64 {
            failures.push(format!(
                "only {live_at_end}/{} clients connected at the end",
                args.clients
            ));
        }
    } else if received != expected {
        failures.push(format!("{received}/{expected} deliveries"));
    }
    if p99_all > args.p99_limit.as_micros() as u64 {
        failures.push(format!(
            "p99 {p99_all}µs over {}ms",
            args.p99_limit.as_millis()
        ));
    }
    if !slow.is_empty() {
        let mut outcomes: HashMap<&str, usize> = HashMap::new();
        for ws in slow {
            *outcomes.entry(slow_outcome(ws).await).or_default() += 1;
        }
        println!("slow clients: {outcomes:?}");
        if let Some(n) = outcomes.get("still connected") {
            failures.push(format!("{n} slow client(s) never cut off"));
        }
    }
    if let Some(url) = &args.metrics {
        match scrape(url).await {
            Ok(page) => {
                let get = |name| metric(&page, name).unwrap_or(f64::NAN);
                let cut = get("ws_slow_client_disconnects_total");
                println!(
                    "server: {cut} slow-client disconnects, {} resyncs, {} connections left",
                    get("ws_resyncs_total"),
                    get("ws_connections"),
                );
                if args.slow > 0 && cut < args.slow as f64 {
                    failures.push(format!(
                        "server cut off {cut} of {} slow clients for being slow",
                        args.slow
                    ));
                }
            }
            Err(e) => failures.push(format!("can't read {url}: {e}")),
        }
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
            slow: 0,
            pad: 0,
            metrics: None,
            reconnect: false,
            prefix: String::new(),
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
    fn every_generated_topic_is_valid_with_or_without_prefix() {
        for prefix in ["", "bad-wifi."] {
            let mut a = args(1000);
            a.prefix = prefix.into();
            for i in 0..1000 {
                for topic in client_topics(i, &a) {
                    Topic::parse(&topic).unwrap();
                    assert!(topic.contains(prefix));
                }
            }
            for topic in slow_topics(&a) {
                Topic::parse(&topic).unwrap();
            }
        }
    }

    #[test]
    fn slow_topics_stay_under_the_subscription_limit() {
        let mut a = args(10);
        assert_eq!(slow_topics(&a).len(), 35);
        a.rooms = 100;
        assert_eq!(slow_topics(&a).len(), 50);
    }

    #[test]
    fn seq_tracker_tells_gaps_from_announced_jumps() {
        let mut t = SeqTracker::default();
        assert_eq!(t.observe("room:1", 5).1, SeqCheck::InOrder); // first seen
        assert_eq!(t.observe("room:1", 6).1, SeqCheck::InOrder);
        assert_eq!(t.observe("room:1", 9).1, SeqCheck::Gap);

        // After a reconnect a jump is expected, once.
        t.rejoined();
        assert_eq!(t.observe("room:1", 15).1, SeqCheck::MissedWhileAway(5));
        assert_eq!(t.observe("room:1", 17).1, SeqCheck::Gap);

        // After a resync, only that topic may jump.
        t.observe("room:2", 1);
        t.resync("room:1");
        assert_eq!(t.observe("room:2", 3).1, SeqCheck::Gap);
        assert_eq!(t.observe("room:1", 20).1, SeqCheck::MissedWhileAway(2));

        // Redis lost its counters: seq starts again, which is fine only when announced.
        t.resync("room:1");
        assert_eq!(t.observe("room:1", 1).1, SeqCheck::Restarted);
        assert_eq!(t.observe("room:1", 1).1, SeqCheck::Gap); // a repeat, unannounced
    }

    #[test]
    fn seq_tracker_reuses_topic_names() {
        let mut t = SeqTracker::default();
        let (a, _) = t.observe("room:1", 1);
        let (b, _) = t.observe("room:1", 2);
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn jitter_stays_in_range_and_varies() {
        let mut state = 42;
        let values: Vec<_> = (0..100)
            .map(|_| jitter(&mut state, Duration::from_millis(2000)))
            .collect();
        assert!(values.iter().all(|d| *d <= Duration::from_millis(2000)));
        assert!(values.iter().any(|d| *d != values[0]));
    }

    #[test]
    fn reads_prometheus_values() {
        let page =
            "# TYPE x counter\nws_resyncs_total 3\nws_resyncs_total_x 9\nws_connections 1000\n";
        assert_eq!(metric(page, "ws_resyncs_total"), Some(3.0));
        assert_eq!(metric(page, "ws_connections"), Some(1000.0));
        assert_eq!(metric(page, "missing"), None);
    }

    #[test]
    fn percentile_picks_nearest_rank() {
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&v, 0.5), 51);
        assert_eq!(percentile(&v, 0.99), 99);
        assert_eq!(percentile(&[], 0.99), 0);
    }
}
