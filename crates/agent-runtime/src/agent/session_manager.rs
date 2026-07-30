//! Multi-session pool. `SessionManager` owns the shared `Agent` backend and a map of
//! live `Session`s keyed by `(user, session)`, admitting/reaping them under capacity
//! caps so one process serves many concurrent sessions. Extracted from `agent.rs`;
//! `OpenError` + `SessionManager` are re-exported through the `agent` module, so
//! `agent_runtime::{OpenError, SessionManager}` is unchanged.

use super::*;
use std::sync::Arc;

/// Owns the shared [`Agent`] backend and a map of live [`Session`]s keyed by
/// `(user, session)`, so one process serves many concurrent sessions. Turns *within*
/// a session are serialized by its per-session async mutex; *distinct* sessions run
/// in parallel over the shared backend, which is `Send + Sync` and mostly functional
/// (docs/design/multi-session/02-runtime-split.md).
///
/// The single-session CLI/REPL can ignore this and call [`Agent::session`] directly;
/// the multi-tenant (portal) path drives sessions through [`Self::get_or_create`].
/// Rejected [`SessionManager::open`] — a *new* session would exceed a capacity cap.
/// The amplification guard against a hostile client spraying session ids
/// (docs/design/multi-session/05-lifecycle.md); a wire `SessionRegistry.Open` maps
/// this to `RESOURCE_EXHAUSTED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// The per-user live-session cap (the contained value) is full.
    PerUserLimit(usize),
    /// The global live-session cap (the contained value) is full.
    TotalLimit(usize),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::PerUserLimit(n) => write!(f, "per-user session limit reached ({n})"),
            OpenError::TotalLimit(n) => write!(f, "global session limit reached ({n})"),
        }
    }
}

impl std::error::Error for OpenError {}

/// One live session plus its idle-GC bookkeeping.
struct Entry {
    session: Arc<tokio::sync::Mutex<Session>>,
    /// Wall-clock of the last dispatch/heartbeat, for idle reaping.
    last_used: std::time::Instant,
}

pub struct SessionManager {
    backend: Arc<Agent>,
    sessions: std::sync::Mutex<std::collections::HashMap<agent_core::SessionKey, Entry>>,
    /// Global live-session cap (`0` = unbounded). See [`Self::open`].
    max_total: usize,
    /// Per-user live-session cap (`0` = unbounded).
    max_per_user: usize,
}

impl SessionManager {
    /// Wrap a built backend, with no capacity caps (the caps are for the multi-tenant
    /// serve path — [`Self::with_limits`]).
    pub fn new(backend: Arc<Agent>) -> Self {
        Self {
            backend,
            sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
            max_total: 0,
            max_per_user: 0,
        }
    }

    /// Set the amplification-guard caps: at most `max_total` live sessions overall and
    /// `max_per_user` per user (`0` = unbounded). Only [`Self::open`] enforces them;
    /// the lazy [`Self::get_or_create`] correctness path never rejects.
    pub fn with_limits(mut self, max_total: usize, max_per_user: usize) -> Self {
        self.max_total = max_total;
        self.max_per_user = max_per_user;
        self
    }

    /// The shared backend (for the `--serve-<seam>` accessors and cleanup).
    pub fn backend(&self) -> &Arc<Agent> {
        &self.backend
    }

    /// Look up the session for `key`, creating it on first use, and mark it used. Lock
    /// the returned handle to take a turn. The map lock is held only for the
    /// lookup/insert, never across `.await`, so distinct sessions never contend.
    ///
    /// This is the **lazy correctness path** and never rejects: in a split deployment a
    /// seam may receive its first request for a session another process `Open`ed, and
    /// must allocate on demand (docs/design/multi-session/05-lifecycle.md). Capacity is
    /// enforced only at the trusted entry point, [`Self::open`].
    pub fn get_or_create(&self, key: agent_core::SessionKey) -> Arc<tokio::sync::Mutex<Session>> {
        let mut map = self.sessions.lock().expect("session map poisoned");
        let entry = map.entry(key.clone()).or_insert_with(|| Entry {
            session: Arc::new(tokio::sync::Mutex::new(self.backend.session_with(key))),
            last_used: std::time::Instant::now(),
        });
        entry.last_used = std::time::Instant::now();
        entry.session.clone()
    }

