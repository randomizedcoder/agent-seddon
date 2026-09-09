//! `SqliteBackend` — the embedded-SQLite [`Backend`], behind the non-default
//! `config-store-sqlite` feature (the DB is compiled in via `rusqlite`
//! `bundled`, so this tier is hermetic in `nix flake check`).
//!
//! One `cards(collection, tenant, id, pos, blob)` table keyed by
//! `(collection, tenant, id)` with a foreign key to `tenants(tenant)` and
//! `PRAGMA foreign_keys = ON` — so a card can never reference a missing tenant,
//! and a whole batch commits or rolls back as one `rusqlite` transaction.
//! **Ids/tenants reach SQL only as bound parameters**; a card field is inside
//! the opaque blob, so `'; DROP TABLE …` in a field is inert.
//!
//! The connection is a `Mutex<Connection>` (rusqlite's `Connection` is `Send`
//! but `!Sync`); every method locks it for a short synchronous query and never
//! holds the guard across an `.await` — a low-traffic control-plane surface.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use agent_core::{Error, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection};

use crate::{check_batch, Backend, Write};

/// A SQLite-backed config store.
pub struct SqliteBackend {
    conn: Mutex<Connection>,
}

fn sql_err(e: rusqlite::Error) -> Error {
    Error::Config(format!("sqlite: {e}"))
}

impl SqliteBackend {
    /// Open (creating if absent) the store at `path` and ensure the schema.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_conn(Connection::open(path).map_err(sql_err)?)
    }

    /// An in-memory database (tests).
    pub fn open_in_memory() -> Result<Self> {
        Self::from_conn(Connection::open_in_memory().map_err(sql_err)?)
    }

    fn from_conn(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS tenants (
                 tenant TEXT NOT NULL PRIMARY KEY
             );
             CREATE TABLE IF NOT EXISTS cards (
                 collection TEXT NOT NULL,
                 tenant     TEXT NOT NULL,
                 id         TEXT NOT NULL,
                 pos        INTEGER NOT NULL,
                 blob       BLOB NOT NULL,
                 PRIMARY KEY (collection, tenant, id),
                 FOREIGN KEY (tenant) REFERENCES tenants(tenant)
             );",
        )
        .map_err(sql_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait]
impl Backend for SqliteBackend {
    async fn get(&self, collection: &str, tenant: &str, id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        conn.query_row(
            "SELECT blob FROM cards WHERE collection = ?1 AND tenant = ?2 AND id = ?3",
            params![collection, tenant, id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(sql_err(e)),
        })
    }

    async fn list(&self, collection: &str, tenant: &str) -> Result<Vec<Vec<u8>>> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn
            .prepare(
                "SELECT blob FROM cards WHERE collection = ?1 AND tenant = ?2 ORDER BY pos, id",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(params![collection, tenant], |row| row.get::<_, Vec<u8>>(0))
            .map_err(sql_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)?;
        Ok(rows)
    }

    async fn count(&self, collection: &str, tenant: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM cards WHERE collection = ?1 AND tenant = ?2",
                params![collection, tenant],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        Ok(n.max(0) as usize)
    }

    async fn apply(&self, writes: &[Write]) -> Result<()> {
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Snapshot existing tenants so the fail-closed check and the writes are
        // one atomic unit (the whole method holds the connection lock).
        let existing: HashSet<String> = {
            let mut stmt = guard
                .prepare("SELECT tenant FROM tenants")
                .map_err(sql_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(sql_err)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sql_err)?;
            rows.into_iter().collect()
        };
        check_batch(writes, |t| existing.contains(t))?;

        let tx = guard.transaction().map_err(sql_err)?;
        for w in writes {
            match w {
                Write::EnsureTenant { tenant } => {
                    tx.execute(
                        "INSERT OR IGNORE INTO tenants (tenant) VALUES (?1)",
                        params![tenant],
                    )
                    .map_err(sql_err)?;
                }
                Write::Put {
                    collection,
                    tenant,
                    id,
                    blob,
                } => {
                    // New rows get the next global pos (insertion order); an
                    // existing row keeps its pos and only swaps the blob.
                    tx.execute(
                        "INSERT INTO cards (collection, tenant, id, pos, blob)
                         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(pos), -1) + 1 FROM cards), ?4)
                         ON CONFLICT (collection, tenant, id) DO UPDATE SET blob = excluded.blob",
                        params![collection, tenant, id, blob],
                    )
                    .map_err(sql_err)?;
                }
                Write::Delete {
                    collection,
                    tenant,
                    id,
                } => {
                    tx.execute(
                        "DELETE FROM cards WHERE collection = ?1 AND tenant = ?2 AND id = ?3",
                        params![collection, tenant, id],
                    )
                    .map_err(sql_err)?;
                }
            }
        }
        tx.commit().map_err(sql_err)?;
        Ok(())
    }
}
