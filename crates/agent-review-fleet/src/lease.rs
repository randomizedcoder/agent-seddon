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
//! Two backends, mirroring the roster triad:
//! - [`MemoryPostLease`] — in-process (a mutex-guarded map). Atomic **within one process**
//!   (closes the concurrent + sequential-within-process double-post), the default and the
//!   test double. It does **not** survive a restart or coordinate across processes — a
//!   fleet that needs that must run a durable backend.
//! - `SqlitePostLease` (feature `fleet-sqlite`) — a durable, cross-process CAS via an
//!   `INSERT … ON CONFLICT DO NOTHING` claim in an embedded SQLite table.
//!
//! `review_id` is untrusted wire input; every backend binds it as a query argument and
//! never turns it into a path.

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

#[cfg(feature = "fleet-sqlite")]
pub use sqlite_lease::SqlitePostLease;

#[cfg(feature = "fleet-sqlite")]
mod sqlite_lease {
    use super::*;
    use agent_core::Error;
    use rusqlite::{params, Connection};

    fn sql_err(e: impl std::fmt::Display) -> Error {
        Error::Fleet(format!("post-lease sqlite: {e}"))
    }

    /// A durable, cross-process [`FleetPostLease`] backed by an embedded SQLite table.
    ///
    /// The claim is a single atomic statement — `INSERT … ON CONFLICT(review_id) DO
    /// NOTHING` — so exactly one racer (across threads *and* processes sharing the file)
    /// inserts the `held` row and observes `changes() == 1`; the losers read the existing
    /// row's status. `busy_timeout` lets a second connection to the same file wait out a
    /// writer's lock rather than fail.
    pub struct SqlitePostLease {
        conn: std::sync::Mutex<Connection>,
    }

    impl SqlitePostLease {
        /// Open (creating if absent) the lease table in the SQLite database at `path`. May
        /// share the file with [`crate::SqliteFleet`] — it uses its own table.
        pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
            let conn = Connection::open(path).map_err(sql_err)?;
            Self::from_conn(conn)
        }

        /// An in-memory lease (tests): durable within the connection's lifetime only.
        pub fn open_in_memory() -> Result<Self> {
            Self::from_conn(Connection::open_in_memory().map_err(sql_err)?)
        }

        fn from_conn(conn: Connection) -> Result<Self> {
            // Wait out a concurrent writer's lock (the second connection to a shared file)
            // rather than error immediately.
            conn.busy_timeout(std::time::Duration::from_secs(5))
                .map_err(sql_err)?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS post_lease (
                     review_id TEXT PRIMARY KEY,
                     status    TEXT NOT NULL,
                     ts_ms     INTEGER NOT NULL
                 );",
            )
            .map_err(sql_err)?;
            Ok(Self {
                conn: std::sync::Mutex::new(conn),
            })
        }

        fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
            self.conn
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    #[async_trait]
    impl FleetPostLease for SqlitePostLease {
        async fn acquire(&self, review_id: &str) -> Result<PostLease> {
            let conn = self.lock();
            // Atomic claim: only the first racer inserts the `held` row.
            let inserted = conn
                .execute(
                    "INSERT INTO post_lease (review_id, status, ts_ms) VALUES (?1, 'held', 0)
                     ON CONFLICT(review_id) DO NOTHING",
                    params![review_id],
                )
                .map_err(sql_err)?;
            if inserted == 1 {
                return Ok(PostLease::Acquired);
            }
            // Someone else holds or already posted — read the (now-stable) row.
            let status: Option<String> = conn
                .query_row(
                    "SELECT status FROM post_lease WHERE review_id = ?1",
                    params![review_id],
                    |r| r.get(0),
                )
                .map_err(sql_err)?;
            match status.as_deref() {
                Some("posted") => Ok(PostLease::AlreadyPosted),
                // A `held` row (or a row that vanished via a concurrent release — treat the
                // absence as still-contended for this call; the next approve re-acquires).
                _ => Ok(PostLease::Held),
            }
        }

        async fn commit(&self, review_id: &str) -> Result<()> {
            let conn = self.lock();
            // Upsert to `posted` so a held→posted flip is durable even if the original
            // `held` row was lost; terminal thereafter.
            conn.execute(
                "INSERT INTO post_lease (review_id, status, ts_ms) VALUES (?1, 'posted', 0)
                 ON CONFLICT(review_id) DO UPDATE SET status = 'posted'",
                params![review_id],
            )
            .map_err(sql_err)?;
            Ok(())
        }

        async fn release(&self, review_id: &str) -> Result<()> {
            let conn = self.lock();
            // Release only a held (un-posted) lease; never delete a committed post.
            conn.execute(
                "DELETE FROM post_lease WHERE review_id = ?1 AND status = 'held'",
                params![review_id],
            )
            .map_err(sql_err)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // MemoryPostLease and (when built) SqlitePostLease must satisfy the SAME contract, so
    // the behavioural tests run over both via a small factory list.
    fn backends() -> Vec<(&'static str, Box<dyn FleetPostLease>)> {
        #[cfg_attr(not(feature = "fleet-sqlite"), allow(unused_mut))]
        let mut v: Vec<(&'static str, Box<dyn FleetPostLease>)> =
            vec![("memory", Box::new(MemoryPostLease::new()))];
        #[cfg(feature = "fleet-sqlite")]
        v.push((
            "sqlite",
            Box::new(SqlitePostLease::open_in_memory().expect("open in-memory lease")),
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

    #[rstest]
    #[case::uuid("6f3d2a1e-0000-4b2c-9f1a-abcdef012345")]
    // review_id is untrusted wire input; a hostile value is a bound query arg (SQLite) /
    // map key (memory), never a path or interpolated SQL — it can neither escape nor
    // inject, and still dedups correctly.
    #[case::sql_meta("rev'; DROP TABLE post_lease;--")]
    #[case::traversal("../../etc/passwd")]
    #[tokio::test]
    async fn adversarial_hostile_review_id_is_inert_and_still_dedups(#[case] id: &str) {
        for (name, lease) in backends() {
            assert_eq!(
                lease.acquire(id).await.unwrap(),
                PostLease::Acquired,
                "{name}: first claim of a hostile id"
            );
            lease.commit(id).await.unwrap();
            assert_eq!(
                lease.acquire(id).await.unwrap(),
                PostLease::AlreadyPosted,
                "{name}: hostile id still dedups (bound arg, not injected)"
            );
        }
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
