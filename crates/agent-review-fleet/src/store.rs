//! `StoreFleet` — the [`FleetRegistry`] backed by the shared transactional config
//! store (`agent-config-store`, config design C41 / increment A3b).
//!
//! The clean twin of the registry's `StoreRegistry`: instead of a bespoke
//! file/SQLite roster, the fleet persists onto any [`agent_config_store::Backend`]
//! (memory, file, sqlite, or — the capability A3b unlocks — **postgres**). The
//! legacy [`MemoryFleet`](crate::MemoryFleet) / [`FileFleet`](crate::FileFleet) /
//! `SqliteFleet` backends are **untouched** (decision: keep `rusqlite` for
//! `file`/`sqlite`, add postgres as the new `sqlx` tier), so their behaviour — and
//! their tests — are unchanged; this backend adds the shared-store path beside
//! them.
//!
//! **Behaviour-identical by construction.** Every mutation routes through the same
//! shared [`crate::ops`], and reads decode the same **JSON** roster rows the file
//! and SQLite tiers use (one at-rest shape, so encodings can't drift) then
//! `ops::revalidate` fail-closed. The store is a serialized snapshot → op →
//! rewrite cycle, committed as one atomic [`Backend::apply`] batch — exactly like
//! the SQLite tier's `mutate`, so the tiers stay interchangeable.
//!
//! **Untrusted input, fail closed.** Rows decode-then-`validate` on read (an
//! out-of-band-tampered row fails closed at the seam); ids reach the backend only
//! after `safe_segment` (via the shared `ops`/`check_id`), and the backend itself
//! binds them as parameters. `token_ref` is stored verbatim as a reference.

use std::sync::Arc;

use agent_config_store::{Backend, Write};
use agent_core::{safe_segment, Error, FleetRegistry, FleetSession, Result};
use async_trait::async_trait;

use crate::{check_id, not_found, ops};

/// The collection holding one card per roster row (id = the session id).
const FLEET: &str = "fleet_sessions";
/// The default single-tenant scope. Per-tenant scoping (a verified tenant per
/// call) arrives with the per-tenant plane (config C35 / increment C2); until
/// then the roster is one un-namespaced control plane under this key.
pub const DEFAULT_TENANT: &str = "local";

/// A [`FleetRegistry`] persisted on a shared [`Backend`]. Cheap to clone (an
/// `Arc` handle plus the tenant key).
pub struct StoreFleet {
    backend: Arc<dyn Backend>,
    tenant: String,
}

impl StoreFleet {
    /// A roster over `backend` under the default single-tenant scope.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            tenant: DEFAULT_TENANT.to_string(),
        }
    }

    /// A roster scoped to an explicit tenant (config C2 will call this per
    /// verified identity). The tenant is `safe_segment`-gated — a hostile tenant
    /// is rejected at construction, never persisted or turned into a key.
    pub fn with_tenant(backend: Arc<dyn Backend>, tenant: &str) -> Result<Self> {
        if !safe_segment(tenant) {
            return Err(Error::Fleet(format!("invalid tenant `{tenant}`")));
        }
        Ok(Self {
            backend,
            tenant: tenant.to_string(),
        })
    }

    /// Snapshot the whole roster from the store. JSON decode + `ops::revalidate`
    /// fail closed on an out-of-band-tampered row; list order is the backend's
    /// insertion order (`pos`), matching the file/memory/sqlite backends.
    async fn load(&self) -> Result<Vec<FleetSession>> {
        let rows: Vec<FleetSession> = self
            .backend
            .list(FLEET, &self.tenant)
            .await?
            .into_iter()
            .map(|blob| {
                serde_json::from_slice::<FleetSession>(&blob)
                    .map_err(|e| Error::Fleet(format!("stored fleet row decode: {e}")))
            })
            .collect::<Result<_>>()?;
        ops::revalidate(&rows)?;
        Ok(rows)
    }

    /// One serialized snapshot → shared op → rewrite cycle, committed atomically.
    /// Rewriting the (≤`MAX_FLEET_ROWS`) card set inside a single
    /// [`Backend::apply`] batch mirrors the SQLite tier's `mutate`, keeping the
    /// backends byte-for-byte interchangeable without a per-row path that could
    /// drift from `ops`.
    async fn mutate<T>(&self, f: impl FnOnce(&mut Vec<FleetSession>) -> Result<T>) -> Result<T> {
        use std::collections::HashSet;
        let mut rows = self.load().await?;
        let before: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
        let out = f(&mut rows)?;
        ops::revalidate(&rows)?;
        let after: HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();

        let mut writes: Vec<Write> = vec![Write::EnsureTenant {
            tenant: self.tenant.clone(),
        }];
        // Remove rows that disappeared from the snapshot …
        for id in &before {
            if !after.contains(id.as_str()) {
                writes.push(Write::Delete {
                    collection: FLEET,
                    tenant: self.tenant.clone(),
                    id: id.clone(),
                });
            }
        }
        // … then upsert every current row (upsert keeps `pos` for existing ids,
        // appends new ones — preserving order like `ops::put`'s push).
        for r in &rows {
            let blob = serde_json::to_vec(r)
                .map_err(|e| Error::Fleet(format!("serialize fleet row: {e}")))?;
            writes.push(Write::Put {
                collection: FLEET,
                tenant: self.tenant.clone(),
                id: r.id.clone(),
                blob,
            });
        }
        self.backend.apply(&writes).await?;
        Ok(out)
    }
}

