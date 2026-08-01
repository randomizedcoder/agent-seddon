//! Multi-session pool. `SessionManager` owns the shared `Agent` backend and a map of
//! live `Session`s keyed by `(user, session)`, admitting/reaping them under capacity
//! caps so one process serves many concurrent sessions. Extracted from `agent.rs`;
//! `OpenError` + `SessionManager` are re-exported through the `agent` module, so
//! `agent_runtime::{OpenError, SessionManager}` is unchanged.
//!
//! Each live session is an **actor**: a spawned task that *owns* its [`Session`] and
//! processes [`SessionCommand`]s one at a time — the mpsc receiver being drained
//! serially is the turn serializer (it replaces the per-session async mutex). A
//! [`SessionHandle`] is a cheap, cloneable command sender. Driving a goal is a `Run`
//! command whose cancel channel, dropped by a disconnecting client, aborts the
//! in-flight run at its next `.await` (future-drop cancellation — the same model
//! `main.rs` uses for ctrl-c). This is the foundation for the portal's `Send` RPC.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// Owns the shared [`Agent`] backend and a map of live [`Session`]s keyed by
/// `(user, session)`, so one process serves many concurrent sessions. Turns *within*
/// a session are serialized by its actor task draining commands one at a time;
/// *distinct* sessions run in parallel over the shared backend, which is `Send + Sync`
/// and mostly functional (docs/design/multi-session/02-runtime-split.md).
///
/// The single-session CLI/REPL can ignore this and call [`Agent::session`] directly;
/// the multi-tenant (portal) path drives sessions through [`Self::get_or_create`].
/// Rejected [`SessionManager::admit`] — a *new* session would exceed a capacity cap.
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

/// A command to a session actor. `Run` is the only variant today; the enum leaves room
/// for future `Compact`/`Load`/`Messages` queries without changing the actor shape.
enum SessionCommand {
    /// Run `goal` to the next final answer. `cancel` firing (sender sent or **dropped**)
    /// aborts the in-flight run at its next `.await`; the result is delivered on `done`
    /// (ignored by the fire-and-forget `Send` path, which observes via the event sink).
    Run {
        goal: String,
        cancel: oneshot::Receiver<()>,
        done: oneshot::Sender<anyhow::Result<String>>,
    },
}

/// State shared between the map side (reap) and the actor task.
#[derive(Default)]
struct SessionShared {
    /// True while a `Run` is executing — the mutex-free "skip mid-turn" for `reap_idle`
    /// (replaces the old `try_lock` probe).
    busy: AtomicBool,
}

/// One live session: the actor's command channel + its task + idle-GC bookkeeping.
struct Entry {
    tx: mpsc::Sender<SessionCommand>,
    task: tokio::task::JoinHandle<()>,
    /// Wall-clock of the last dispatch/heartbeat, for idle reaping (map-side, bumped by
    /// `get_or_create`/`admit`/`touch` under the map lock — exactly the old events).
    last_used: std::time::Instant,
    shared: Arc<SessionShared>,
}

/// A cheap, cloneable handle to one session's actor. Submitting a goal is a command;
/// the actor serializes turns, so two `run`s on the same handle never overlap.
#[derive(Clone)]
pub struct SessionHandle {
    key: agent_core::SessionKey,
    tx: mpsc::Sender<SessionCommand>,
    shared: Arc<SessionShared>,
}

impl SessionHandle {
    /// The `(user, session)` identity this session was created for — equal to the owned
    /// `Session`'s `id` (the manager builds it with `session_with(key)`).
    pub fn key(&self) -> &agent_core::SessionKey {
        &self.key
    }

    /// Whether two handles point at the *same* live session (identity, not equality).
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// Submit `goal` and await the run's result — the direct turn-taking path (CLI/tests).
    /// The cancel channel is held for the whole run, so this never self-cancels; the
    /// fire-and-forget cancellable path is [`Self::start`] (used by the `Send` RPC).
    pub async fn run(&self, goal: &str) -> anyhow::Result<String> {
        // `_cancel_tx` lives until this fn returns, so `cancel` never fires during the run.
        let (_cancel_tx, cancel) = oneshot::channel();
        let (done, done_rx) = oneshot::channel();
        self.tx
            .send(SessionCommand::Run {
                goal: goal.to_string(),
                cancel,
                done,
            })
            .await
            .map_err(|_| anyhow::anyhow!("session actor is gone"))?;
        done_rx
            .await
            .map_err(|_| anyhow::anyhow!("session actor dropped the run"))?
    }
}

pub struct SessionManager {
    backend: Arc<Agent>,
    sessions: std::sync::Mutex<std::collections::HashMap<agent_core::SessionKey, Entry>>,
    /// Global live-session cap (`0` = unbounded). See [`Self::admit`].
    max_total: usize,
    /// Per-user live-session cap (`0` = unbounded).
    max_per_user: usize,
}

