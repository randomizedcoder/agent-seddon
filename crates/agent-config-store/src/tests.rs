//! The table-driven backend matrix — every scenario runs unchanged over
//! [`MemoryBackend`], [`FileBackend`], and (under the `config-store-sqlite`
//! feature) [`SqliteBackend`], so the trait's behaviour is proven identical
//! across tiers. Four case classes (`positive_`/`negative_`/`boundary_`/
//! `corner_`) plus `adversarial_` for the untrusted `(tenant, id)` and card
//! fields; each row carries its own `desc`/`expect` intent.

use std::sync::Arc;

use agent_core::Result;
use serde::{Deserialize, Serialize};

use crate::{Backend, Card, FileBackend, MemoryBackend, Store, Write};

/// A JSON-backed test card. `weight` is the attacker-supplied number clamped on
/// ingest (mirrors `Upstream::sanitize`); `note` is a free-text field used to
/// prove a hostile string is inert data, never SQL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TestCard {
    id: String,
    note: String,
    weight: u32,
}

/// Ceiling `weight` is clamped to on ingest.
const MAX_WEIGHT: u32 = 1000;

impl Card for TestCard {
    const COLLECTION: &'static str = "test_cards";

    fn id(&self) -> &str {
        &self.id
    }

    fn sanitize(&mut self) {
        if self.weight > MAX_WEIGHT {
            self.weight = MAX_WEIGHT;
        }
    }

    fn validate(&self) -> Result<()> {
        if self.id.is_empty() {
            return Err(agent_core::Error::Config("test card: empty id".into()));
        }
        Ok(())
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("test card json")
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes)
            .map_err(|e| agent_core::Error::Config(format!("test card decode: {e}")))
    }
}

/// A second card type in a *different* collection, to prove one transaction
/// spans distinct card types on the shared backend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TestCardB {
    id: String,
    label: String,
}

impl Card for TestCardB {
    const COLLECTION: &'static str = "test_cards_b";

    fn id(&self) -> &str {
        &self.id
    }

    fn sanitize(&mut self) {}

    fn validate(&self) -> Result<()> {
        Ok(())
    }

    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("test card b json")
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes)
            .map_err(|e| agent_core::Error::Config(format!("test card b decode: {e}")))
    }
}

fn card(id: &str, weight: u32) -> TestCard {
    TestCard {
        id: id.to_string(),
        note: "n".to_string(),
        weight,
    }
}

// --- backend constructors, one fresh instance per test -----------------------

fn mem_backend() -> Arc<dyn Backend> {
    Arc::new(MemoryBackend::new())
}

fn file_backend() -> Arc<dyn Backend> {
    let dir = agent_testkit::tempdir();
    Arc::new(FileBackend::new(dir.join("store.json")))
}

#[cfg(feature = "config-store-sqlite")]
fn sqlite_backend() -> Arc<dyn Backend> {
    Arc::new(crate::SqliteBackend::open_in_memory().expect("open in-memory sqlite"))
}

/// A real-Postgres backend for the `#[ignore]`-gated suite (run only under the
/// `pg-integration` harness, which sets the DSN and runs single-threaded). Each
/// call ensures the schema and TRUNCATEs to a clean slate so every scenario —
/// which asserts exact counts — starts empty, exactly like the fresh
/// in-memory/tempdir tiers.
#[cfg(feature = "config-store-postgres")]
async fn pg_backend() -> Arc<dyn Backend> {
    use sqlx::postgres::PgPoolOptions;
    let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
        .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&dsn)
        .await
        .expect("connect postgres");
    sqlx::raw_sql(include_str!("../migrations/0001_config_store.sql"))
        .execute(&pool)
        .await
        .expect("ensure schema");
    // FK: cards references tenants; truncating both in one statement satisfies it.
    sqlx::query("TRUNCATE cards, tenants")
        .execute(&pool)
        .await
        .expect("reset to clean slate");
    Arc::new(crate::PgBackend::from_pool(pool))
}

// --- the scenarios, each generic over the backend tier -----------------------

mod scen {
    use super::*;

    /// positive: a put is readable back byte-identical.
    pub async fn put_get_roundtrip(backend: Arc<dyn Backend>) {
        let store = Store::<TestCard>::new(backend);
        let c = card("x", 5);
        store.put("orga", c.clone()).await.expect("put succeeds");
        let got = store.get("orga", "x").await.expect("get succeeds");
        assert_eq!(got, c, "roundtrip returns the stored card unchanged");
    }

