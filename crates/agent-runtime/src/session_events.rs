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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use agent_core::{
    SessionEvent, SessionEventStream, SessionSource, SessionSourceRegistry, StatusSnapshot,
};
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
            SessionEvent::ModeSwitch { to, .. } => s.current_mode.clone_from(to),
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
        let stream = BroadcastStream::new(self.tx.subscribe()).filter_map(std::result::Result::ok);
        Box::pin(stream)
    }
}

/// One [`SessionEvents`] sink **per live session**, keyed by session id
/// (docs/design/multi-session/03-hazards.md, hazard B). Held on the shared `Agent`
/// backend; each `Session` gets its own sink from [`Self::get_or_create`] at
/// construction so concurrent tenants never share a broadcast channel or snapshot —
/// Bob's `Subscribe` no longer sees Alice's tokens. A served `AgentSessionService`
/// reads this as an `Arc<dyn SessionSourceRegistry>` and selects by `session_id`.
///
/// Keyed by the **session** segment alone (not the full `(user, session)` key): the
/// wire selector is a bare `session_id`, and session ids are unique (a UUID minted
/// per run; server-minted ids arrive in increment 05). Under the locked "identity is
/// only as trustworthy as the transport" stance a colliding id is a transport-trust
/// question, not one this map second-guesses.
pub struct SessionEventsRegistry {
    /// Model context window, forwarded to each new sink so a `Snapshot` before the
    /// first turn already reports the budget (mirrors [`SessionEvents::new`]).
    context_window: u32,
    map: Mutex<HashMap<String, Arc<SessionEvents>>>,
}

impl SessionEventsRegistry {
    /// A fresh registry; new sinks are seeded with `context_window`.
    pub fn new(context_window: u32) -> Self {
        Self {
            context_window,
            map: Mutex::new(HashMap::new()),
        }
    }

    /// The sink for `session_id`, created on first use (lazy-per-session, mirroring
    /// [`crate::SessionManager::get_or_create`]). The map lock is held only for the
    /// lookup/insert.
    pub fn get_or_create(&self, session_id: &str) -> Arc<SessionEvents> {
        self.map
            .lock()
            .expect("session events map poisoned")
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(SessionEvents::new(self.context_window)))
            .clone()
    }

    /// Retire a finished session's sink (idempotent). Called from
    /// [`crate::SessionManager::remove`] so a dead session stops being observable and
    /// its channel is freed — the eviction half of the low-hundreds bound.
    pub fn remove(&self, session_id: &str) {
        self.map
            .lock()
            .expect("session events map poisoned")
            .remove(session_id);
    }

    /// Number of live sinks.
    pub fn len(&self) -> usize {
        self.map.lock().expect("session events map poisoned").len()
    }

    /// Whether no sinks are live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SessionSourceRegistry for SessionEventsRegistry {
    fn source(&self, session_id: &str) -> Option<Arc<dyn SessionSource>> {
        self.map
            .lock()
            .expect("session events map poisoned")
            .get(session_id)
            .map(|e| e.clone() as Arc<dyn SessionSource>)
    }

    fn sole_source(&self) -> Option<Arc<dyn SessionSource>> {
        let map = self.map.lock().expect("session events map poisoned");
        if map.len() == 1 {
            map.values()
                .next()
                .map(|e| e.clone() as Arc<dyn SessionSource>)
        } else {
            None
        }
    }

    fn live_session_ids(&self) -> Vec<String> {
        self.map
            .lock()
            .expect("session events map poisoned")
            .keys()
            .cloned()
            .collect()
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

    // --- registry: one sink per session, keyed and evictable -----------------

    #[test]
    fn positive_get_or_create_is_idempotent_per_session() {
        let reg = SessionEventsRegistry::new(4096);
        assert!(reg.is_empty());
        let a1 = reg.get_or_create("alice-s1");
        let a2 = reg.get_or_create("alice-s1");
        assert!(Arc::ptr_eq(&a1, &a2), "same id returns the same sink");
        assert_eq!(reg.len(), 1);
        // A fresh sink is seeded with the model window.
        assert_eq!(a1.snapshot().context_window, 4096);
    }

    #[test]
    fn positive_distinct_sessions_get_distinct_sinks() {
        let reg = SessionEventsRegistry::new(1000);
        let alice = reg.get_or_create("alice-s1");
        let bob = reg.get_or_create("bob-s2");
        assert!(!Arc::ptr_eq(&alice, &bob), "distinct ids → distinct sinks");
        assert_eq!(reg.len(), 2);

        // The whole point of hazard B: a publish into one sink is invisible to the
        // other, so a per-session `Subscribe` never sees a neighbour's events.
        alice.publish(SessionEvent::RunStarted { goal: "a".into() });
        assert!(alice.snapshot().active);
        assert!(
            !bob.snapshot().active,
            "bob's snapshot is untouched by alice"
        );
    }

    #[test]
    fn positive_source_resolves_only_live_sessions() {
        let reg = SessionEventsRegistry::new(1000);
        reg.get_or_create("s1");
        assert!(reg.source("s1").is_some());
        assert!(
            reg.source("nope").is_none(),
            "unknown id → None, never fabricated"
        );
    }

    #[test]
    fn corner_sole_source_only_when_exactly_one() {
        let reg = SessionEventsRegistry::new(1000);
        assert!(reg.sole_source().is_none(), "zero live → no sole");
        reg.get_or_create("s1");
        assert!(reg.sole_source().is_some(), "exactly one → sole");
        reg.get_or_create("s2");
        assert!(reg.sole_source().is_none(), "ambiguous when many");
    }

    #[test]
    fn positive_remove_evicts_and_is_idempotent() {
        let reg = SessionEventsRegistry::new(1000);
        reg.get_or_create("s1");
        assert_eq!(reg.live_session_ids(), vec!["s1".to_string()]);
        reg.remove("s1");
        assert!(reg.is_empty());
        assert!(reg.source("s1").is_none());
        reg.remove("s1"); // absent id is a no-op, never panics
        reg.remove("never-existed");
    }
}