    /// Capacity-checked admission (backs the wire `SessionRegistry.Open`): return the
    /// session for `key`, but if it does **not** already exist, reject when creating it
    /// would exceed the total or per-user cap. An existing session is always returned
    /// (and touched), so a legitimate client re-entering its own session is never
    /// throttled — only a hostile *new*-id spray is (docs/design/multi-session/05).
    #[allow(clippy::result_large_err)]
    pub fn admit(
        &self,
        key: agent_core::SessionKey,
    ) -> Result<Arc<tokio::sync::Mutex<Session>>, OpenError> {
        let mut map = self.sessions.lock().expect("session map poisoned");
        if !map.contains_key(&key) {
            if self.max_total > 0 && map.len() >= self.max_total {
                return Err(OpenError::TotalLimit(self.max_total));
            }
            if self.max_per_user > 0 {
                let per_user = map.keys().filter(|k| k.user == key.user).count();
                if per_user >= self.max_per_user {
                    return Err(OpenError::PerUserLimit(self.max_per_user));
                }
            }
        }
        let entry = map.entry(key.clone()).or_insert_with(|| Entry {
            session: Arc::new(tokio::sync::Mutex::new(self.backend.session_with(key))),
            last_used: std::time::Instant::now(),
        });
        entry.last_used = std::time::Instant::now();
        Ok(entry.session.clone())
    }

    /// Reset a session's idle timer (the wire `Heartbeat`), keeping a long-lived but
    /// quiet session warm. Returns whether the session was live.
    pub fn touch(&self, key: &agent_core::SessionKey) -> bool {
        let mut map = self.sessions.lock().expect("session map poisoned");
        match map.get_mut(key) {
            Some(e) => {
                e.last_used = std::time::Instant::now();
                true
            }
            None => false,
        }
    }

    /// Drop a finished session, freeing its state. Absent key is a no-op. Also
    /// retires the session's event sink from the backend registry, so a dead session
    /// stops being observable and its broadcast channel is freed (hazard B eviction),
    /// and retires its per-tenant gauge series (06-observability).
    pub fn remove(&self, key: &agent_core::SessionKey) {
        self.sessions
            .lock()
            .expect("session map poisoned")
            .remove(key);
        self.backend.events.remove(key.session.as_str());
        // Retire this session's per-tenant gauge series, so Prometheus doesn't retain a
        // dead session's `agent_active = 1` (docs/design/multi-session/06-observability.md).
        self.backend
            .metrics
            .for_session(key.session.as_str(), key.user.as_str())
            .retire();
    }

    /// Reap sessions idle for at least `idle_after`, freeing their state — the real
    /// resource-use guarantee, since `Close` is best-effort and a crashed client never
    /// sends it (docs/design/multi-session/05-lifecycle.md). A session **currently
    /// mid-turn** (its handle is locked) is skipped, so a long turn that hasn't touched
    /// `last_used` is never reaped out from under itself. Returns how many were reaped.
    pub fn reap_idle(&self, idle_after: std::time::Duration) -> usize {
        let now = std::time::Instant::now();
        let stale: Vec<agent_core::SessionKey> = {
            let map = self.sessions.lock().expect("session map poisoned");
            map.iter()
                .filter(|(_, e)| now.duration_since(e.last_used) >= idle_after)
                // `try_lock` Ok ⇒ no turn in progress ⇒ safe to reap.
                .filter(|(_, e)| e.session.try_lock().is_ok())
                .map(|(k, _)| k.clone())
                .collect()
        };
        for k in &stale {
            self.remove(k);
        }
        stale.len()
    }

    /// Number of live sessions.
    pub fn len(&self) -> usize {
        self.sessions.lock().expect("session map poisoned").len()
    }

    /// Whether no sessions are live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The wire lifecycle seam over the live-session map (docs/design/multi-session/05):
/// `open` **server-mints** the session id (an unguessable `Uuid`, removing client-chosen
/// id collision/prediction), `close` frees state, `heartbeat` keeps a quiet session warm.
#[async_trait::async_trait]
impl agent_core::SessionRegistry for SessionManager {
    async fn open(&self, user: &str) -> agent_core::Result<agent_core::SessionId> {
        // Mint the id server-side, then admit under the capacity caps. The minted id is
        // always a valid segment; `user` is validated by `SessionKey::parse`.
        let session = uuid::Uuid::new_v4().to_string();
        let key = agent_core::SessionKey::parse(user, &session)
            .map_err(|e| agent_core::Error::Config(format!("invalid user: {e}")))?;
        // Capacity rejection → `Overloaded` (maps to gRPC RESOURCE_EXHAUSTED).
        self.admit(key)
            .map_err(|e| agent_core::Error::Overloaded(e.to_string()))?;
        Ok(agent_core::SessionId::new(session))
    }

    async fn close(&self, key: &agent_core::SessionKey) -> agent_core::Result<()> {
        // Best-effort: an absent session is not an error (idle-GC may have reaped it).
        self.remove(key);
        Ok(())
    }

    async fn heartbeat(&self, key: &agent_core::SessionKey) -> agent_core::Result<()> {
        if self.touch(key) {
            Ok(())
        } else {
            Err(agent_core::Error::Config(format!(
                "no live session `{}` for `{}` (reaped? re-open)",
                key.session, key.user
            )))
        }
    }
}
