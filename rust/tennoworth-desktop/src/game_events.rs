//! What the game says, for whoever needs to hear it.
//!
//! The EE.log tailer lives with the trade ledger, but log lines are not the
//! ledger's alone: the reward overlay reacts to them too, and later in-game
//! features will. The tailer publishes here and startup decides who listens, so
//! the service that reads the log never has to know which features exist.

use std::sync::RwLock;

/// One observation of the game's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameEvent<'a> {
    /// A complete new line, as the tailer read it.
    LogLine(&'a str),
    /// The recent tail of the log, re-read whole. Reward slots arrive as a
    /// batch of lines that only make sense together.
    RecentLog(&'a str),
}

type Subscriber = Box<dyn Fn(GameEvent<'_>) + Send + Sync>;

#[derive(Default)]
pub struct GameEvents {
    subscribers: RwLock<Vec<Subscriber>>,
}

impl GameEvents {
    pub fn subscribe(&self, subscriber: impl Fn(GameEvent<'_>) + Send + Sync + 'static) {
        self.subscribers
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(Box::new(subscriber));
    }

    /// Delivered on the publishing thread, in subscription order. A subscriber
    /// runs on the tailer's poll, so it must hand slow work to its own thread.
    pub fn publish(&self, event: GameEvent<'_>) {
        for subscriber in self
            .subscribers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            subscriber(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn every_subscriber_hears_every_event_in_order() {
        let events = GameEvents::default();
        let heard = Arc::new(Mutex::new(Vec::new()));
        for name in ["first", "second"] {
            let heard = heard.clone();
            events.subscribe(move |event| {
                heard.lock().unwrap().push(format!("{name}: {event:?}"));
            });
        }

        events.publish(GameEvent::LogLine("a"));
        events.publish(GameEvent::RecentLog("b"));

        assert_eq!(
            *heard.lock().unwrap(),
            vec![
                r#"first: LogLine("a")"#,
                r#"second: LogLine("a")"#,
                r#"first: RecentLog("b")"#,
                r#"second: RecentLog("b")"#,
            ]
        );
    }

    #[test]
    fn publishing_with_no_subscribers_is_silent() {
        GameEvents::default().publish(GameEvent::LogLine("nobody listens"));
    }
}
