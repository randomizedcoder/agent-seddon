//! [`FleetPostLease`] seam implementations (review-fleet C17): the durable,
//! read-your-writes, cross-process compare-and-set that makes approve→post
//! **idempotent** on `review_id`.
//!
//! The forge post is the only irreversible side effect of an approve, and the fleet's
//! draft `status` is persisted through an async, eventually-consistent telemetry funnel —
//! so it can't be the idempotency key (a double comment slips through when two approves
//! race, or a re-approve lands inside the flush window). The lease is the authoritative
//! guard: exactly one `acquire` wins (`Acquired`), the rest observe `Held`/`AlreadyPosted`
//! and never post.
//!
//! Two backends:
//! - [`MemoryPostLease`] — in-process (a mutex-guarded map). Atomic **within one process**
//!   (closes the concurrent + sequential-within-process double-post), the default and the
//!   test double. It does **not** survive a restart or coordinate across processes — a
//!   fleet that needs that must run a durable backend.
//! - [`StorePostLease`] (feature `fleet-store`) — the durable, cross-process lease over the
//!   shared transactional config store ([`agent_config_store::Backend`]: memory/file/sqlite/
//!   **postgres**), the lease twin of [`crate::StoreFleet`]. It replaces the retired
//!   `SqlitePostLease`: the claim is one atomic [`agent_config_store::Write::CompareAndSwap`]
//!   (land `held` only if the row is absent), so exactly one racer — across threads *and*
//!   processes sharing the backend — observes `Acquired`. A `postgres` fleet finally gets
//!   cross-process/restart-durable dedup, not just the old embedded-SQLite tier.
//!
//! `review_id` is untrusted wire input. [`MemoryPostLease`] keys it into an opaque in-process
//! map (never a path). [`StorePostLease`] makes it a card **id**, so it is `safe_segment`-gated
//! and reaches the backend only as a bound parameter — it can neither traverse nor inject.
//! (Review ids are server-minted UUIDs, so a well-formed id always passes.)

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use agent_core::{FleetPostLease, PostLease, Result};
use async_trait::async_trait;

/// Cap on the number of terminal (`posted`) leases the in-memory backend retains for
/// dedup. Bounds memory against a long-lived process that posts many reviews; the oldest
/// terminal leases are evicted first (a re-approve of a very old, evicted review could
/// re-post — acceptable for the non-durable tier, whose dedup is best-effort by design).
/// In-flight (`held`) leases are never counted here and never evicted.
pub const MAX_POSTED_LEASES: usize = 100_000;

#[derive(Default)]
struct MemState {
    /// `review_id → committed?` — present-and-`false` is *held* (a post in flight),
    /// present-and-`true` is *posted* (terminal). Absent is *none*.
    states: HashMap<String, bool>,
    /// Insertion order of *posted* keys only, for bounded FIFO eviction.
    posted_order: VecDeque<String>,
}

/// The in-process [`FleetPostLease`]: one lease table behind a mutex. Atomic within a
/// single process; not durable across restarts and not shared across processes.
#[derive(Default)]
pub struct MemoryPostLease {
    inner: Mutex<MemState>,
    cap: usize,
}

