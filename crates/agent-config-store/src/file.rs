//! `FileBackend` — the bootstrap/dev [`Backend`]: the whole store as one JSON
//! bundle on disk, rewritten atomically (same-directory temp + `rename`).
//!
//! Reads re-parse the bundle every time (it may be edited out of band) with a
//! size cap; an **absent** file is the empty store (the first write creates it);
//! a *present but unparseable* file is an error on every operation — never a
//! partially-loaded store. Writes are one serialized read-modify-write cycle
//! (a `Mutex<()>` guards the cycle; the `rename` is what makes it atomic).
//!
//! Unlike a per-domain textproto file, this generic tier stores **opaque card
//! blobs** (fidelity across any codec) — the human-editable textproto stays each
//! domain's own file backend, preserved when they converge (increment A3*).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agent_core::{Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{check_batch, Backend, Write};

/// Size cap on the on-disk bundle, checked before parse (a hostile/oversized
/// file must not be buffered unboundedly).
pub const MAX_BUNDLE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct Bundle {
    #[serde(default)]
    tenants: Vec<String>,
    #[serde(default)]
    cards: Vec<Row>,
}

#[derive(Serialize, Deserialize)]
struct Row {
    collection: String,
    tenant: String,
    id: String,
    pos: u64,
    blob: Vec<u8>,
}

/// A file-backed config store.
pub struct FileBackend {
    path: PathBuf,
    /// Serializes read-modify-write cycles (the `rename` is atomic; the cycle
    /// is not) so two concurrent mutations cannot lose an update.
    write: Mutex<()>,
}

impl FileBackend {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load + size-check the bundle; an absent file is the empty store.
    fn load(&self) -> Result<Bundle> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Bundle::default()),
            Err(e) => {
                return Err(Error::Config(format!(
                    "config store `{}`: {e}",
                    self.path.display()
                )))
            }
        };
        if text.len() > MAX_BUNDLE_BYTES {
            return Err(Error::Config(format!(
                "config store `{}` is {} bytes (cap {MAX_BUNDLE_BYTES})",
                self.path.display(),
                text.len()
            )));
        }
        serde_json::from_str(&text)
            .map_err(|e| Error::Config(format!("config store `{}`: {e}", self.path.display())))
    }

    /// Persist atomically (same-directory temp + rename; the temp name is
    /// derived from ours, not attacker-influenced).
    fn persist(&self, bundle: &Bundle) -> Result<()> {
        let text = serde_json::to_string_pretty(bundle)?;
        // Symmetric with load()'s cap: a write must never grow the bundle past what
        // a read accepts. Otherwise the next load() — which EVERY op runs, get/list/
        // count/tenants and apply() itself (including a Delete meant to prune) —
        // fails on size, wedging the ENTIRE shared store unrecoverably (a plain
        // `cargo build --serve-*` fleet reaches this simply by the scheduler
        // retaining MAX_HISTORY×MAX_DETAIL per job across normal ticks). Reject the
        // over-cap write instead: it fails here BEFORE the temp/rename, so the
        // on-disk bundle stays at its last-good state and fully usable.
        if text.len() > MAX_BUNDLE_BYTES {
            return Err(Error::Config(format!(
                "config store `{}`: write rejected — bundle would be {} bytes (cap {MAX_BUNDLE_BYTES})",
                self.path.display(),
                text.len()
            )));
        }
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::Config(format!("persist `{}`: {e}", self.path.display()))
        })?;
        Ok(())
    }
}

