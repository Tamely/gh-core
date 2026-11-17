//! `GET /api/v1/ws`: clients subscribe to topics and receive their events.
//!
//! Client to server: `{"op":"subscribe","topic":"room:PFT-1200"}` or
//! `{"op":"unsubscribe","topic":"..."}`.
//!
//! Server to client:
//! - events: `{"topic","seq","type","payload"}`. `seq` goes up by one per
//!   topic; a gap means something was missed.
//! - `{"type":"resync","topic"}`: reload that topic over REST.
//! - `{"type":"subscribed"|"unsubscribed","topic"}` and
//!   `{"type":"error","topic"?,"payload":{"reason"}}`.
//!
//! Each connection has a bounded send queue, filled by one task per
//! subscription and drained by one writer task. A slow client only ever
//! fills its own queue: when it's half full the client gets `resync`, and
//! when it's full the client is closed with 1013. Nobody else waits.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU16, AtomicU32, Ordering},
    },
    time::Duration,
};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{CloseFrame, Message, Utf8Bytes, WebSocket, close_code},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use metrics::{counter, gauge, histogram};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::{AppState, auth::Identity, events::now_us, hub::Frame, topic::Topic};

pub const MAX_MESSAGE_BYTES: usize = 4 * 1024;
pub const MAX_SUBSCRIPTIONS: usize = 50;
/// Messages waiting to be written to one client.
pub const SEND_QUEUE: usize = 64;
/// Queue length at which a client is considered behind and gets `resync`.
const BEHIND_AT: usize = SEND_QUEUE / 2;
/// Unanswered pings before the connection is dropped.
const MAX_MISSED_PONGS: u32 = 2;
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub ping_interval: Duration,
    /// How long to try sending a close frame to a client that isn't reading
    /// before dropping the socket.
    pub close_timeout: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(25),
            close_timeout: Duration::from_secs(1),
        }
    }
}

pub async fn upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // While shutting down, send clients to another instance.
    if state.shutdown.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Some(identity) = state.authorizer.identify(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        // The defaults are 128KiB per connection each; 1,000 connections
        // would spend 256MB on mostly empty buffers.
        .read_buffer_size(MAX_MESSAGE_BYTES)
        .write_buffer_size(0)
        .on_upgrade(move |socket| run(socket, state, identity))
}

enum Out {
    Event { text: Utf8Bytes, sent_at_us: u64 },
    Control(Utf8Bytes),
    Ping,
}

/// State shared by a connection's tasks.
struct Conn {
    /// Cancelled when the connection should end, for any reason.
    closed: CancellationToken,
    /// The close code to send; the first reason wins.
    close_code: AtomicU16,
    missed_pongs: AtomicU32,
}

impl Conn {
    fn close(&self, code: u16) {
        let _ = self
            .close_code
            .compare_exchange(0, code, Ordering::SeqCst, Ordering::SeqCst);
        self.closed.cancel();
    }

    /// Queues a message, or closes the connection with 1013 if the queue is
    /// full. Returns false when the connection is finished.
    fn push(&self, tx: &mpsc::Sender<Out>, out: Out) -> bool {
        match tx.try_send(out) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                if !self.closed.is_cancelled() {
                    counter!("ws_slow_client_disconnects_total").increment(1);
                }
                self.close(close_code::AGAIN);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }
}

fn control(value: serde_json::Value) -> Out {
    Out::Control(value.to_string().into())
}

fn resync(topic: &Topic) -> Out {
    counter!("ws_resyncs_total").increment(1);
    control(json!({ "type": "resync", "topic": topic.as_str() }))
}

fn error(topic: Option<&str>, reason: &str) -> Out {
    control(json!({ "type": "error", "topic": topic, "payload": { "reason": reason } }))
}

