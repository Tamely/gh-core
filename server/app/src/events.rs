//! Events travel through Redis, so every instance sees every event no matter
//! which instance published it. Nothing is ever delivered only in memory.
//!
//! On the wire, channel `gh:events:{topic}` carries `"{seq} {json}"`.

use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use fred::{
    clients::{Pool, SubscriberClient},
    interfaces::{ClientLike, EventInterface, LuaInterface, PubsubInterface},
    prelude::{Builder, Config, ReconnectPolicy},
};
use metrics::{counter, gauge, histogram};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{
    hub::{Frame, Hub},
    topic::Topic,
};

const CHANNEL_PREFIX: &str = "gh:events:";
const SEQ_PREFIX: &str = "gh:seq:";

/// Bumps the topic's counter and publishes in one atomic step. `seq` is
/// assigned once for every instance, and two publishers can't send seq 6
/// before seq 5.
const PUBLISH_SCRIPT: &str = r"
local seq = redis.call('INCR', KEYS[1])
redis.call('PUBLISH', KEYS[2], seq .. ' ' .. ARGV[1])
return seq
";

pub type Error = fred::error::Error;

/// Publishing side. Cheap to clone.
#[derive(Clone)]
pub struct Events {
    pool: Pool,
}

#[derive(Serialize)]
struct Outgoing<'a, P: Serialize> {
    topic: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    payload: &'a P,
    sent_at_us: u64,
}

#[derive(Deserialize)]
struct Incoming<'a> {
    topic: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    #[serde(borrow)]
    payload: &'a RawValue,
    sent_at_us: u64,
}

/// What clients receive: `{topic, seq, type, payload}`.
#[derive(Serialize)]
struct ClientEvent<'a> {
    topic: &'a str,
    seq: u64,
    #[serde(rename = "type")]
    kind: &'a str,
    payload: &'a RawValue,
}

impl Events {
    /// Publishes to every instance and returns the event's `seq`.
    pub async fn publish(
        &self,
        topic: &Topic,
        kind: &str,
        payload: &impl Serialize,
    ) -> Result<u64, Error> {
        let body = serde_json::to_string(&Outgoing {
            topic: topic.as_str(),
            kind,
            payload,
            sent_at_us: now_us(),
        })
        .map_err(|e| Error::new(fred::error::ErrorKind::InvalidArgument, e.to_string()))?;
        let keys = vec![
            format!("{SEQ_PREFIX}{topic}"),
            format!("{CHANNEL_PREFIX}{topic}"),
        ];
        self.pool.eval(PUBLISH_SCRIPT, keys, vec![body]).await
    }

    pub async fn ping(&self) -> Result<(), Error> {
        self.pool.ping::<()>(None).await
    }
}

fn redis_config(url: &str) -> Result<Builder, Error> {
    let mut builder = Builder::from_config(Config::from_url(url)?);
    builder
        .with_connection_config(|c| c.connection_timeout = Duration::from_secs(2))
        // Retry forever: 100ms, 200ms, 400ms ... capped at 10s.
        .set_policy(ReconnectPolicy::new_exponential(0, 100, 10_000, 2));
    Ok(builder)
}

/// Connects a publish-only client (no subscriber), e.g. for tools.
pub async fn publisher(url: &str) -> Result<Events, Error> {
    let pool = redis_config(url)?.build_pool(2)?;
    pool.init().await?;
    Ok(Events { pool })
}

/// Connects the publishing pool and the dedicated subscriber connection, and
/// starts forwarding Redis events into `hub`.
pub async fn connect(url: &str, hub: Arc<Hub>) -> Result<(Events, SubscriberClient), Error> {
    let events = publisher(url).await?;
    let builder = redis_config(url)?;

    let subscriber = builder.build_subscriber_client()?;
    subscriber.init().await?;
    // Re-subscribes automatically after every reconnect.
    subscriber.manage_subscriptions();
    subscriber.psubscribe(format!("{CHANNEL_PREFIX}*")).await?;
    gauge!("redis_connected").set(1.0);

    spawn_forwarding(&subscriber, hub);
    Ok((events, subscriber))
}