#[async_trait]
impl Backend for FileBackend {
    async fn get(&self, collection: &str, tenant: &str, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .load()?
            .cards
            .into_iter()
            .find(|r| r.collection == collection && r.tenant == tenant && r.id == id)
            .map(|r| r.blob))
    }

    async fn list(&self, collection: &str, tenant: &str) -> Result<Vec<Vec<u8>>> {
        let mut rows: Vec<Row> = self
            .load()?
            .cards
            .into_iter()
            .filter(|r| r.collection == collection && r.tenant == tenant)
            .collect();
        rows.sort_by_key(|r| r.pos);
        Ok(rows.into_iter().map(|r| r.blob).collect())
    }

    async fn count(&self, collection: &str, tenant: &str) -> Result<usize> {
        Ok(self
            .load()?
            .cards
            .iter()
            .filter(|r| r.collection == collection && r.tenant == tenant)
            .count())
    }

    async fn tenants(&self, collection: &str) -> Result<Vec<String>> {
        let set: std::collections::BTreeSet<String> = self
            .load()?
            .cards
            .into_iter()
            .filter(|r| r.collection == collection)
            .map(|r| r.tenant)
            .collect();
        Ok(set.into_iter().collect())
    }

    async fn apply(&self, writes: &[Write]) -> Result<()> {
        let _guard = self
            .write
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut bundle = self.load()?;
        check_batch(writes, |t| bundle.tenants.iter().any(|x| x == t))?;
        let mut next_pos = bundle
            .cards
            .iter()
            .map(|r| r.pos)
            .max()
            .map_or(0, |m| m + 1);
        for w in writes {
            match w {
                Write::EnsureTenant { tenant } => {
                    if !bundle.tenants.iter().any(|x| x == tenant) {
                        bundle.tenants.push(tenant.clone());
                    }
                }
                Write::Put {
                    collection,
                    tenant,
                    id,
                    blob,
                } => {
                    match bundle
                        .cards
                        .iter_mut()
                        .find(|r| r.collection == *collection && r.tenant == *tenant && r.id == *id)
                    {
                        Some(row) => row.blob.clone_from(blob),
                        None => {
                            bundle.cards.push(Row {
                                collection: collection.to_string(),
                                tenant: tenant.clone(),
                                id: id.clone(),
                                pos: next_pos,
                                blob: blob.clone(),
                            });
                            next_pos += 1;
                        }
                    }
                }
                Write::Delete {
                    collection,
                    tenant,
                    id,
                } => {
                    bundle.cards.retain(|r| {
                        !(r.collection == *collection && r.tenant == *tenant && r.id == *id)
                    });
                }
            }
        }
        self.persist(&bundle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> FileBackend {
        FileBackend::new(agent_testkit::tempdir().join("store.json"))
    }

    fn ensure(tenant: &str) -> Write {
        Write::EnsureTenant {
            tenant: tenant.into(),
        }
    }
    fn put(id: &str, blob: Vec<u8>) -> Write {
        Write::Put {
            collection: "c",
            tenant: "t".into(),
            id: id.into(),
            blob,
        }
    }

    /// A normal under-cap write round-trips through the on-disk bundle.
    #[tokio::test]
    async fn positive_under_cap_write_round_trips() {
        let be = backend();
        be.apply(&[ensure("t"), put("a", b"hi".to_vec())])
            .await
            .unwrap();
        assert_eq!(be.get("c", "t", "a").await.unwrap(), Some(b"hi".to_vec()));
    }

    /// The R8-4 fix: a write that would push the serialized bundle past the cap is
    /// REJECTED at persist time — and, critically, the store is NOT wedged. The
    /// prior value still reads and a further small write still applies (before the
    /// fix, the oversize bundle landed on disk and every later load() failed).
    #[tokio::test]
    async fn adversarial_oversize_write_is_rejected_and_store_stays_usable() {
        let be = backend();
        be.apply(&[ensure("t"), put("a", b"hi".to_vec())])
            .await
            .unwrap();

        // A single blob at the byte cap serializes (as a JSON number array) to far
        // more than MAX_BUNDLE_BYTES, so persist() must reject the write.
        let huge = vec![0u8; MAX_BUNDLE_BYTES];
        let err = be.apply(&[put("big", huge)]).await;
        assert!(err.is_err(), "over-cap write is rejected");

        // Store still usable: the earlier card reads, and a new small write applies.
        assert_eq!(be.get("c", "t", "a").await.unwrap(), Some(b"hi".to_vec()));
        be.apply(&[put("b", b"ok".to_vec())]).await.unwrap();
        assert_eq!(be.get("c", "t", "b").await.unwrap(), Some(b"ok".to_vec()));
    }
}