async fn run(socket: WebSocket, state: AppState, identity: Identity) {
    gauge!("ws_connections").increment(1.0);
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::channel(SEND_QUEUE);
    let conn = Arc::new(Conn {
        closed: CancellationToken::new(),
        close_code: AtomicU16::new(0),
        missed_pongs: AtomicU32::new(0),
    });

    let writer = tokio::spawn(write_loop(sink, rx, conn.clone(), state.ws.close_timeout));
    tokio::spawn(heartbeat(tx.clone(), conn.clone(), state.ws.ping_interval));

    let mut subscriptions: HashMap<Topic, CancellationToken> = HashMap::new();
    loop {
        let message = tokio::select! {
            () = conn.closed.cancelled() => break,
            () = state.shutdown.cancelled() => {
                conn.close(close_code::RESTART);
                break;
            }
            message = stream.next() => message,
        };
        match message {
            Some(Ok(Message::Text(text))) => {
                let reply = handle(&text, &state, &identity, &tx, &conn, &mut subscriptions);
                if !conn.push(&tx, reply) {
                    break;
                }
            }
            Some(Ok(Message::Pong(_))) => conn.missed_pongs.store(0, Ordering::SeqCst),
            // Pings are answered automatically.
            Some(Ok(Message::Ping(_))) => {}
            Some(Ok(Message::Binary(_))) => {
                conn.close(close_code::UNSUPPORTED);
                break;
            }
            Some(Ok(Message::Close(_))) => {
                conn.close(close_code::NORMAL);
                break;
            }
            Some(Err(e)) => {
                // Includes messages over the 4KB limit.
                tracing::debug!(error = %e, "WebSocket read error");
                conn.close(close_code::POLICY);
                break;
            }
            None => {
                conn.close(close_code::AWAY);
                break;
            }
        }
    }

    conn.closed.cancel();
    for (_, stop) in subscriptions.drain() {
        stop.cancel();
    }
    drop(tx);
    let _ = writer.await;
    // Finish the closing handshake: wait for the client's close reply before
    // dropping the socket. Dropping it first can reset the connection and
    // make the client lose the close frame.
    let drain = async { while let Some(Ok(_)) = stream.next().await {} };
    let _ = tokio::time::timeout(state.ws.close_timeout, drain).await;
    gauge!("ws_connections").decrement(1.0);
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Command {
    Subscribe { topic: String },
    Unsubscribe { topic: String },
}

fn handle(
    text: &str,
    state: &AppState,
    identity: &Identity,
    tx: &mpsc::Sender<Out>,
    conn: &Arc<Conn>,
    subscriptions: &mut HashMap<Topic, CancellationToken>,
) -> Out {
    let command = match serde_json::from_str::<Command>(text) {
        Ok(command) => command,
        Err(_) => {
            return error(
                None,
                "expected {\"op\":\"subscribe\"|\"unsubscribe\",\"topic\":...}",
            );
        }
    };
    let (op, name) = match &command {
        Command::Subscribe { topic } => ("subscribed", topic),
        Command::Unsubscribe { topic } => ("unsubscribed", topic),
    };
    let topic = match Topic::parse(name) {
        Ok(topic) => topic,
        Err(reason) => return error(Some(name), reason),
    };

    match command {
        Command::Subscribe { .. } if !subscriptions.contains_key(&topic) => {
            if subscriptions.len() >= MAX_SUBSCRIPTIONS {
                return error(Some(name), "subscription limit reached");
            }
            if !state.authorizer.can_subscribe(identity, &topic) {
                return error(Some(name), "not allowed");
            }
            let stop = conn.closed.child_token();
            let rx = state.hub.subscribe(&topic);
            tokio::spawn(forward(
                topic.clone(),
                rx,
                tx.clone(),
                conn.clone(),
                stop.clone(),
                state.clone(),
            ));
            subscriptions.insert(topic.clone(), stop);
        }
        Command::Subscribe { .. } => {} // already subscribed
        Command::Unsubscribe { .. } => {
            if let Some(stop) = subscriptions.remove(&topic) {
                stop.cancel();
            }
        }
    }
    control(json!({ "type": op, "topic": topic.as_str() }))
}

/// Moves one topic's events into the connection's send queue.
async fn forward(
    topic: Topic,
    mut rx: broadcast::Receiver<Frame>,
    tx: mpsc::Sender<Out>,
    conn: Arc<Conn>,
    stop: CancellationToken,
    state: AppState,
) {
    let kind = topic.kind().as_str();
    gauge!("ws_subscriptions", "kind" => kind).increment(1.0);
    let mut behind = false;

    loop {
        let frame = tokio::select! {
            () = stop.cancelled() => break,
            frame = rx.recv() => frame,
        };
        let queued = tx.max_capacity() - tx.capacity();
        if queued == 0 {
            behind = false;
        }
        let out = match frame {
            Ok(Frame::Event { text, sent_at_us }) => {
                if !behind && queued >= BEHIND_AT {
                    behind = true;
                    if !conn.push(&tx, resync(&topic)) {
                        break;
                    }
                }
                Out::Event { text, sent_at_us }
            }
            Ok(Frame::Resync) | Err(broadcast::error::RecvError::Lagged(_)) => {
                behind = true;
                resync(&topic)
            }
            Err(broadcast::error::RecvError::Closed) => break,
        };
        if !conn.push(&tx, out) {
            break;
        }
    }

    drop(rx);
    state.hub.release(&topic);
    gauge!("ws_subscriptions", "kind" => kind).decrement(1.0);
}

async fn heartbeat(tx: mpsc::Sender<Out>, conn: Arc<Conn>, every: Duration) {
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    loop {
        tokio::select! {
            () = conn.closed.cancelled() => return,
            _ = interval.tick() => {}
        }
        if conn.missed_pongs.fetch_add(1, Ordering::SeqCst) >= MAX_MISSED_PONGS {
            conn.close(close_code::AWAY);
            return;
        }
        if !conn.push(&tx, Out::Ping) {
            return;
        }
    }
}

async fn write_loop(
    mut sink: SplitSink<WebSocket, Message>,
    mut rx: mpsc::Receiver<Out>,
    conn: Arc<Conn>,
    close_timeout: Duration,
) {
    loop {
        let out = tokio::select! {
            biased;
            () = conn.closed.cancelled() => break,
            out = rx.recv() => match out {
                Some(out) => out,
                None => break,
            },
        };
        let (message, sent_at_us) = match out {
            Out::Event { text, sent_at_us } => (Message::Text(text), Some(sent_at_us)),
            Out::Control(text) => (Message::Text(text), None),
            Out::Ping => (Message::Ping(Default::default()), None),
        };
        // A client that stops reading blocks this send. Closing the
        // connection abandons it rather than waiting.
        let sent = tokio::select! {
            () = conn.closed.cancelled() => break,
            sent = sink.send(message) => sent,
        };
        if sent.is_err() {
            conn.close(close_code::AWAY);
            return;
        }
        if let Some(sent_at_us) = sent_at_us {
            counter!("ws_messages_sent_total").increment(1);
            let latency = now_us().saturating_sub(sent_at_us);
            histogram!("event_delivery_seconds").record(latency as f64 / 1e6);
        }
    }

    let code = match conn.close_code.load(Ordering::SeqCst) {
        0 => close_code::NORMAL,
        code => code,
    };
    // Queued events are stale now, but a queued `resync` or error still
    // matters to the client, so send those before the close frame.
    let goodbye = async {
        while let Ok(out) = rx.try_recv() {
            if let Out::Control(text) = out {
                sink.send(Message::Text(text)).await?;
            }
        }
        let close = CloseFrame {
            code,
            reason: Utf8Bytes::default(),
        };
        sink.send(Message::Close(Some(close))).await
    };
    let _ = tokio::time::timeout(close_timeout, goodbye).await;
}