#[async_trait]
impl FleetRegistry for StoreFleet {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        self.load().await
    }
    async fn get(&self, id: &str) -> Result<FleetSession> {
        check_id(id)?;
        self.load()
            .await?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| not_found(id))
    }
    async fn put(&self, session: FleetSession) -> Result<FleetSession> {
        self.mutate(|rows| ops::put(rows, session)).await
    }
    async fn delete(&self, id: &str) -> Result<bool> {
        self.mutate(|rows| ops::delete(rows, id)).await
    }
    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<FleetSession> {
        self.mutate(|rows| ops::set_enabled(rows, id, enabled))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::row;
    use crate::MemoryFleet;
    use agent_config_store::MemoryBackend;
    use agent_core::MAX_FLEET_ROWS;
    use rstest::rstest;

    fn store() -> StoreFleet {
        StoreFleet::new(Arc::new(MemoryBackend::new()))
    }

    async fn seeded() -> StoreFleet {
        let reg = store();
        reg.put(row("r1")).await.expect("seed r1");
        reg.put(row("r2")).await.expect("seed r2");
        reg
    }

    // desc: a row survives the encode→store→decode roundtrip byte-for-byte.
    #[tokio::test]
    async fn positive_put_get_roundtrips_through_blobs() {
        let reg = store();
        let stored = reg.put(row("r1")).await.expect("put");
        assert_eq!(stored, row("r1"));
        assert_eq!(reg.get("r1").await.expect("get"), row("r1"));
    }

    // desc: the store backend and the in-memory backend list identically (the
    // behaviour-preservation proof for the converged tier).
    #[tokio::test]
    async fn positive_store_and_memory_backends_agree() {
        let store = seeded().await;
        let mem = MemoryFleet::new();
        mem.put(row("r1")).await.unwrap();
        mem.put(row("r2")).await.unwrap();
        assert_eq!(store.list().await.unwrap(), mem.list().await.unwrap());
    }

    // desc: set_enabled toggles the row and persists, keeping it listed.
    #[tokio::test]
    async fn positive_set_enabled_toggles_and_keeps_row() {
        let reg = seeded().await;
        let off = reg.set_enabled("r1", false).await.expect("disable");
        assert!(!off.enabled);
        assert_eq!(reg.list().await.unwrap().len(), 2, "row stays listed");
        assert!(!reg.get("r1").await.unwrap().enabled, "toggle persisted");
        assert!(reg.set_enabled("r1", true).await.unwrap().enabled);
    }

    // desc: delete reports existed-then-absent and shrinks the roster.
    #[tokio::test]
    async fn positive_delete_true_then_false() {
        let reg = seeded().await;
        assert!(reg.delete("r1").await.expect("first"));
        assert!(!reg.delete("r1").await.expect("second"));
        assert_eq!(reg.list().await.unwrap().len(), 1);
    }

    // desc: state persists across a re-open sharing the same backend handle.
    #[tokio::test]
    async fn positive_persists_across_reopen_of_shared_backend() {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        {
            let reg = StoreFleet::new(backend.clone());
            reg.put(row("r1")).await.expect("put");
            reg.set_enabled("r1", false).await.expect("disable");
        }
        let reg = StoreFleet::new(backend);
        assert!(
            !reg.get("r1").await.expect("get").enabled,
            "toggle persisted"
        );
    }

    // negative: an unknown id is `not found` / Ok(false), the seam contract.
    #[tokio::test]
    async fn negative_delete_unknown_false_set_enabled_unknown_not_found() {
        let reg = store();
        assert!(!reg.delete("ghost").await.expect("delete"));
        let err = reg
            .set_enabled("ghost", true)
            .await
            .expect_err("set_enabled");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    // corner: re-putting the same id upserts in place (not a duplicate).
    #[tokio::test]
    async fn corner_put_same_id_updates_in_place() {
        let reg = store();
        reg.put(row("r1")).await.unwrap();
        let mut upd = row("r1");
        upd.skill = "deep-review".into();
        reg.put(upd).await.unwrap();
        assert_eq!(reg.list().await.unwrap().len(), 1, "upsert, not duplicate");
        assert_eq!(reg.get("r1").await.unwrap().skill, "deep-review");
    }

    // boundary: a full roster rejects an insert but still allows an update.
    #[tokio::test]
    async fn boundary_roster_full_rejects_insert_but_allows_update() {
        let reg = store();
        for i in 0..MAX_FLEET_ROWS {
            reg.put(row(&format!("r{i}"))).await.expect("fits");
        }
        assert!(reg.put(row("one-too-many")).await.is_err());
        let mut upd = row("r0");
        upd.skill = "x".into();
        assert!(reg.put(upd).await.is_ok(), "upsert at cap still allowed");
    }

    // adversarial: every hostile id is rejected at every entry point, and no
    // rejected call mutates the roster.
    #[rstest]
    #[case::traversal("../../etc/passwd")]
    #[case::separator("a/b")]
    #[case::leading_dash("-rf")]
    #[case::dotdot("..")]
    #[case::empty("")]
    #[tokio::test]
    async fn adversarial_hostile_ids_rejected_everywhere(#[case] id: &str) {
        let reg = seeded().await;
        assert!(reg.get(id).await.is_err(), "get {id:?}");
        assert!(reg.delete(id).await.is_err(), "delete {id:?}");
        assert!(
            reg.set_enabled(id, true).await.is_err(),
            "set_enabled {id:?}"
        );
        let mut bad = row("ok");
        bad.id = id.into();
        assert!(reg.put(bad).await.is_err(), "put {id:?}");
        assert_eq!(reg.list().await.unwrap().len(), 2, "unchanged after {id:?}");
    }

    // adversarial: a raw token (not a `*_ref`) is rejected, error never echoes it.
    #[tokio::test]
    async fn adversarial_raw_token_in_token_ref_rejected() {
        let reg = store();
        let mut bad = row("r1");
        bad.token_ref = "ghp_rawsecrettoken".into();
        let err = reg.put(bad).await.expect_err("raw token rejected");
        assert!(!err.to_string().contains("ghp_rawsecret"), "no echo: {err}");
    }

    // adversarial: a blob tampered out of band to decode to a traversal id fails
    // closed on the next read (decode → revalidate rejects it).
    #[tokio::test]
    async fn adversarial_out_of_band_row_tamper_fails_closed() {
        let backend: Arc<dyn Backend> = Arc::new(MemoryBackend::new());
        let reg = StoreFleet::new(backend.clone());
        reg.put(row("r1")).await.expect("seed");
        let evil = serde_json::to_vec(&FleetSession {
            id: "../escape".into(),
            ..Default::default()
        })
        .unwrap();
        backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: DEFAULT_TENANT.to_string(),
                },
                Write::Put {
                    collection: FLEET,
                    tenant: DEFAULT_TENANT.to_string(),
                    id: "r1".to_string(),
                    blob: evil,
                },
            ])
            .await
            .expect("tamper");
        assert!(reg.list().await.is_err(), "tampered row must fail closed");
    }

    // adversarial: an unknown forge backend is refused.
    #[tokio::test]
    async fn adversarial_unknown_backend_rejected() {
        let reg = store();
        let mut bad = row("r1");
        bad.backend = "evil-forge".into();
        assert!(
            reg.put(bad).await.is_err(),
            "only github/gitlab/'' accepted"
        );
    }
}

