//! `SqliteRegistry` — the embedded-SQLite [`ProviderRegistry`] backend, behind
//! the non-default `registry-sqlite` feature (mirrors `agent-prompt`'s
//! `prompt-sqlite`). Cards and the policy are stored as **prost-encoded wire
//! messages** (`pb::Upstream` / `pb::RoutePolicy`) — the schema is the proto,
//! so the three encodings (binary blob, textproto file, Rust structs) can
//! never drift.
//!
//! **Interchangeable with the file/memory backends**: every mutation routes
//! through the same shared `ops` (validation, clamps, caps), and reads re-run
//! the wire→core decode (which clamps) + `validate` — a row edited out of band
//! fails closed at the seam. Ids reach SQL only as **bound parameters**.

use std::path::Path;
use std::sync::Mutex;

use agent_core::{
    Error, ModelRouterConfig, ProviderRegistry, Result, RouteDecision, RouteHint, RoutePolicySpec,
    Upstream, UpstreamHealth,
};
use agent_proto::pb;
use async_trait::async_trait;
use prost::Message;
use rusqlite::{params, Connection};

use crate::{check_id, decide, not_found, ops, static_health};

/// A SQLite-backed [`ProviderRegistry`]. The connection is wrapped in a `Mutex`
/// (rusqlite's `Connection` is `Send` but `!Sync`); every method locks it for a
/// short, synchronous query and never holds the guard across an `.await` — a
/// low-traffic control-plane surface, not the hot loop.
pub struct SqliteRegistry {
    conn: Mutex<Connection>,
}

impl SqliteRegistry {
    /// Open (creating if absent) the registry at `path` and ensure the schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).map_err(sql_err)?;
        Self::from_conn(conn)
    }

    /// An in-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::from_conn(Connection::open_in_memory().map_err(sql_err)?)
    }

    fn from_conn(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS upstreams (
                 id   TEXT NOT NULL PRIMARY KEY,
                 pos  INTEGER NOT NULL,
                 card BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS policy (
                 id   INTEGER NOT NULL PRIMARY KEY CHECK (id = 0),
                 data BLOB NOT NULL
             );",
        )
        .map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Snapshot the whole config (decode clamps numbers; validate fails closed
    /// on out-of-band tampering). `pos` preserves insertion order so routing
    /// order ties break exactly like the file/memory backends.
    fn load(&self, conn: &Connection) -> Result<ModelRouterConfig> {
        let mut stmt = conn
            .prepare("SELECT card FROM upstreams ORDER BY pos, id")
            .map_err(sql_err)?;
        let upstreams: Vec<Upstream> = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(sql_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)?
            .into_iter()
            .map(|blob| {
                pb::Upstream::decode(blob.as_slice())
                    .map(Upstream::from)
                    .map_err(|e| Error::Registry(format!("stored card decode: {e}")))
            })
            .collect::<Result<_>>()?;
        let policy: RoutePolicySpec = conn
            .query_row("SELECT data FROM policy WHERE id = 0", [], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(sql_err(e)),
            })?
            .map(|blob| {
                pb::RoutePolicy::decode(blob.as_slice())
                    .map(RoutePolicySpec::from)
                    .map_err(|e| Error::Registry(format!("stored policy decode: {e}")))
            })
            .transpose()?
            .unwrap_or_default();
        let cfg = ModelRouterConfig { upstreams, policy };
        cfg.validate()?;
        Ok(cfg)
    }

    /// One serialized read-modify-write cycle: snapshot → shared op → rewrite.
    /// Rewriting the (≤512-row) table inside a transaction keeps the three
    /// backends byte-for-byte interchangeable in behaviour without a parallel
    /// per-row mutation path that could drift from `ops`.
    fn mutate<T>(&self, f: impl FnOnce(&mut ModelRouterConfig) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let conn = &mut *guard;
        let mut cfg = self.load(conn)?;
        let out = f(&mut cfg)?;
        cfg.validate()?;
        let tx = conn.transaction().map_err(sql_err)?;
        tx.execute("DELETE FROM upstreams", []).map_err(sql_err)?;
        for (pos, u) in cfg.upstreams.iter().enumerate() {
            let blob = pb::Upstream::from(u.clone()).encode_to_vec();
            tx.execute(
                "INSERT INTO upstreams (id, pos, card) VALUES (?1, ?2, ?3)",
                params![u.id, pos as i64, blob],
            )
            .map_err(sql_err)?;
        }
        let policy_blob = pb::RoutePolicy::from(cfg.policy.clone()).encode_to_vec();
        tx.execute(
            "INSERT OR REPLACE INTO policy (id, data) VALUES (0, ?1)",
            params![policy_blob],
        )
        .map_err(sql_err)?;
        tx.commit().map_err(sql_err)?;
        Ok(out)
    }

    fn snapshot(&self) -> Result<ModelRouterConfig> {
        let guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.load(&guard)
    }
}