fn spawn_forwarding(subscriber: &SubscriberClient, hub: Arc<Hub>) {
    let mut messages = subscriber.message_rx();
    tokio::spawn({
        let hub = hub.clone();
        async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match messages.recv().await {
                    Ok(message) => match message.value.as_str() {
                        Some(raw) => forward(&hub, &raw),
                        None => tracing::warn!("non-text message on {}", message.channel),
                    },
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "event forwarding fell behind Redis");
                        hub.resync_all();
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    });

    // While Redis is unreachable events are lost (pub/sub keeps nothing), so
    // after reconnecting every client is told to reload.
    let down_since: Arc<std::sync::Mutex<Option<Instant>>> = Arc::default();
    let mut errors = subscriber.error_rx();
    tokio::spawn({
        let down_since = down_since.clone();
        async move {
            while let Ok((error, _)) = errors.recv().await {
                tracing::warn!(%error, "Redis subscriber connection lost");
                gauge!("redis_connected").set(0.0);
                down_since.lock().unwrap().get_or_insert_with(Instant::now);
            }
        }
    });
    let mut reconnects = subscriber.reconnect_rx();
    tokio::spawn(async move {
        while reconnects.recv().await.is_ok() {
            counter!("redis_reconnects_total").increment(1);
            gauge!("redis_connected").set(1.0);
            if let Some(since) = down_since.lock().unwrap().take() {
                histogram!("redis_outage_seconds").record(since.elapsed());
            }
            tracing::info!("Redis subscriber reconnected, telling clients to resync");
            hub.resync_all();
        }
    });
}

/// Parses one Redis message and hands it to the hub.
pub fn forward(hub: &Hub, raw: &str) {
    match parse(raw) {
        Ok((topic, frame)) => hub.dispatch(&topic, frame),
        Err(e) => tracing::warn!(error = e, "dropping malformed event"),
    }
}

fn parse(raw: &str) -> Result<(Topic, Frame), &'static str> {
    let (seq, body) = raw.split_once(' ').ok_or("missing seq")?;
    let seq: u64 = seq.parse().map_err(|_| "bad seq")?;
    let event: Incoming = serde_json::from_str(body).map_err(|_| "bad json")?;
    let topic = Topic::parse(event.topic)?;
    let text = serde_json::to_string(&ClientEvent {
        topic: event.topic,
        seq,
        kind: event.kind,
        payload: event.payload,
    })
    .map_err(|_| "can't serialize")?;
    Ok((
        topic,
        Frame::Event {
            text: text.into(),
            sent_at_us: event.sent_at_us,
        },
    ))
}

pub fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_micros() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_published_event_into_client_envelope() {
        let raw = r#"7 {"topic":"room:PFT-1200","type":"queue.advanced","payload":{"k":3},"sent_at_us":5}"#;
        let (topic, frame) = parse(raw).unwrap();
        assert_eq!(topic.as_str(), "room:PFT-1200");
        let Frame::Event { text, sent_at_us } = frame else {
            panic!("expected an event")
        };
        assert_eq!(sent_at_us, 5);
        assert_eq!(
            text.as_str(),
            r#"{"topic":"room:PFT-1200","seq":7,"type":"queue.advanced","payload":{"k":3}}"#
        );
    }

    #[test]
    fn rejects_malformed_messages() {
        for raw in [
            "",
            "7",
            "x {}",
            "7 not-json",
            r#"7 {"topic":"bad topic","type":"t","payload":1,"sent_at_us":0}"#,
            r#"7 {"type":"t","payload":1,"sent_at_us":0}"#,
            r#"-1 {"topic":"room:1","type":"t","payload":1,"sent_at_us":0}"#,
        ] {
            assert!(parse(raw).is_err(), "{raw:?} should be rejected");
        }
    }
}