impl MemoryPostLease {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MemState::default()),
            cap: MAX_POSTED_LEASES,
        }
    }

    /// A backend with an explicit posted-lease cap (0 ⇒ the default). Test-facing.
    pub fn with_cap(cap: usize) -> Self {
        Self {
            inner: Mutex::new(MemState::default()),
            cap: if cap == 0 { MAX_POSTED_LEASES } else { cap },
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemState> {
        // A poisoned lock means a panic mid-mutation; each mutation leaves the map in a
        // consistent state, so recovering the inner value is safe.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl FleetPostLease for MemoryPostLease {
    async fn acquire(&self, review_id: &str) -> Result<PostLease> {
        let mut g = self.lock();
        match g.states.get(review_id) {
            None => {
                g.states.insert(review_id.to_string(), false);
                Ok(PostLease::Acquired)
            }
            Some(false) => Ok(PostLease::Held),
            Some(true) => Ok(PostLease::AlreadyPosted),
        }
    }

    async fn commit(&self, review_id: &str) -> Result<()> {
        let mut g = self.lock();
        // Only flip a held/absent lease to posted once; a second commit is a no-op that
        // must not re-enqueue the key (which would corrupt the eviction bookkeeping).
        let newly_posted = !matches!(g.states.get(review_id), Some(true));
        g.states.insert(review_id.to_string(), true);
        if newly_posted {
            g.posted_order.push_back(review_id.to_string());
            while g.posted_order.len() > self.cap {
                if let Some(old) = g.posted_order.pop_front() {
                    // Evict only if still posted (a released+re-held key could reappear;
                    // don't drop an in-flight hold).
                    if matches!(g.states.get(&old), Some(true)) {
                        g.states.remove(&old);
                    }
                }
            }
        }
        Ok(())
    }

    async fn release(&self, review_id: &str) -> Result<()> {
        let mut g = self.lock();
        // Release only a *held* (un-posted) lease, so a failed post can be retried; never
        // undo a committed post.
        if matches!(g.states.get(review_id), Some(false)) {
            g.states.remove(review_id);
        }
        Ok(())
    }
}

#[cfg(feature = "fleet-store")]
pub use store_lease::StorePostLease;

#[cfg(feature = "fleet-store")]
mod store_lease {
    use std::sync::Arc;

    use agent_config_store::{is_conflict, Backend, Write};
    use agent_core::{safe_segment, Error, FleetPostLease, PostLease, Result};
    use async_trait::async_trait;

    /// One card per review's post lease; the id is the (server-minted) review id.
    const POST_LEASE: &str = "post_lease";
    /// The default single-tenant scope (mirrors [`crate::StoreFleet`]'s).
    const DEFAULT_TENANT: &str = "local";
    /// Lease states, stored as the opaque card blob: `held` = a post in flight,
    /// `posted` = terminal (the forge comment landed). An absent card is *none*.
    const HELD: &[u8] = b"held";
    const POSTED: &[u8] = b"posted";

    /// The durable, cross-process [`FleetPostLease`] over a shared [`Backend`] — the
    /// lease twin of [`crate::StoreFleet`]. Cheap to clone (an `Arc` handle plus the
    /// tenant key).
    pub struct StorePostLease {
        backend: Arc<dyn Backend>,
        tenant: String,
    }

    impl StorePostLease {
        /// A lease over `backend` under the default single-tenant scope.
        pub fn new(backend: Arc<dyn Backend>) -> Self {
            Self {
                backend,
                tenant: DEFAULT_TENANT.to_string(),
            }
        }

        /// A lease scoped to an explicit tenant. The tenant is `safe_segment`-gated —
        /// a hostile tenant is rejected at construction, never persisted or keyed.
        pub fn with_tenant(backend: Arc<dyn Backend>, tenant: &str) -> Result<Self> {
            if !safe_segment(tenant) {
                return Err(Error::Fleet(format!("invalid tenant `{tenant}`")));
            }
            Ok(Self {
                backend,
                tenant: tenant.to_string(),
            })
        }

        /// `review_id` becomes a card id, so — unlike the opaque-map
        /// [`MemoryPostLease`] — it must pass [`safe_segment`]. We fail closed early
        /// with a clear error (the backend would reject a hostile segment anyway).
        fn check(&self, review_id: &str) -> Result<()> {
            if safe_segment(review_id) {
                Ok(())
            } else {
                Err(Error::Fleet("invalid review id".into()))
            }
        }
    }

    /// The config-store seam speaks `Error::Config`; re-tag non-precondition failures
    /// as `Error::Fleet` for the lease seam's callers.
    fn map_err(e: Error) -> Error {
        Error::Fleet(format!("post-lease store: {e}"))
    }

    #[async_trait]
    impl FleetPostLease for StorePostLease {
        async fn acquire(&self, review_id: &str) -> Result<PostLease> {
            self.check(review_id)?;
            // Atomic claim: land `held` ONLY if the card is currently absent. Exactly
            // one racer's CAS precondition (`expected = None`) holds — across threads
            // AND processes sharing the backend — so exactly one sees `Acquired`.
            let claim = [
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::CompareAndSwap {
                    collection: POST_LEASE,
                    tenant: self.tenant.clone(),
                    id: review_id.to_string(),
                    expected: None,
                    blob: HELD.to_vec(),
                },
            ];
            match self.backend.apply(&claim).await {
                Ok(()) => Ok(PostLease::Acquired),
                // Lost the claim — someone holds or already posted. Read the row to
                // classify. This read is NOT atomic with the failed CAS, but lease
                // states are monotonic (none→held→posted; held→none only via the
                // holder's own release), so the worst case is a racing release between
                // the CAS and this read → we read *none* and report `Held` (never
                // `Acquired`), a benign over-report that never double-posts.
                Err(e) if is_conflict(&e) => {
                    match self
                        .backend
                        .get(POST_LEASE, &self.tenant, review_id)
                        .await?
                    {
                        Some(b) if b.as_slice() == POSTED => Ok(PostLease::AlreadyPosted),
                        _ => Ok(PostLease::Held),
                    }
                }
                Err(e) => Err(map_err(e)),
            }
        }

        async fn commit(&self, review_id: &str) -> Result<()> {
            self.check(review_id)?;
            // Terminal: upsert `posted` unconditionally, so a held→posted flip is
            // durable even if the `held` row was lost, and a re-commit is a no-op.
            let writes = [
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::Put {
                    collection: POST_LEASE,
                    tenant: self.tenant.clone(),
                    id: review_id.to_string(),
                    blob: POSTED.to_vec(),
                },
            ];
            self.backend.apply(&writes).await.map_err(map_err)
        }

        async fn release(&self, review_id: &str) -> Result<()> {
            self.check(review_id)?;
            // Release ONLY a still-held lease (a failed post can retry); never undo a
            // committed post. The CAS precondition (`expected = held`) makes the
            // delete-if-held atomic under the backend's transaction: if the row is
            // `posted` or absent the CAS conflicts and the whole batch — the `Delete`
            // included — is rejected, so a posted lease is never removed.
            let writes = [
                Write::EnsureTenant {
                    tenant: self.tenant.clone(),
                },
                Write::CompareAndSwap {
                    collection: POST_LEASE,
                    tenant: self.tenant.clone(),
                    id: review_id.to_string(),
                    expected: Some(HELD.to_vec()),
                    blob: HELD.to_vec(),
                },
                Write::Delete {
                    collection: POST_LEASE,
                    tenant: self.tenant.clone(),
                    id: review_id.to_string(),
                },
            ];
            match self.backend.apply(&writes).await {
                Ok(()) => Ok(()),
                // Not held (posted or already released) → nothing to release, a no-op.
                Err(e) if is_conflict(&e) => Ok(()),
                Err(e) => Err(map_err(e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // MemoryPostLease and (when built) StorePostLease must satisfy the SAME contract, so
    // the behavioural tests run over each via a small factory list. StorePostLease is
    // exercised over an in-process `MemoryBackend` (the hermetic proof of the shared-store
    // path); the real Postgres backend is covered by the `#[ignore]` `pg_lease_tests`.
    // Every contract case uses a `safe_segment` id (all backends accept it); the hostile-id
    // divergence between the two tiers is asserted in its own tests below.
    fn backends() -> Vec<(&'static str, Box<dyn FleetPostLease>)> {
        #[cfg_attr(not(feature = "fleet-store"), allow(unused_mut))]
        let mut v: Vec<(&'static str, Box<dyn FleetPostLease>)> =
            vec![("memory", Box::new(MemoryPostLease::new()))];
        #[cfg(feature = "fleet-store")]
        v.push((
            "store",
            Box::new(StorePostLease::new(std::sync::Arc::new(
                agent_config_store::MemoryBackend::new(),
            ))),
        ));
        v
    }

    #[tokio::test]
    async fn positive_first_acquire_wins_repeat_is_already_posted() {
        for (name, lease) in backends() {
            assert_eq!(
                lease.acquire("rev-1").await.unwrap(),
                PostLease::Acquired,
                "{name}: first acquire wins"
            );
            lease.commit("rev-1").await.unwrap();
            // A sequential re-approve (the read-after-write case) sees posted, never re-posts.
            assert_eq!(
                lease.acquire("rev-1").await.unwrap(),
                PostLease::AlreadyPosted,
                "{name}: committed lease dedups"
            );
        }
    }

    #[tokio::test]
    async fn negative_second_acquire_before_commit_is_held() {
        for (name, lease) in backends() {
            assert_eq!(lease.acquire("rev-2").await.unwrap(), PostLease::Acquired);
            // A concurrent approve mid-post must be told to stand down, never Acquired.
            assert_eq!(
                lease.acquire("rev-2").await.unwrap(),
                PostLease::Held,
                "{name}: a held lease blocks a second poster"
            );
        }
    }

    #[tokio::test]
    async fn corner_release_allows_retry_after_failed_post() {
        for (name, lease) in backends() {
            assert_eq!(lease.acquire("rev-3").await.unwrap(), PostLease::Acquired);
            // The post failed → release the hold → a later approve may retry.
            lease.release("rev-3").await.unwrap();
            assert_eq!(
                lease.acquire("rev-3").await.unwrap(),
                PostLease::Acquired,
                "{name}: a released lease is re-acquirable"
            );
        }
    }

    #[tokio::test]
    async fn corner_release_never_undoes_a_committed_post() {
        for (name, lease) in backends() {
            lease.acquire("rev-4").await.unwrap();
            lease.commit("rev-4").await.unwrap();
            // A stray release after commit must not re-open the lease (no double post).
            lease.release("rev-4").await.unwrap();
            assert_eq!(
                lease.acquire("rev-4").await.unwrap(),
                PostLease::AlreadyPosted,
                "{name}: commit is terminal even after a release"
            );
        }
    }

    // MemoryPostLease keys review_id into an opaque in-process map, so a hostile value is
    // inert (never a path, never interpolated) and still dedups correctly.
    #[rstest]
    #[case::uuid("6f3d2a1e-0000-4b2c-9f1a-abcdef012345")]
    #[case::sql_meta("rev'; DROP TABLE post_lease;--")]
    #[case::traversal("../../etc/passwd")]
    #[tokio::test]
    async fn adversarial_hostile_review_id_inert_in_memory(#[case] id: &str) {
        let lease = MemoryPostLease::new();
        assert_eq!(
            lease.acquire(id).await.unwrap(),
            PostLease::Acquired,
            "first claim of a hostile id"
        );
        lease.commit(id).await.unwrap();
        assert_eq!(
            lease.acquire(id).await.unwrap(),
            PostLease::AlreadyPosted,
            "hostile id still dedups (opaque map key, not injected)"
        );
    }

    // StorePostLease makes review_id a card id, so a hostile value is rejected fail-closed
    // at every entry point (it can neither traverse nor inject); no rejected call mutates
    // the store. (A well-formed server-minted id — see the contract tests — is accepted.)
    #[cfg(feature = "fleet-store")]
    #[rstest]
    #[case::sql_meta("rev'; DROP TABLE post_lease;--")]
    #[case::traversal("../../etc/passwd")]
    #[case::separator("a/b")]
    #[case::leading_dash("-rf")]
    #[case::dotdot("..")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_store_hostile_review_id_rejected_fail_closed(#[case] id: &str) {
        let lease =
            StorePostLease::new(std::sync::Arc::new(agent_config_store::MemoryBackend::new()));
        assert!(lease.acquire(id).await.is_err(), "acquire {id:?}");
        assert!(lease.commit(id).await.is_err(), "commit {id:?}");
        assert!(lease.release(id).await.is_err(), "release {id:?}");
    }

    // adversarial: N concurrent acquires of the SAME review_id yield EXACTLY ONE
    // `Acquired` — the CAS none→held claim is atomic under the backend, so the
    // cross-process double-post is impossible even under a thundering herd.
    #[cfg(feature = "fleet-store")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn adversarial_racing_acquires_exactly_one_acquired() {
        use std::sync::Arc;
        let lease = Arc::new(StorePostLease::new(Arc::new(
            agent_config_store::MemoryBackend::new(),
        )));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let l = lease.clone();
                tokio::spawn(async move { l.acquire("rev-race").await })
            })
            .collect();
        let mut acquired = 0;
        for h in handles {
            if matches!(h.await.unwrap().unwrap(), PostLease::Acquired) {
                acquired += 1;
            }
        }
        assert_eq!(acquired, 1, "exactly one racer may acquire the lease");
    }

    // positive: a committed lease survives dropping and rebuilding the wrapper over the
    // SAME backend — all durable state lives in the store, not the wrapper (the hermetic
    // stand-in for a process restart; `pg_lease_tests` proves it over a real reconnect).
    #[cfg(feature = "fleet-store")]
    #[tokio::test]
    async fn positive_restart_durability_over_shared_backend() {
        use std::sync::Arc;
        let backend: Arc<dyn agent_config_store::Backend> =
            Arc::new(agent_config_store::MemoryBackend::new());
        {
            let lease = StorePostLease::new(backend.clone());
            assert_eq!(
                lease.acquire("rev-durable").await.unwrap(),
                PostLease::Acquired
            );
            lease.commit("rev-durable").await.unwrap();
        }
        let lease = StorePostLease::new(backend);
        assert_eq!(
            lease.acquire("rev-durable").await.unwrap(),
            PostLease::AlreadyPosted,
            "committed lease persists across a wrapper rebuild"
        );
    }

    #[tokio::test]
    async fn boundary_memory_eviction_bounds_the_map_but_keeps_recent() {
        // The in-memory tier caps posted leases; the oldest are evicted, the most recent
        // still dedup. (Durable backends have no such cap — they persist all.)
        let lease = MemoryPostLease::with_cap(2);
        for i in 0..5 {
            let id = format!("rev-{i}");
            lease.acquire(&id).await.unwrap();
            lease.commit(&id).await.unwrap();
        }
        // The two most-recent stay deduped…
        assert_eq!(
            lease.acquire("rev-4").await.unwrap(),
            PostLease::AlreadyPosted
        );
        assert_eq!(
            lease.acquire("rev-3").await.unwrap(),
            PostLease::AlreadyPosted
        );
        // …an evicted old one is re-acquirable (best-effort dedup, documented).
        assert_eq!(lease.acquire("rev-0").await.unwrap(), PostLease::Acquired);
    }
}

// The `StorePostLease` Postgres arm exercised against a REAL server — the durable,
// cross-process dedup this PR unlocks for a `postgres` fleet, and the tier `nix flake
// check` cannot host. `#[ignore]`-gated and run single-threaded by the `pg-integration`
// harness (`AGENT_CONFIG_STORE_TEST_DSN`). A dedicated tenant + a per-test cleanup keep
// re-runs idempotent without a global TRUNCATE.
#[cfg(all(test, feature = "fleet-store-postgres"))]
mod pg_lease_tests {
    use super::*;
    use agent_config_store::{Backend, PgBackend, Write};
    use std::sync::Arc;

    const IT_TENANT: &str = "c17_lease_it";
    // Must match `store_lease::POST_LEASE` (private); asserted by construction — the
    // contract tests above fail if the collection name drifts.
    const POST_LEASE: &str = "post_lease";

    // Connect, and hand back both the shared backend (for cleanup) and a lease over it.
    async fn pg_lease() -> (Arc<dyn Backend>, StorePostLease) {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend: Arc<dyn Backend> = Arc::new(
            PgBackend::connect(&dsn, 4, true)
                .await
                .expect("connect postgres + ensure schema"),
        );
        let lease = StorePostLease::with_tenant(backend.clone(), IT_TENANT).expect("tenant");
        (backend, lease)
    }

    // Unconditional delete of the given ids for this tenant (a `posted` lease is
    // terminal via the API, so tests reset with the raw backend, not `release`).
    async fn clean(backend: &Arc<dyn Backend>, ids: &[&str]) {
        let writes: Vec<Write> = std::iter::once(Write::EnsureTenant {
            tenant: IT_TENANT.to_string(),
        })
        .chain(ids.iter().map(|id| Write::Delete {
            collection: POST_LEASE,
            tenant: IT_TENANT.to_string(),
            id: (*id).to_string(),
        }))
        .collect();
        backend.apply(&writes).await.expect("clean");
    }

    // desc (postgres, live): first acquire wins; a sequential re-approve after commit
    // sees `AlreadyPosted` (the read-your-writes dedup) — durable across the server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_acquire_commit_dedups() {
        let (backend, lease) = pg_lease().await;
        clean(&backend, &["rev-1"]).await;
        assert_eq!(lease.acquire("rev-1").await.unwrap(), PostLease::Acquired);
        assert_eq!(lease.acquire("rev-1").await.unwrap(), PostLease::Held);
        lease.commit("rev-1").await.unwrap();
        assert_eq!(
            lease.acquire("rev-1").await.unwrap(),
            PostLease::AlreadyPosted
        );
    }

    // desc (postgres, live): a released hold is re-acquirable; a release after commit is
    // a no-op that never re-opens a posted lease (no double post).
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_release_then_commit_semantics() {
        let (backend, lease) = pg_lease().await;
        clean(&backend, &["rev-2"]).await;
        assert_eq!(lease.acquire("rev-2").await.unwrap(), PostLease::Acquired);
        lease.release("rev-2").await.unwrap();
        assert_eq!(
            lease.acquire("rev-2").await.unwrap(),
            PostLease::Acquired,
            "a released hold is re-acquirable"
        );
        lease.commit("rev-2").await.unwrap();
        lease.release("rev-2").await.unwrap(); // no-op after commit
        assert_eq!(
            lease.acquire("rev-2").await.unwrap(),
            PostLease::AlreadyPosted,
            "commit is terminal even after a stray release"
        );
    }

    // adversarial (postgres, live): a committed lease survives a full reconnect (a fresh
    // pool over the same server) — real cross-process/restart durability.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_restart_durability() {
        let (backend, lease) = pg_lease().await;
        clean(&backend, &["rev-restart"]).await;
        assert_eq!(
            lease.acquire("rev-restart").await.unwrap(),
            PostLease::Acquired
        );
        lease.commit("rev-restart").await.unwrap();
        drop(lease);
        drop(backend);
        // A brand-new connection/pool = a restart.
        let (_b2, lease2) = pg_lease().await;
        assert_eq!(
            lease2.acquire("rev-restart").await.unwrap(),
            PostLease::AlreadyPosted,
            "posted lease is durable across a reconnect"
        );
    }

    // adversarial (postgres, live): N concurrent acquires of one id over a shared pool
    // yield EXACTLY ONE `Acquired` — the CAS claim is atomic on the real server.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_racing_one_acquired() {
        let (backend, _lease) = pg_lease().await;
        clean(&backend, &["rev-pg-race"]).await;
        let lease = Arc::new(StorePostLease::with_tenant(backend, IT_TENANT).expect("tenant"));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let l = lease.clone();
                tokio::spawn(async move { l.acquire("rev-pg-race").await })
            })
            .collect();
        let mut acquired = 0;
        for h in handles {
            if matches!(h.await.unwrap().unwrap(), PostLease::Acquired) {
                acquired += 1;
            }
        }
        assert_eq!(acquired, 1, "exactly one racer acquires on the real server");
    }

    // adversarial (postgres, live): a hostile review_id is rejected fail-closed and never
    // reaches the server as a card id.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_hostile_review_id_rejected() {
        let (_backend, lease) = pg_lease().await;
        for id in ["../../etc/passwd", "rev'; DROP TABLE cards;--", ""] {
            assert!(lease.acquire(id).await.is_err(), "acquire {id:?}");
        }
    }
}