fn sql_err(e: rusqlite::Error) -> Error {
    Error::Registry(format!("sqlite: {e}"))
}

#[async_trait]
impl ProviderRegistry for SqliteRegistry {
    async fn list(&self) -> Result<Vec<Upstream>> {
        Ok(self.snapshot()?.upstreams)
    }
    async fn get(&self, id: &str) -> Result<Upstream> {
        check_id(id)?;
        self.snapshot()?
            .upstreams
            .into_iter()
            .find(|u| u.id == id)
            .ok_or_else(|| not_found(id))
    }
    async fn put(&self, card: Upstream) -> Result<Upstream> {
        self.mutate(|cfg| ops::put(cfg, card))
    }
    async fn delete(&self, id: &str) -> Result<bool> {
        self.mutate(|cfg| ops::delete(cfg, id))
    }
    async fn enable(&self, id: &str, enabled: bool) -> Result<Upstream> {
        self.mutate(|cfg| ops::enable(cfg, id, enabled))
    }
    async fn get_policy(&self) -> Result<RoutePolicySpec> {
        Ok(self.snapshot()?.policy)
    }
    async fn put_policy(&self, policy: RoutePolicySpec) -> Result<RoutePolicySpec> {
        self.mutate(|cfg| ops::put_policy(cfg, policy))
    }
    async fn route(&self, hint: &RouteHint) -> Result<RouteDecision> {
        Ok(decide(&self.snapshot()?, hint))
    }
    async fn health(&self) -> Result<Vec<UpstreamHealth>> {
        Ok(static_health(&self.snapshot()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::{card, config};
    use agent_core::RouteRole;

    async fn seeded() -> SqliteRegistry {
        let reg = SqliteRegistry::open_in_memory().expect("open");
        for u in config().upstreams {
            reg.put(u).await.expect("seed put");
        }
        reg.put_policy(config().policy).await.expect("seed policy");
        reg
    }

    #[tokio::test]
    async fn positive_put_get_roundtrips_through_blobs() {
        let reg = SqliteRegistry::open_in_memory().expect("open");
        reg.put(card("kimi")).await.expect("put");
        assert_eq!(reg.get("kimi").await.expect("get"), card("kimi"));
    }

    #[tokio::test]
    async fn positive_persists_across_reopen() {
        let dir = agent_testkit::tempdir();
        let path = dir.join("registry.sqlite3");
        {
            let reg = SqliteRegistry::open(&path).expect("open");
            reg.put(card("kimi")).await.expect("put");
            reg.put_policy(config().policy).await.expect("policy");
        }
        let reg = SqliteRegistry::open(&path).expect("reopen");
        assert_eq!(reg.get("kimi").await.expect("get"), card("kimi"));
        assert_eq!(reg.get_policy().await.expect("policy"), config().policy);
    }

    #[tokio::test]
    async fn positive_sqlite_and_memory_stores_agree() {
        let sql = seeded().await;
        let mem = crate::MemoryRegistry::new(config()).expect("valid");
        let hint = RouteHint {
            role: Some(RouteRole::Judge),
            ..Default::default()
        };
        assert_eq!(
            sql.route(&hint).await.unwrap(),
            mem.route(&hint).await.unwrap()
        );
        assert_eq!(sql.list().await.unwrap(), mem.list().await.unwrap());
    }

    #[tokio::test]
    async fn negative_delete_unknown_false_enable_unknown_not_found() {
        let reg = SqliteRegistry::open_in_memory().expect("open");
        assert!(!reg.delete("ghost").await.expect("delete"));
        let err = reg.enable("ghost", true).await.expect_err("enable");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn adversarial_bound_parameters_make_metachar_ids_inert() {
        // A hostile id is rejected by `check_id` long before SQL — but even the
        // storage layer only ever binds parameters, so verify the store state
        // stays intact after rejected attempts.
        let reg = seeded().await;
        for id in ["a'; DROP TABLE upstreams; --", "../x", ""] {
            assert!(reg.get(id).await.is_err());
        }
        assert_eq!(reg.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn adversarial_out_of_band_row_tamper_fails_closed() {
        let reg = seeded().await;
        {
            let conn = reg.conn.lock().unwrap();
            // Replace a stored card with a blob that decodes to a traversal id.
            let evil = pb::Upstream {
                id: "../escape".into(),
                ..Default::default()
            }
            .encode_to_vec();
            conn.execute(
                "UPDATE upstreams SET card = ?1 WHERE id = 'kimi'",
                params![evil],
            )
            .unwrap();
        }
        assert!(reg.list().await.is_err(), "tampered row must fail closed");
    }
}
