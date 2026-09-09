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
}

/// Generate the full scenario matrix for one backend tier.
macro_rules! suite {
    ($modname:ident, $make:expr) => {
        mod $modname {
            use super::*;

            #[tokio::test]
            async fn positive_put_get_roundtrip() {
                scen::put_get_roundtrip($make).await;
            }
            #[tokio::test]
            async fn positive_multi_card_commit() {
                scen::multi_card_commit($make).await;
            }
            #[tokio::test]
            async fn positive_list_scoped_to_tenant() {
                scen::list_scoped_to_tenant($make).await;
            }
            #[tokio::test]
            async fn negative_missing_card() {
                scen::missing_card($make).await;
            }
            #[tokio::test]
            async fn negative_partial_failure_rolls_back_all() {
                scen::partial_failure_rolls_back_all($make).await;
            }
            #[tokio::test]
            async fn negative_fk_violation_rejected() {
                scen::fk_violation_rejected($make).await;
            }
            #[tokio::test]
            async fn boundary_max_cards_per_tenant() {
                scen::max_cards_per_tenant($make).await;
            }
            #[tokio::test]
            async fn boundary_number_clamped_on_ingest() {
                scen::number_clamped_on_ingest($make).await;
            }
            #[tokio::test]
            async fn corner_empty_document() {
                scen::empty_document($make).await;
            }
            #[tokio::test]
            async fn adversarial_hostile_tenant_id_confined() {
                scen::hostile_tenant_id_confined($make).await;
            }
            #[tokio::test]
            async fn adversarial_sql_injection_via_card_field() {
                scen::sql_injection_via_card_field($make).await;
            }
        }
    };
}

suite!(memory, mem_backend());
suite!(file, file_backend());
#[cfg(feature = "config-store-sqlite")]
suite!(sqlite, sqlite_backend());
