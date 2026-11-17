//! Fans events out to this instance's WebSocket connections: one broadcast
//! channel per topic that has at least one subscriber.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use axum::extract::ws::Utf8Bytes;
use tokio::sync::broadcast;

use crate::topic::Topic;

/// How many events a topic keeps for a subscriber that hasn't taken them yet.
/// A subscriber further behind than this gets `resync`.
pub const TOPIC_BUFFER: usize = 256;

#[derive(Clone, Debug)]
pub enum Frame {
    /// An event, already serialized for clients.
    Event { text: Utf8Bytes, sent_at_us: u64 },
    /// Events may have been lost (e.g. Redis reconnected): clients should
    /// reload this topic over REST.
    Resync,
}

#[derive(Default)]
pub struct Hub {
    topics: Mutex<HashMap<Topic, broadcast::Sender<Frame>>>,
}

impl Hub {
    pub fn subscribe(&self, topic: &Topic) -> broadcast::Receiver<Frame> {
        self.lock()
            .entry(topic.clone())
            .or_insert_with(|| broadcast::channel(TOPIC_BUFFER).0)
            .subscribe()
    }

    /// Forgets a topic once its last subscriber has gone. Call after dropping
    /// the receiver.
    pub fn release(&self, topic: &Topic) {
        let mut topics = self.lock();
        if topics.get(topic).is_some_and(|tx| tx.receiver_count() == 0) {
            topics.remove(topic);
        }
    }

    pub fn dispatch(&self, topic: &Topic, frame: Frame) {
        if let Some(tx) = self.lock().get(topic) {
            // Fails only when nobody is subscribed, which is fine.
            let _ = tx.send(frame);
        }
    }

    /// Tells every subscriber of every topic to reload.
    pub fn resync_all(&self) {
        for tx in self.lock().values() {
            let _ = tx.send(Frame::Resync);
        }
    }

    pub fn topic_count(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Topic, broadcast::Sender<Frame>>> {
        // A panic while holding this lock can't leave the map half-updated,
        // so a poisoned lock is safe to keep using.
        self.topics.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(text: &str) -> Frame {
        Frame::Event {
            text: text.into(),
            sent_at_us: 0,
        }
    }

    #[tokio::test]
    async fn delivers_only_to_the_matching_topic() {
        let hub = Hub::default();
        let a = Topic::parse("room:a").unwrap();
        let b = Topic::parse("room:b").unwrap();
        let mut rx_a = hub.subscribe(&a);
        let mut rx_b = hub.subscribe(&b);

        hub.dispatch(&a, event("for a"));
        assert!(matches!(rx_a.recv().await.unwrap(), Frame::Event { text, .. } if text == "for a"));
        assert!(rx_b.try_recv().is_err());
    }

    #[test]
    fn dispatch_without_subscribers_is_a_no_op() {
        let hub = Hub::default();
        hub.dispatch(&Topic::parse("room:x").unwrap(), event("nobody"));
        assert_eq!(hub.topic_count(), 0);
    }

    #[test]
    fn release_forgets_topic_only_after_last_subscriber() {
        let hub = Hub::default();
        let t = Topic::parse("team:1").unwrap();
        let first = hub.subscribe(&t);
        let second = hub.subscribe(&t);

        drop(first);
        hub.release(&t);
        assert_eq!(hub.topic_count(), 1);

        drop(second);
        hub.release(&t);
        assert_eq!(hub.topic_count(), 0);
    }

    #[tokio::test]
    async fn resync_all_reaches_every_topic() {
        let hub = Hub::default();
        let mut a = hub.subscribe(&Topic::parse("room:a").unwrap());
        let mut b = hub.subscribe(&Topic::parse("user:b").unwrap());
        hub.resync_all();
        assert!(matches!(a.recv().await.unwrap(), Frame::Resync));
        assert!(matches!(b.recv().await.unwrap(), Frame::Resync));
    }

    #[tokio::test]
    async fn subscriber_too_far_behind_sees_lag() {
        let hub = Hub::default();
        let t = Topic::parse("room:busy").unwrap();
        let mut rx = hub.subscribe(&t);
        for i in 0..TOPIC_BUFFER + 10 {
            hub.dispatch(&t, event(&i.to_string()));
        }
        assert!(matches!(
            rx.recv().await,
            Err(broadcast::error::RecvError::Lagged(10))
        ));
    }
}