    /// positive: one batch commits two *different* card types atomically.
    pub async fn multi_card_commit(backend: Arc<dyn Backend>) {
        let sa = Store::<TestCard>::new(backend.clone());
        let sb = Store::<TestCardB>::new(backend.clone());
        let mut b = sa.batch();
        b.put("org", card("x", 3)).expect("stage card a");
        b.put::<TestCardB>(
            "org",
            TestCardB {
                id: "y".into(),
                label: "L".into(),
            },
        )
        .expect("stage card b");
        b.commit().await.expect("commit both");
        sa.get("org", "x").await.expect("card a present");
        sb.get("org", "y").await.expect("card b present");
    }

    /// positive: list is scoped to one tenant, never bleeding across tenants.
    pub async fn list_scoped_to_tenant(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend);
        s.put("orga", card("a1", 1)).await.expect("put orga");
        s.put("orgb", card("b1", 1)).await.expect("put orgb");
        s.put("orgb", card("b2", 1)).await.expect("put orgb 2");
        assert_eq!(
            s.list("orga").await.expect("list orga").len(),
            1,
            "orga sees only its own"
        );
        assert_eq!(
            s.list("orgb").await.expect("list orgb").len(),
            2,
            "orgb sees only its own"
        );
    }

    /// positive: `tenants` enumerates exactly the distinct tenants owning a card
    /// in the collection — sorted, deduplicated, and blind to other collections.
    pub async fn tenants_enumerated(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend.clone());
        s.put("orgb", card("b1", 1)).await.expect("put orgb");
        s.put("orga", card("a1", 1)).await.expect("put orga");
        s.put("orga", card("a2", 1)).await.expect("put orga 2"); // same tenant twice
                                                                 // A card in a *different* collection under a third tenant must not appear.
        let other = Store::<TestCardB>::new(backend.clone());
        other
            .put(
                "orgc",
                TestCardB {
                    id: "c".into(),
                    label: "L".into(),
                },
            )
            .await
            .expect("put other collection");
        let ts = backend
            .tenants(TestCard::COLLECTION)
            .await
            .expect("tenants");
        assert_eq!(
            ts,
            vec!["orga".to_string(), "orgb".to_string()],
            "distinct + sorted, this collection only (orgc lives elsewhere)"
        );
    }

    /// corner: an empty collection has no tenants — an empty list, not an error.
    pub async fn tenants_empty(backend: Arc<dyn Backend>) {
        assert!(
            backend
                .tenants(TestCard::COLLECTION)
                .await
                .expect("tenants")
                .is_empty(),
            "no cards ⇒ no tenants"
        );
    }

    /// negative: getting an absent card is a `not found` error, not a panic.
    pub async fn missing_card(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend);
        let err = s.get("orga", "nope").await.expect_err("absent card errors");
        assert!(
            format!("{err}").contains("not found"),
            "error carries the not-found contract: {err}"
        );
    }

    /// negative: a batch whose Nth write is invalid persists NONE of it.
    pub async fn partial_failure_rolls_back_all(backend: Arc<dyn Backend>) {
        // A good card for existing-in-batch tenant `a`, then a Put for a tenant
        // that is neither ensured nor pre-existing -> the whole batch is rejected.
        let writes = vec![
            Write::EnsureTenant { tenant: "a".into() },
            Write::Put {
                collection: TestCard::COLLECTION,
                tenant: "a".into(),
                id: "good".into(),
                blob: card("good", 1).encode(),
            },
            Write::Put {
                collection: TestCard::COLLECTION,
                tenant: "ghost".into(),
                id: "bad".into(),
                blob: card("bad", 1).encode(),
            },
        ];
        backend
            .apply(&writes)
            .await
            .expect_err("invalid batch is rejected");
        assert_eq!(
            backend
                .count(TestCard::COLLECTION, "a")
                .await
                .expect("count"),
            0,
            "the good write in the same batch did not land",
        );
    }

    /// negative: a Put for a tenant that does not exist is a foreign-key reject.
    pub async fn fk_violation_rejected(backend: Arc<dyn Backend>) {
        let writes = vec![Write::Put {
            collection: TestCard::COLLECTION,
            tenant: "ghost".into(),
            id: "x".into(),
            blob: card("x", 1).encode(),
        }];
        let err = backend
            .apply(&writes)
            .await
            .expect_err("orphan card rejected");
        assert!(
            format!("{err}").contains("not found"),
            "FK violation surfaces not-found: {err}"
        );
    }

    /// boundary: the per-tenant cap admits up to N new ids, rejects the N+1th,
    /// but still allows updating an existing id.
    pub async fn max_cards_per_tenant(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::with_cap(backend, 2);
        s.put("t", card("a", 1)).await.expect("1st under cap");
        s.put("t", card("b", 1)).await.expect("2nd at cap");
        let err = s
            .put("t", card("c", 1))
            .await
            .expect_err("3rd new id over cap");
        assert!(
            format!("{err}").contains("full"),
            "cap error names fullness: {err}"
        );
        s.put("t", card("a", 9))
            .await
            .expect("update existing id stays under cap");
    }

    /// corner: the per-tenant cap is a **soft** ceiling. `Store::put`'s pre-check is
    /// not atomic with its count, so concurrent distinct-new-id writers (or a batch,
    /// which does not cap) can push a tenant past it. The accepted contract: the cap
    /// **re-converges** — once over, the next new-id `put` is still rejected, so growth
    /// stops; updating an existing id is always allowed. A batch commit stands in for
    /// the (inherently racy) concurrent overshoot so the test stays deterministic.
    pub async fn cap_is_soft_and_reconverges(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::with_cap(backend, 2);
        // Overshoot cap=2 atomically via the batch path (which does not enforce the
        // cap) — a deterministic stand-in for W concurrent writers each observing
        // count < cap before any commits.
        let mut b = s.batch();
        b.put("t", card("a", 1)).expect("stage a");
        b.put("t", card("b", 1)).expect("stage b");
        b.put("t", card("c", 1)).expect("stage c"); // 3 > cap of 2
        b.commit().await.expect("batch commits past the soft cap");
        assert_eq!(
            s.list("t").await.expect("list").len(),
            3,
            "the soft cap was overshot"
        );
        // Re-converges: a further NEW id is rejected, so growth halts at the overshoot.
        let err = s
            .put("t", card("d", 1))
            .await
            .expect_err("a new id past the soft cap is rejected");
        assert!(
            format!("{err}").contains("full"),
            "the cap is re-observed on the next put: {err}"
        );
        // Updating an EXISTING id is not a new id ⇒ always allowed, even over the cap.
        s.put("t", card("a", 9))
            .await
            .expect("updating an existing id stays allowed over the soft cap");
    }

    /// boundary: a hostile number is clamped on ingest, not stored raw.
    pub async fn number_clamped_on_ingest(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend);
        s.put("t", card("x", u32::MAX))
            .await
            .expect("put with huge weight");
        let got = s.get("t", "x").await.expect("get");
        assert_eq!(
            got.weight, MAX_WEIGHT,
            "weight clamped to the ceiling on ingest"
        );
    }

    /// corner: a fresh store is empty — list empty, delete idempotent-false,
    /// get not-found — with no file/table yet materialized.
    pub async fn empty_document(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend);
        assert!(
            s.list("t").await.expect("list empty").is_empty(),
            "empty store lists nothing"
        );
        assert!(
            !s.delete("t", "x").await.expect("delete absent"),
            "deleting absent returns false"
        );
        assert!(
            s.get("t", "x").await.is_err(),
            "get on empty store is not-found"
        );
    }

    /// adversarial: traversal/injection in the tenant or id is confined at the
    /// seam — never a key, never a path.
    pub async fn hostile_tenant_id_confined(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend);
        for bad in ["../etc", "a/b", "", "-lead", ".."] {
            assert!(
                s.list(bad).await.is_err(),
                "list rejects hostile tenant `{bad}`"
            );
            assert!(
                s.put(bad, card("x", 1)).await.is_err(),
                "put rejects hostile tenant `{bad}`"
            );
        }
        // A hostile *id* (carried on the card) is rejected too.
        assert!(
            s.put("t", card("../escape", 1)).await.is_err(),
            "put rejects hostile card id",
        );
    }

    /// adversarial: a SQL metacharacter payload in a card *field* is inert — it
    /// is opaque blob data, and ids/tenants only ever reach SQL as bound params.
    pub async fn sql_injection_via_card_field(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend);
        let evil = TestCard {
            id: "ok".into(),
            note: "'; DROP TABLE cards;--".into(),
            weight: 1,
        };
        s.put("t", evil.clone())
            .await
            .expect("hostile field stored as data");
        let got = s
            .get("t", "ok")
            .await
            .expect("table intact after hostile field");
        assert_eq!(got.note, evil.note, "hostile field round-trips verbatim");
        // A second write proves the table/store still exists and is writable.
        s.put("t", card("ok2", 1))
            .await
            .expect("store still writable");
        assert_eq!(
            s.list("t").await.expect("list").len(),
            2,
            "both cards present"
        );
    }

    // --- compare-and-swap: the scheduler S1 cross-driver exclusion primitive ---

    /// A `CompareAndSwap` on `(TestCard::COLLECTION, "t", id)`.
    fn cas(id: &str, expected: Option<Vec<u8>>, blob: Vec<u8>) -> Write {
        Write::CompareAndSwap {
            collection: TestCard::COLLECTION,
            tenant: "t".into(),
            id: id.into(),
            expected,
            blob,
        }
    }

    /// positive: a CAS whose `expected` matches the stored value swaps it.
    pub async fn cas_matching_expected_writes(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend.clone());
        s.put("t", card("x", 1)).await.expect("seed");
        let prior = backend
            .get(TestCard::COLLECTION, "t", "x")
            .await
            .expect("get")
            .expect("present");
        backend
            .apply(&[cas("x", Some(prior), card("x", 2).encode())])
            .await
            .expect("CAS on the current value swaps it");
        assert_eq!(
            s.get("t", "x").await.expect("get").weight,
            2,
            "the new value landed"
        );
    }

    /// positive: a CAS with `expected = None` on an absent id creates it.
    pub async fn cas_absent_expected_creates(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend.clone());
        // Seed a sibling so tenant `t` exists (the CAS's FK check requires it).
        s.put("t", card("seed", 1)).await.expect("seed tenant");
        backend
            .apply(&[cas("fresh", None, card("fresh", 1).encode())])
            .await
            .expect("CAS create on an absent id");
        s.get("t", "fresh")
            .await
            .expect("the created card is present");
    }

    /// negative: a CAS whose `expected` does not match is a conflict, and the
    /// stored value is left untouched (all-or-nothing).
    pub async fn cas_mismatch_is_conflict(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend.clone());
        s.put("t", card("x", 1)).await.expect("seed");
        let err = backend
            .apply(&[cas("x", Some(b"stale".to_vec()), card("x", 9).encode())])
            .await
            .expect_err("a stale `expected` must conflict");
        assert!(
            crate::is_conflict(&err),
            "the error is a CAS conflict: {err}"
        );
        assert_eq!(
            s.get("t", "x").await.expect("get").weight,
            1,
            "the value is unchanged after a conflict"
        );
    }

    /// negative: a CAS with `expected = None` on an id that already exists is a
    /// conflict — create-if-absent must not silently overwrite.
    pub async fn cas_absent_expected_but_present(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend.clone());
        s.put("t", card("x", 1)).await.expect("seed");
        let err = backend
            .apply(&[cas("x", None, card("x", 9).encode())])
            .await
            .expect_err("create-if-absent on an existing id conflicts");
        assert!(crate::is_conflict(&err), "conflict: {err}");
        assert_eq!(s.get("t", "x").await.expect("get").weight, 1, "unchanged");
    }

    /// corner: a batch mixing a valid Put with a conflicting CAS persists NONE of
    /// it — the CAS conflict rolls the whole batch back.
    pub async fn cas_batch_all_or_nothing(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend.clone());
        s.put("t", card("x", 1)).await.expect("seed the CAS target");
        let batch = vec![
            Write::Put {
                collection: TestCard::COLLECTION,
                tenant: "t".into(),
                id: "sibling".into(),
                blob: card("sibling", 1).encode(),
            },
            cas("x", Some(b"stale".to_vec()), card("x", 9).encode()),
        ];
        backend
            .apply(&batch)
            .await
            .expect_err("the conflicting CAS fails the whole batch");
        assert!(
            s.get("t", "sibling").await.is_err(),
            "the sibling Put in the same batch did not land"
        );
        assert_eq!(
            s.get("t", "x").await.expect("get").weight,
            1,
            "the CAS target is unchanged"
        );
    }

    /// boundary: the compare is exact — an `expected` differing from the stored
    /// value by a single trailing byte still conflicts.
    pub async fn cas_off_by_one_expected_conflicts(backend: Arc<dyn Backend>) {
        let s = Store::<TestCard>::new(backend.clone());
        s.put("t", card("x", 1)).await.expect("seed");
        let mut prior = backend
            .get(TestCard::COLLECTION, "t", "x")
            .await
            .expect("get")
            .expect("present");
        prior.push(b' '); // one byte different from the stored value
        let err = backend
            .apply(&[cas("x", Some(prior), card("x", 9).encode())])
            .await
            .expect_err("a near-miss `expected` conflicts");
        assert!(crate::is_conflict(&err), "conflict: {err}");
    }

    /// adversarial: a hostile tenant/id on a CAS is rejected by the segment gate
    /// (a validation reject before any store access) — never a conflict or a write.
    pub async fn cas_hostile_segment_rejected(backend: Arc<dyn Backend>) {
        let bad = Write::CompareAndSwap {
            collection: TestCard::COLLECTION,
            tenant: "../etc".into(),
            id: "x".into(),
            expected: None,
            blob: card("x", 1).encode(),
        };
        let err = backend
            .apply(&[bad])
            .await
            .expect_err("hostile tenant rejected");
        assert!(
            !crate::is_conflict(&err),
            "a hostile segment is a validation reject, not a CAS conflict: {err}"
        );
        assert!(
            format!("{err}").contains("invalid"),
            "segment reject: {err}"
        );
    }
}