// The Postgres arm exercised against a REAL server — the tier `nix flake check`
// cannot host. `#[ignore]`-gated and run single-threaded by the `pg-integration`
// harness (`AGENT_CONFIG_STORE_TEST_DSN`). A dedicated tenant keeps the run
// isolated without a global TRUNCATE.
#[cfg(all(test, feature = "fleet-store-postgres"))]
mod pg_tests {
    use super::*;
    use crate::testdata::row;
    use crate::MemoryFleet;
    use agent_config_store::PgBackend;

    const IT_TENANT: &str = "a3b_fleet_it";

    async fn pg_fleet() -> StoreFleet {
        let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
            .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
        let backend = PgBackend::connect(&dsn, 4, true)
            .await
            .expect("connect postgres + ensure schema");
        let reg = StoreFleet::with_tenant(Arc::new(backend), IT_TENANT).expect("tenant");
        // Clean slate for this tenant (idempotent across re-runs).
        for r in reg.list().await.expect("list") {
            reg.delete(&r.id).await.expect("cleanup delete");
        }
        reg
    }

    // desc (postgres, live): the full CRUD matrix roundtrips over a real server
    // and agrees with the in-memory backend.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn positive_pg_crud_agrees_with_memory() {
        let reg = pg_fleet().await;
        reg.put(row("r1")).await.expect("put");
        reg.put(row("r2")).await.expect("put");

        let mem = MemoryFleet::new();
        mem.put(row("r1")).await.unwrap();
        mem.put(row("r2")).await.unwrap();
        assert_eq!(reg.list().await.unwrap(), mem.list().await.unwrap());

        assert!(!reg.set_enabled("r1", false).await.unwrap().enabled);
        assert!(!reg.get("r1").await.unwrap().enabled, "toggle durable");
        assert!(reg.delete("r1").await.unwrap());
        assert_eq!(reg.list().await.unwrap().len(), 1);
    }

    // adversarial (postgres, live): a hostile id never mutates the roster.
    #[tokio::test]
    #[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
    async fn adversarial_pg_hostile_ids_rejected() {
        let reg = pg_fleet().await;
        reg.put(row("r1")).await.expect("seed");
        for id in ["../../etc/passwd", "a/b", ""] {
            assert!(reg.get(id).await.is_err(), "get {id:?}");
            assert!(reg.delete(id).await.is_err(), "delete {id:?}");
        }
        assert_eq!(reg.list().await.unwrap().len(), 1, "store unchanged");
    }
}
