//! Per-user tenant routing for the file-backed memory + dimension stores
//! (docs/design/multi-session/04-tenancy.md).
//!
//! The loop shares one store `Arc` across every session; `session_id` is merely a
//! column on `MemoryEvent`, so today two users' memories interleave and — worse —
//! `recall` returns another tenant's facts. **The path is the boundary.** These
//! wrappers resolve the ambient `(user, session)` identity (the `AGENT_IDENTITY`
//! task-local, scoped per turn by the runtime) on **each call** and delegate to a
//! per-user store rooted at `<base>/<user>/…`. A user's memory therefore accumulates
//! across *their own* sessions (cross-session learning, the locked tenancy model) but
//! is invisible to other users.
//!
//! The default `local` user maps to the **un-namespaced** base path, so the
//! single-user CLI is byte-identical to before this change — no silent relocation of
//! an existing `.agent/episodic.jsonl`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent_core::{current_identity, safe_segment, LlmProvider, UserId};

/// Root `base` under `user`: insert the user segment as the **parent of `base`'s
/// final component**, giving the design's `<root>/<user>/<leaf>` layout
/// (`.agent/episodic.jsonl` → `.agent/<user>/episodic.jsonl`; `.agent/memory` →
/// `.agent/<user>/memory`).
///
/// The default `local` user maps to `base` unchanged (the single-user path stays
/// byte-identical). `user` is `safe_segment`-validated upstream (it is
/// attacker-controlled wire identity) and re-checked here as defense in depth — a
/// segment that somehow fails validation falls back to the base (the **default**
/// tenant), never escaping via `..`/separators into or out of another user's tree.
fn tenant_path(base: &Path, user: &str) -> PathBuf {
    if user == UserId::LOCAL || !safe_segment(user) {
        return base.to_path_buf();
    }
    match (base.parent(), base.file_name()) {
        (Some(parent), Some(name)) => parent.join(user).join(name),
        // A bare relative name with no parent (unusual): namespace it directly.
        _ => Path::new(user).join(base),
    }
}

/// The current turn's user segment, or `local` when no identity is scoped (the
/// single-user CLI, or an unauthenticated served call — it lands in the default
/// tenant, never another user's namespace).
fn current_user() -> String {
    current_identity()
        .map(|k| k.user.as_str().to_string())
        .unwrap_or_else(|| UserId::LOCAL.to_string())
}

/// A [`MemoryStore`](agent_core::MemoryStore) that routes each call to a per-user
/// file store rooted at `<base>/<user>/…`. Per-user stores are built lazily on first
/// touch and cached, mirroring the runtime's lazy session/handle maps.
#[cfg(feature = "memory-file")]
pub struct PerUserMemory {
    episodic_path: PathBuf,
    semantic_dir: PathBuf,
    provider: Option<Arc<dyn LlmProvider>>,
    cache: Mutex<HashMap<String, Arc<dyn agent_core::MemoryStore>>>,
}