/// Generate the full scenario matrix for one backend tier. The hermetic tiers
/// use the two-arg form; the `postgres` tier uses the `ignore` form so the whole
/// matrix is `#[ignore]`d (it needs a live server — run under the
/// `pg-integration` harness), while still proving the trait behaves identically.
macro_rules! suite {
    ($modname:ident, $make:expr) => {
        suite!(@gen $modname, $make,);
    };
    (ignore $modname:ident, $make:expr, $reason:literal) => {
        suite!(@gen $modname, $make, #[ignore = $reason]);
    };
    (@gen $modname:ident, $make:expr, $(#[$ig:meta])?) => {
        mod $modname {
            use super::*;

            #[tokio::test]
            $(#[$ig])?
            async fn positive_put_get_roundtrip() {
                scen::put_get_roundtrip($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn positive_multi_card_commit() {
                scen::multi_card_commit($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn positive_list_scoped_to_tenant() {
                scen::list_scoped_to_tenant($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn positive_tenants_enumerated() {
                scen::tenants_enumerated($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn corner_tenants_empty() {
                scen::tenants_empty($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn negative_missing_card() {
                scen::missing_card($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn negative_partial_failure_rolls_back_all() {
                scen::partial_failure_rolls_back_all($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn negative_fk_violation_rejected() {
                scen::fk_violation_rejected($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn boundary_max_cards_per_tenant() {
                scen::max_cards_per_tenant($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn corner_cap_is_soft_and_reconverges() {
                scen::cap_is_soft_and_reconverges($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn boundary_number_clamped_on_ingest() {
                scen::number_clamped_on_ingest($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn corner_empty_document() {
                scen::empty_document($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn adversarial_hostile_tenant_id_confined() {
                scen::hostile_tenant_id_confined($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn adversarial_sql_injection_via_card_field() {
                scen::sql_injection_via_card_field($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn positive_cas_matching_expected_writes() {
                scen::cas_matching_expected_writes($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn positive_cas_absent_expected_creates() {
                scen::cas_absent_expected_creates($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn negative_cas_mismatch_is_conflict() {
                scen::cas_mismatch_is_conflict($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn negative_cas_absent_expected_but_present() {
                scen::cas_absent_expected_but_present($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn corner_cas_batch_all_or_nothing() {
                scen::cas_batch_all_or_nothing($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn boundary_cas_off_by_one_expected_conflicts() {
                scen::cas_off_by_one_expected_conflicts($make).await;
            }
            #[tokio::test]
            $(#[$ig])?
            async fn adversarial_cas_hostile_segment_rejected() {
                scen::cas_hostile_segment_rejected($make).await;
            }
        }
    };
}

suite!(memory, mem_backend());
suite!(file, file_backend());
#[cfg(feature = "config-store-sqlite")]
suite!(sqlite, sqlite_backend());
#[cfg(feature = "config-store-postgres")]
suite!(
    ignore postgres,
    pg_backend().await,
    "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"
);

/// A second Postgres handle on the SAME database WITHOUT resetting — for the
/// concurrent-writer test, which needs two independent pools racing one row.
#[cfg(feature = "config-store-postgres")]
async fn pg_backend_no_reset() -> Arc<dyn Backend> {
    use sqlx::postgres::PgPoolOptions;
    let dsn = std::env::var("AGENT_CONFIG_STORE_TEST_DSN")
        .expect("AGENT_CONFIG_STORE_TEST_DSN must be set by the pg-integration harness");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&dsn)
        .await
        .expect("connect postgres");
    Arc::new(crate::PgBackend::from_pool(pool))
}

/// `corner` (postgres, live): two independent connections race a `put` of the
/// same `(tenant, id)`. Under MVCC the second INSERT … ON CONFLICT blocks on the
/// first's row lock, then takes the UPDATE branch — both commit (no lost update,
/// no deadlock/panic), and the surviving blob is one of the two writers'. The
/// barrier is `join!` (both futures in flight together), never a sleep.
#[cfg(feature = "config-store-postgres")]
#[tokio::test]
#[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
async fn corner_concurrent_writers_last_write_conflicts() {
    let a = pg_backend().await; // resets the DB to a clean slate
    let b = pg_backend_no_reset().await; // second pool on the same DB
                                         // Serialize the tenant's creation once so both racers hit an existing tenant.
    Store::<TestCard>::new(a.clone())
        .put("t", card("seed", 1))
        .await
        .expect("seed tenant");

    let sa = Store::<TestCard>::new(a);
    let sb = Store::<TestCard>::new(b);
    let (ra, rb) = tokio::join!(sa.put("t", card("x", 1)), sb.put("t", card("x", 2)));
    ra.expect("writer A commits");
    rb.expect("writer B commits");

    let got = sa.get("t", "x").await.expect("row present after the race");
    assert!(
        got.weight == 1 || got.weight == 2,
        "last write wins with no lost update; weight = {}",
        got.weight
    );
    // The seed and the raced id are the only two cards — no phantom duplicate.
    assert_eq!(
        sa.list("t").await.expect("list").len(),
        2,
        "no lost/duplicated rows"
    );
}

/// `adversarial` (postgres, live, S1): two independent connections race a
/// `CompareAndSwap` of the same row, both conditioned on the same prior value.
/// `SELECT … FOR UPDATE` serializes them, so exactly one CAS commits and the
/// other sees the now-changed value and returns a conflict — the true
/// cross-driver mutual exclusion the atomic-batch claim could not give. The
/// barrier is `join!` (both in flight together), never a sleep.
#[cfg(feature = "config-store-postgres")]
#[tokio::test]
#[ignore = "needs a running postgres (nix run .#postgres-up) — run via `nix run .#integration`"]
async fn adversarial_concurrent_cas_exactly_one_wins() {
    let a = pg_backend().await; // resets the DB to a clean slate
    let b = pg_backend_no_reset().await; // second pool on the same DB
    let sa = Store::<TestCard>::new(a.clone());
    sa.put("t", card("x", 1))
        .await
        .expect("seed the CAS target");
    let prior = a
        .get(TestCard::COLLECTION, "t", "x")
        .await
        .expect("get")
        .expect("present");

    let mk = |weight: u32| {
        vec![Write::CompareAndSwap {
            collection: TestCard::COLLECTION,
            tenant: "t".into(),
            id: "x".into(),
            expected: Some(prior.clone()),
            blob: card("x", weight).encode(),
        }]
    };
    let (wa, wb) = (mk(2), mk(3));
    let (ra, rb) = tokio::join!(a.apply(&wa), b.apply(&wb));

    let results = [ra, rb];
    let wins = results.iter().filter(|r| r.is_ok()).count();
    let conflicts = results
        .iter()
        .filter(|r| matches!(r, Err(e) if crate::is_conflict(e)))
        .count();
    assert_eq!(wins, 1, "exactly one CAS wins: {results:?}");
    assert_eq!(conflicts, 1, "the loser sees a conflict: {results:?}");
    // The surviving value is the winner's, and it is the only card besides… none.
    let got = sa.get("t", "x").await.expect("row present after the race");
    assert!(
        got.weight == 2 || got.weight == 3,
        "the winner's value survived; weight = {}",
        got.weight
    );
}
