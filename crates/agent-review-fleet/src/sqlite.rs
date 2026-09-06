//! `SqliteFleet` — the embedded-SQLite [`FleetRegistry`] backend, behind the
//! non-default `fleet-sqlite` feature (mirrors `agent-registry`'s `registry-sqlite`).
//! Each roster row is stored as its **JSON form** in a BLOB — the same at-rest shape
//! the file backend uses, so the two can never drift.
//!
//! **Interchangeable with the file/memory backends**: every mutation routes through
//! the same shared `ops` (validation, clamps, caps), and reads re-run the JSON decode
//! and `validate` (via `ops::revalidate`) — a row edited out of band fails closed at
//! the seam. Ids reach SQL only as **bound parameters**.

use std::path::Path;
use std::sync::Mutex;

use agent_core::{Error, FleetRegistry, FleetSession, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection};

use crate::{check_id, not_found, ops};

/// A SQLite-backed [`FleetRegistry`]. The connection is wrapped in a `Mutex`
/// (rusqlite's `Connection` is `Send` but `!Sync`); every method locks it for a
/// short, synchronous query and never holds the guard across an `.await` — a
/// low-traffic control-plane surface, not the hot loop.
pub struct SqliteFleet {
    conn: Mutex<Connection>,
}

impl SqliteFleet {
    /// Open (creating if absent) the roster at `path` and ensure the schema.
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
            "CREATE TABLE IF NOT EXISTS fleet (
                 id      TEXT NOT NULL PRIMARY KEY,
                 pos     INTEGER NOT NULL,
                 enabled INTEGER NOT NULL,
                 data    BLOB NOT NULL
             );",
        )
        .map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Snapshot the whole roster (JSON decode + `validate` fail closed on out-of-band
    /// tampering). `pos` preserves insertion order so listing order matches the
    /// file/memory backends.
    fn load(&self, conn: &Connection) -> Result<Vec<FleetSession>> {
        let mut stmt = conn
            .prepare("SELECT data FROM fleet ORDER BY pos, id")
            .map_err(sql_err)?;
        let rows: Vec<FleetSession> = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(sql_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)?
            .into_iter()
            .map(|blob| {
                serde_json::from_slice::<FleetSession>(&blob)
                    .map_err(|e| Error::Fleet(format!("stored fleet row decode: {e}")))
            })
            .collect::<Result<_>>()?;
        ops::revalidate(&rows)?;
        Ok(rows)
    }

    /// One serialized read-modify-write cycle: snapshot → shared op → rewrite.
    /// Rewriting the (≤512-row) table inside a transaction keeps the three backends
    /// byte-for-byte interchangeable in behaviour without a parallel per-row mutation
    /// path that could drift from `ops`.
    fn mutate<T>(&self, f: impl FnOnce(&mut Vec<FleetSession>) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let conn = &mut *guard;
        let mut rows = self.load(conn)?;
        let out = f(&mut rows)?;
        ops::revalidate(&rows)?;
        let tx = conn.transaction().map_err(sql_err)?;
        tx.execute("DELETE FROM fleet", []).map_err(sql_err)?;
        for (pos, r) in rows.iter().enumerate() {
            let blob = serde_json::to_vec(r)
                .map_err(|e| Error::Fleet(format!("serialize fleet row: {e}")))?;
            tx.execute(
                "INSERT INTO fleet (id, pos, enabled, data) VALUES (?1, ?2, ?3, ?4)",
                params![r.id, pos as i64, i64::from(r.enabled), blob],
            )
            .map_err(sql_err)?;
        }
        tx.commit().map_err(sql_err)?;
        Ok(out)
    }

    fn snapshot(&self) -> Result<Vec<FleetSession>> {
        let guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.load(&guard)
    }
}

fn sql_err(e: rusqlite::Error) -> Error {
    Error::Fleet(format!("sqlite: {e}"))
}

#[async_trait]
impl FleetRegistry for SqliteFleet {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        self.snapshot()
    }
    async fn get(&self, id: &str) -> Result<FleetSession> {
        check_id(id)?;
        self.snapshot()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| not_found(id))
    }
    async fn put(&self, session: FleetSession) -> Result<FleetSession> {
        self.mutate(|rows| ops::put(rows, session))
    }
    async fn delete(&self, id: &str) -> Result<bool> {
        self.mutate(|rows| ops::delete(rows, id))
    }
    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<FleetSession> {
        self.mutate(|rows| ops::set_enabled(rows, id, enabled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata::row;

    async fn seeded() -> SqliteFleet {
        let store = SqliteFleet::open_in_memory().expect("open");
        store.put(row("r1")).await.expect("seed r1");
        store.put(row("r2")).await.expect("seed r2");
        store
    }

    #[tokio::test]
    async fn positive_put_get_roundtrips_through_blobs() {
        let store = SqliteFleet::open_in_memory().expect("open");
        store.put(row("r1")).await.expect("put");
        assert_eq!(store.get("r1").await.expect("get"), row("r1"));
    }

    #[tokio::test]
    async fn positive_sqlite_survives_reopen() {
        let dir = agent_testkit::tempdir();
        let path = dir.join("review-fleet.sqlite3");
        {
            let store = SqliteFleet::open(&path).expect("open");
            store.put(row("r1")).await.expect("put");
            store.set_enabled("r1", false).await.expect("disable");
        }
        let store = SqliteFleet::open(&path).expect("reopen");
        let got = store.get("r1").await.expect("get after reopen");
        assert!(!got.enabled, "the toggle persisted across reopen");
    }

    #[tokio::test]
    async fn positive_sqlite_and_memory_stores_agree() {
        let sql = seeded().await;
        let mem = crate::MemoryFleet::new();
        mem.put(row("r1")).await.unwrap();
        mem.put(row("r2")).await.unwrap();
        assert_eq!(sql.list().await.unwrap(), mem.list().await.unwrap());
    }

    #[tokio::test]
    async fn negative_delete_unknown_false_set_enabled_unknown_not_found() {
        let store = SqliteFleet::open_in_memory().expect("open");
        assert!(!store.delete("ghost").await.expect("delete"));
        let err = store
            .set_enabled("ghost", true)
            .await
            .expect_err("set_enabled");
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn adversarial_bound_parameters_make_metachar_ids_inert() {
        // A hostile id is rejected by `check_id` long before SQL — but even the
        // storage layer only ever binds parameters, so verify the store state stays
        // intact after rejected attempts.
        let store = seeded().await;
        for id in ["a'; DROP TABLE fleet; --", "../x", ""] {
            assert!(store.get(id).await.is_err());
        }
        assert_eq!(store.list().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn adversarial_out_of_band_row_tamper_fails_closed() {
        let store = seeded().await;
        {
            let conn = store.conn.lock().unwrap();
            // Replace a stored row with JSON that decodes to a traversal id.
            let evil = serde_json::to_vec(&FleetSession {
                id: "../escape".into(),
                ..Default::default()
            })
            .unwrap();
            conn.execute("UPDATE fleet SET data = ?1 WHERE id = 'r1'", params![evil])
                .unwrap();
        }
        assert!(store.list().await.is_err(), "tampered row must fail closed");
    }
}