#[cfg(feature = "memory-file")]
impl PerUserMemory {
    /// Wrap the configured base paths; `provider` (when `Some`) enables distillation
    /// on each per-user store, exactly as [`crate::file_memory`].
    pub fn new(
        episodic_path: impl Into<PathBuf>,
        semantic_dir: impl Into<PathBuf>,
        provider: Option<Arc<dyn LlmProvider>>,
    ) -> Self {
        Self {
            episodic_path: episodic_path.into(),
            semantic_dir: semantic_dir.into(),
            provider,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn store_for(&self, user: &str) -> Arc<dyn agent_core::MemoryStore> {
        let mut cache = self.cache.lock().expect("tenant memory cache poisoned");
        if let Some(s) = cache.get(user) {
            return s.clone();
        }
        let ep = tenant_path(&self.episodic_path, user);
        let sem = tenant_path(&self.semantic_dir, user);
        let store: Arc<dyn agent_core::MemoryStore> =
            Arc::new(crate::file_memory(ep, sem, self.provider.clone()));
        cache.insert(user.to_string(), store.clone());
        store
    }

    /// The store for the current turn's tenant. The map lock is released before any
    /// `.await`, so distinct tenants never contend on the store call itself.
    fn current(&self) -> Arc<dyn agent_core::MemoryStore> {
        self.store_for(&current_user())
    }
}

#[cfg(feature = "memory-file")]
#[async_trait::async_trait]
impl agent_core::MemoryStore for PerUserMemory {
    async fn recall(
        &self,
        query: &agent_core::RecallQuery,
    ) -> agent_core::Result<Vec<agent_core::MemoryItem>> {
        self.current().recall(query).await
    }
    async fn append(&self, event: agent_core::MemoryEvent) -> agent_core::Result<()> {
        self.current().append(event).await
    }
    async fn distill(&self) -> agent_core::Result<usize> {
        self.current().distill().await
    }
}

/// A [`DimensionStore`](agent_core::DimensionStore) routed per-user, rooted at
/// `<semantic_dir>/<user>/dimensions` — under the same per-user semantic root as
/// [`PerUserMemory`]'s semantic layer, so a user's dimensional histories are
/// cross-session but isolated across users.
#[cfg(feature = "memory-dimensions")]
pub struct PerUserDimensions {
    semantic_dir: PathBuf,
    provider: Option<Arc<dyn LlmProvider>>,
    cache: Mutex<HashMap<String, Arc<dyn agent_core::DimensionStore>>>,
}

#[cfg(feature = "memory-dimensions")]
impl PerUserDimensions {
    /// `semantic_dir` is the memory root whose `<user>/dimensions/` subtree each
    /// tenant's histories live under (matching the inline build's
    /// `<semantic_dir>/dimensions`).
    pub fn new(semantic_dir: impl Into<PathBuf>, provider: Option<Arc<dyn LlmProvider>>) -> Self {
        Self {
            semantic_dir: semantic_dir.into(),
            provider,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn store_for(&self, user: &str) -> Arc<dyn agent_core::DimensionStore> {
        let mut cache = self.cache.lock().expect("tenant dimensions cache poisoned");
        if let Some(s) = cache.get(user) {
            return s.clone();
        }
        let dir = tenant_path(&self.semantic_dir, user).join("dimensions");
        let store: Arc<dyn agent_core::DimensionStore> =
            Arc::new(crate::file_dimensions(dir, self.provider.clone()));
        cache.insert(user.to_string(), store.clone());
        store
    }

    fn current(&self) -> Arc<dyn agent_core::DimensionStore> {
        self.store_for(&current_user())
    }
}

#[cfg(feature = "memory-dimensions")]
#[async_trait::async_trait]
impl agent_core::DimensionStore for PerUserDimensions {
    async fn summarize_step(
        &self,
        events: &[agent_core::MemoryEvent],
    ) -> agent_core::Result<Vec<agent_core::DimensionSummary>> {
        self.current().summarize_step(events).await
    }
    async fn recall_dimension(
        &self,
        dimension: &str,
        limit: usize,
    ) -> agent_core::Result<Vec<agent_core::MemoryItem>> {
        self.current().recall_dimension(dimension, limit).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- tenant_path: the on-disk boundary ---------------------------------

    #[test]
    fn positive_local_user_maps_to_base_path_unchanged() {
        // Back-compat: the default `local` tenant must not relocate existing files.
        let ep = Path::new(".agent/episodic.jsonl");
        assert_eq!(tenant_path(ep, UserId::LOCAL), ep);
        let sem = Path::new(".agent/memory");
        assert_eq!(tenant_path(sem, UserId::LOCAL), sem);
    }

    #[test]
    fn positive_named_user_is_inserted_before_final_component() {
        assert_eq!(
            tenant_path(Path::new(".agent/episodic.jsonl"), "alice"),
            Path::new(".agent/alice/episodic.jsonl"),
        );
        assert_eq!(
            tenant_path(Path::new(".agent/memory"), "bob"),
            Path::new(".agent/bob/memory"),
        );
    }

    #[test]
    fn positive_distinct_users_get_distinct_roots() {
        let base = Path::new("/data/.agent/memory");
        assert_ne!(tenant_path(base, "alice"), tenant_path(base, "bob"));
    }

    #[test]
    fn corner_bare_name_without_parent_is_namespaced() {
        assert_eq!(
            tenant_path(Path::new("episodic.jsonl"), "alice"),
            Path::new("alice/episodic.jsonl"),
        );
    }

    // A spoofed user segment must never escape into a sibling/parent tree; an
    // invalid segment falls back to the base (the default tenant), never `..`.
    #[rstest::rstest]
    #[case::traversal("../../etc")]
    #[case::separator("a/b")]
    #[case::leading_dash("-rf")]
    #[case::dotdot("..")]
    fn adversarial_malformed_user_falls_back_to_base_never_escapes(#[case] bad: &str) {
        let base = Path::new(".agent/memory");
        let got = tenant_path(base, bad);
        assert_eq!(got, base, "malformed user {bad:?} must not re-root");
        // And crucially never contains the injected traversal.
        assert!(!got.to_string_lossy().contains(bad));
    }

    // --- end-to-end routing: the path *is* the isolation boundary -----------
    #[cfg(feature = "memory-file")]
    mod routing {
        use super::*;
        use agent_core::{
            scope, ContentBlock, EpisodicStore, MemoryEvent, MemoryStore, Message, Role, SessionKey,
        };
        use agent_testkit::tempdir;

        fn ev(text: &str) -> MemoryEvent {
            MemoryEvent {
                kind: "goal".into(),
                message: Message {
                    role: Role::User,
                    content: vec![ContentBlock::text(text)],
                    tool_calls: vec![],
                    tool_call_id: None,
                },
                ts_ms: 0,
                session_id: String::new(),
                usage: None,
                iter: None,
                verification: None,
                review: None,
                dimensional: None,
            }
        }

        async fn append_as(mem: &PerUserMemory, user: &str, session: &str, text: &str) {
            let key = SessionKey::parse(user, session).unwrap();
            scope(key, async { mem.append(ev(text)).await.unwrap() }).await;
        }

        /// Read a per-user episodic log back **through the store's own tokio reader**
        /// (`FileEpisodic::recent`). Reading via `std::fs` instead would race the
        /// `tokio::fs` write's background flush (`write_all` returns before the bytes
        /// are durable) and flake.
        async fn events_at(path: &Path) -> Vec<MemoryEvent> {
            crate::FileEpisodic::new(path)
                .recent(1000)
                .await
                .expect("read back episodic log")
        }

        fn texts(events: &[MemoryEvent]) -> String {
            events
                .iter()
                .map(|e| e.message.content_text())
                .collect::<Vec<_>>()
                .join("|")
        }

        #[tokio::test]
        async fn adversarial_users_are_isolated_and_a_user_shares_across_sessions() {
            let root = tempdir();
            let ep = root.join(".agent").join("episodic.jsonl");
            let sem = root.join(".agent").join("memory");
            let mem = PerUserMemory::new(&ep, &sem, None);

            // Alice writes from two different sessions; Bob writes from one.
            append_as(&mem, "alice", "s1", "alice-one").await;
            append_as(&mem, "alice", "s2", "alice-two").await;
            append_as(&mem, "bob", "s1", "bob-only").await;

            let alice_log = root.join(".agent").join("alice").join("episodic.jsonl");
            let bob_log = root.join(".agent").join("bob").join("episodic.jsonl");
            assert_ne!(alice_log, bob_log);

            let alice = events_at(&alice_log).await;
            let bob = events_at(&bob_log).await;

            // Cross-session within a user accumulates into ONE per-user log…
            assert_eq!(alice.len(), 2, "alice's two sessions share a log");
            // …while a different user lands in a DISTINCT file (the isolation boundary).
            assert_eq!(bob.len(), 1, "bob has his own log");
            // No cross-tenant bleed, in either direction.
            assert!(
                !texts(&bob).contains("alice"),
                "alice's memory absent from bob's log"
            );
            assert!(
                !texts(&alice).contains("bob"),
                "bob's memory absent from alice's log"
            );
        }

        #[tokio::test]
        async fn positive_local_user_writes_the_unnamespaced_base_path() {
            // Back-compat: the default `local` tenant writes exactly today's path.
            let root = tempdir();
            let ep = root.join(".agent").join("episodic.jsonl");
            let sem = root.join(".agent").join("memory");
            let mem = PerUserMemory::new(&ep, &sem, None);

            scope(SessionKey::local("s1"), async {
                mem.append(ev("local-one")).await.unwrap()
            })
            .await;

            assert_eq!(
                events_at(&ep).await.len(),
                1,
                "local writes the base episodic path"
            );
            assert!(
                !root.join(".agent").join("local").exists(),
                "the `local` user is NOT namespaced into a `local/` subdir"
            );
        }

        #[tokio::test]
        async fn corner_no_identity_scope_defaults_to_local_base() {
            // An unscoped call (no ambient identity) lands in the default tenant.
            let root = tempdir();
            let ep = root.join(".agent").join("episodic.jsonl");
            let sem = root.join(".agent").join("memory");
            let mem = PerUserMemory::new(&ep, &sem, None);

            mem.append(ev("unscoped")).await.unwrap();
            assert_eq!(events_at(&ep).await.len(), 1);
        }
    }
}
