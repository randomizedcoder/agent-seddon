//! Live agent-session observation (docs/design/portal): a broadcast event sink the
//! loop publishes into, and a [`SessionSource`] a served `AgentSessionService`
//! reads.
//!
//! The loop calls [`SessionEvents::publish`] at its existing recording sites — no
//! new control flow. Snapshot-affecting events (context/mode/run) update a shared
//! [`StatusSnapshot`] so a late subscriber and the `Snapshot` RPC see live state
//! without reaching into the transient `Session`. The channel is **bounded**: a
//! slow subscriber lags and drops (a `Lagged` item is skipped) rather than stalling
//! the loop — the same drop-not-block discipline as the ClickHouse sink.

use std::sync::Mutex;

use agent_core::{SessionEvent, SessionEventStream, SessionSource, StatusSnapshot};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// Ring-buffer capacity for the broadcast channel. Bounds memory; excess for a slow
/// consumer is dropped (surfaced as a skipped `Lagged` item), never queued unbounded.
const CHANNEL_CAPACITY: usize = 512;

/// The shared observation handle: a broadcast sender + the latest snapshot. Held on
/// the `Agent` (so `&Agent`-only methods can publish) and handed to the service as
/// an `Arc<dyn SessionSource>`.
pub struct SessionEvents {
    tx: broadcast::Sender<SessionEvent>,
    snap: Mutex<StatusSnapshot>,
}

impl SessionEvents {
    /// A fresh sink seeded with the model's `context_window` (so a `Snapshot` before
    /// the first turn already reports the budget).
    pub fn new(context_window: u32) -> Self {
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            tx,
            snap: Mutex::new(StatusSnapshot {
                context_window,
                ..Default::default()
            }),
        }
    }

    /// Whether anyone is currently subscribed — a cheap atomic load the hot token
    /// path checks before allocating a `TokenDelta`.
    pub fn has_subscribers(&self) -> bool {
        self.tx.receiver_count() > 0
    }

    /// Update the snapshot (for the snapshot-affecting kinds) and broadcast the
    /// event. Sending is a no-op when there are no subscribers; the snapshot is kept
    /// current either way. Never blocks, never errors out to the caller.
    pub fn publish(&self, event: SessionEvent) {
        self.apply_to_snapshot(&event);
        // `send` returns Err only when there are no receivers — ignore it.
        let _ = self.tx.send(event);
    }

    fn apply_to_snapshot(&self, event: &SessionEvent) {
        let mut s = self.snap.lock().expect("session snapshot poisoned");
        match event {
            SessionEvent::RunStarted { .. } => s.active = true,
            SessionEvent::RunFinished { .. } => s.active = false,
            SessionEvent::ModeSwitch { to, .. } => s.current_mode = to.clone(),
            SessionEvent::ContextUpdate {
                prompt_tokens,
                context_window,
                messages,
            } => {
                s.context_tokens = *prompt_tokens;
                s.context_window = *context_window;
                s.context_messages = *messages;
            }
            _ => {}
        }
    }
}

impl SessionSource for SessionEvents {
    fn snapshot(&self) -> StatusSnapshot {
        self.snap.lock().expect("session snapshot poisoned").clone()
    }

    fn subscribe(&self) -> SessionEventStream {
        // Drop `Lagged` items (slow-consumer backpressure = drop, not stall).
        let stream = BroadcastStream::new(self.tx.subscribe()).filter_map(|r| r.ok());
        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    // `StreamExt` (for `.next()`) comes in via `super::*` — the module-level import.
    use super::*;
    use std::sync::Arc;

    #[test]
    fn snapshot_seeds_context_window() {
        let e = SessionEvents::new(8192);
        assert_eq!(e.snapshot().context_window, 8192);
        assert!(!e.snapshot().active);
    }

    #[test]
    fn positive_publish_updates_snapshot_without_subscribers() {
        let e = SessionEvents::new(1000);
        assert!(!e.has_subscribers());
        e.publish(SessionEvent::RunStarted { goal: "g".into() });
        e.publish(SessionEvent::ModeSwitch {
            from: "other".into(),
            to: "debug".into(),
            reason: "r".into(),
            confidence: 0.9,
        });
        e.publish(SessionEvent::ContextUpdate {
            prompt_tokens: 42,
            context_window: 1000,
            messages: 7,
        });
        let s = e.snapshot();
        assert!(s.active);
        assert_eq!(s.current_mode, "debug");
        assert_eq!(s.context_tokens, 42);
        assert_eq!(s.context_messages, 7);
        e.publish(SessionEvent::RunFinished { ok: true });
        assert!(!e.snapshot().active);
    }

    #[tokio::test]
    async fn positive_subscriber_receives_published_events() {
        let e = Arc::new(SessionEvents::new(1000));
        let mut stream = e.subscribe();
        assert!(e.has_subscribers());
        e.publish(SessionEvent::TokenDelta { text: "hi".into() });
        let ev = stream.next().await.expect("an event");
        match ev {
            SessionEvent::TokenDelta { text } => assert_eq!(text, "hi"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