/// The actor loop: own `session`, drain commands one at a time (serializing turns),
/// and end when the last [`SessionHandle`]/`Entry` is dropped (the channel closes) or
/// the task is aborted by [`SessionManager::remove`].
async fn run_actor(
    mut session: Session,
    mut rx: mpsc::Receiver<SessionCommand>,
    shared: Arc<SessionShared>,
) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            SessionCommand::Run { goal, cancel, done } => {
                shared.busy.store(true, Ordering::SeqCst);
                // Future-drop cancellation: `cancel` resolving (client disconnect drops the
                // sender) drops `session.send` at its next `.await`; the owned `Session`'s
                // working set survives for the next Run.
                let result = tokio::select! {
                    r = session.send(&goal) => r,
                    _ = cancel => Err(anyhow::anyhow!("run cancelled (client disconnected)")),
                };
                shared.busy.store(false, Ordering::SeqCst);
                let _ = done.send(result);
            }
        }
    }
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
    /// `max_per_user` per user (`0` = unbounded). Only [`Self::admit`] enforces them;
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

    /// Spawn a session actor for `key` and build its `Entry` (map lock held by the
    /// caller; `session_with`/`spawn` are synchronous, never `.await`).
    fn spawn_entry(&self, key: agent_core::SessionKey) -> (Entry, SessionHandle) {
        let (tx, rx) = mpsc::channel(32);
        let shared = Arc::new(SessionShared::default());
        let session = self.backend.session_with(key.clone());
        let task = tokio::spawn(run_actor(session, rx, shared.clone()));
        let entry = Entry {
            tx: tx.clone(),
            task,
            last_used: std::time::Instant::now(),
            shared: shared.clone(),
        };
        let handle = SessionHandle { key, tx, shared };
        (entry, handle)
    }

    /// Look up the session for `key`, creating it (and its actor) on first use, and mark
    /// it used. The map lock is held only for the lookup/insert, never across `.await`,
    /// so distinct sessions never contend.
    ///
    /// This is the **lazy correctness path** and never rejects: in a split deployment a
    /// seam may receive its first request for a session another process `Open`ed, and
    /// must allocate on demand (docs/design/multi-session/05-lifecycle.md). Capacity is
    /// enforced only at the trusted entry point, [`Self::admit`].
    pub fn get_or_create(&self, key: agent_core::SessionKey) -> SessionHandle {
        let mut map = self.sessions.lock().expect("session map poisoned");
        if let Some(entry) = map.get_mut(&key) {
            entry.last_used = std::time::Instant::now();
            return SessionHandle {
                key,
                tx: entry.tx.clone(),
                shared: entry.shared.clone(),
            };
        }
        let (entry, handle) = self.spawn_entry(key.clone());
        map.insert(key, entry);
        handle
    }

    /// Capacity-checked admission (backs the wire `SessionRegistry.Open`): return the
    /// session for `key`, but if it does **not** already exist, reject when creating it
    /// would exceed the total or per-user cap. An existing session is always returned
    /// (and touched), so a legitimate client re-entering its own session is never
    /// throttled — only a hostile *new*-id spray is (docs/design/multi-session/05).
    #[allow(clippy::result_large_err)]
    pub fn admit(&self, key: agent_core::SessionKey) -> Result<SessionHandle, OpenError> {
        let mut map = self.sessions.lock().expect("session map poisoned");
        if let Some(entry) = map.get_mut(&key) {
            entry.last_used = std::time::Instant::now();
            return Ok(SessionHandle {
                key,
                tx: entry.tx.clone(),
                shared: entry.shared.clone(),
            });
        }
        if self.max_total > 0 && map.len() >= self.max_total {
            return Err(OpenError::TotalLimit(self.max_total));
        }
        if self.max_per_user > 0 {
            let per_user = map.keys().filter(|k| k.user == key.user).count();
            if per_user >= self.max_per_user {
                return Err(OpenError::PerUserLimit(self.max_per_user));
            }
        }
        let (entry, handle) = self.spawn_entry(key.clone());
        map.insert(key, entry);
        Ok(handle)
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

    /// Drop a finished session, freeing its state. Absent key is a no-op. Aborts the
    /// actor task (stopping even a mid-run turn), then retires the session's event sink
    /// from the backend registry (so a dead session stops being observable and its
    /// broadcast channel is freed — hazard B eviction) and its per-tenant gauge series.
    pub fn remove(&self, key: &agent_core::SessionKey) {
        let entry = self
            .sessions
            .lock()
            .expect("session map poisoned")
            .remove(key);
        if let Some(e) = entry {
            // Stop the actor even if it is mid-turn; dropping `tx` alone would not end a
            // busy actor until its current run finished.
            e.task.abort();
        }
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
    /// mid-turn** (its `busy` flag set) is skipped, so a long turn that hasn't touched
    /// `last_used` is never reaped out from under itself. Returns how many were reaped.
    pub fn reap_idle(&self, idle_after: std::time::Duration) -> usize {
        let now = std::time::Instant::now();
        let stale: Vec<agent_core::SessionKey> = {
            let map = self.sessions.lock().expect("session map poisoned");
            map.iter()
                .filter(|(_, e)| now.duration_since(e.last_used) >= idle_after)
                // Not mid-turn ⇒ safe to reap (the mutex-free replacement for `try_lock`).
                .filter(|(_, e)| !e.shared.busy.load(Ordering::SeqCst))
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
