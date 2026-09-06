//! The file-backed [`FleetRegistry`]: one JSON bundle on disk (a `Vec<FleetSession>`)
//! — hand-editable *or* rewritten by a control-plane `Put` (one format for both jobs;
//! mirrors `agent-registry`'s `FileRegistry`).
//!
//! Reads re-parse and re-validate every row every time — the file may be hand-edited
//! out of band, and an invalid bundle must fail closed at the seam. An **absent** file
//! is an *empty roster* (a fresh control plane starts with no sessions and the first
//! `Put` creates the bundle); a *present but invalid* file (bad JSON, oversized, or a
//! row that fails validation) is an error on every operation — never a partially-loaded
//! roster. Writes are validate-then-persist via a same-directory temp file + atomic
//! rename.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use agent_core::{Error, FleetRegistry, FleetSession, Result};
use async_trait::async_trait;

use crate::{check_id, not_found, ops};

/// Size cap on the JSON bundle, applied before parsing (defense against a hostile
/// or corrupt file — 512 rows of a few hundred bytes each fits comfortably under it).
const MAX_FLEET_BUNDLE_BYTES: usize = 4 * 1024 * 1024;

pub struct FileFleet {
    path: PathBuf,
    /// Serialises read-modify-write cycles so two concurrent mutations cannot lose
    /// an update (the rename itself is atomic; the cycle is not).
    write: Mutex<()>,
}

impl FileFleet {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load + re-validate the bundle; an absent file is the empty roster.
    fn load(&self) -> Result<Vec<FleetSession>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(Error::Fleet(format!(
                    "fleet roster `{}`: {e}",
                    self.path.display()
                )))
            }
        };
        if text.len() > MAX_FLEET_BUNDLE_BYTES {
            return Err(Error::Fleet(format!(
                "fleet roster `{}` is {} bytes (cap {MAX_FLEET_BUNDLE_BYTES})",
                self.path.display(),
                text.len()
            )));
        }
        let rows: Vec<FleetSession> = serde_json::from_str(&text)
            .map_err(|e| Error::Fleet(format!("fleet roster `{}`: {e}", self.path.display())))?;
        ops::revalidate(&rows)?;
        Ok(rows)
    }

    /// Validate-then-persist atomically (same-directory temp + rename; the temp name
    /// is derived from ours, not attacker-influenced).
    fn persist(&self, rows: &[FleetSession]) -> Result<()> {
        ops::revalidate(rows)?;
        let text = serde_json::to_string_pretty(rows)
            .map_err(|e| Error::Fleet(format!("serialize fleet roster: {e}")))?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::Fleet(format!("persist `{}`: {e}", self.path.display()))
        })?;
        Ok(())
    }

    /// One serialized read-modify-write cycle.
    fn mutate<T>(&self, f: impl FnOnce(&mut Vec<FleetSession>) -> Result<T>) -> Result<T> {
        let _guard = self
            .write
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut rows = self.load()?;
        let out = f(&mut rows)?;
        self.persist(&rows)?;
        Ok(out)
    }
}

#[async_trait]
impl FleetRegistry for FileFleet {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        self.load()
    }
    async fn get(&self, id: &str) -> Result<FleetSession> {
        check_id(id)?;
        self.load()?
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
    use agent_testkit::tempdir;

    fn store_in(dir: &Path) -> FileFleet {
        FileFleet::new(dir.join("review-fleet.json"))
    }

    #[tokio::test]
    async fn positive_put_creates_the_bundle_and_roundtrips() {
        let dir = tempdir();
        let store = store_in(&dir);
        store.put(row("r1")).await.expect("put");
        assert_eq!(store.get("r1").await.expect("get"), row("r1"));
        // The on-disk form is diffable JSON that carries the token REFERENCE only.
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("env:FLEET_TOKEN"), "{text}");
        assert!(text.contains("\"backend\": \"github\""), "{text}");
    }

    #[tokio::test]
    async fn corner_absent_file_is_an_empty_roster() {
        let dir = tempdir();
        let store = store_in(&dir);
        assert!(store.list().await.expect("empty list").is_empty());
        assert!(!store.delete("ghost").await.expect("delete on empty"));
        assert!(store.get("ghost").await.is_err());
    }

    #[tokio::test]
    async fn corner_out_of_band_edit_is_revalidated_on_read() {
        let dir = tempdir();
        let store = store_in(&dir);
        store.put(row("r1")).await.unwrap();
        // Hand-edit an id into a traversal — every subsequent op fails closed.
        let text = std::fs::read_to_string(store.path()).unwrap();
        std::fs::write(store.path(), text.replace("\"r1\"", "\"../r1\"")).unwrap();
        assert!(
            store.list().await.is_err(),
            "tampered id must fail the read"
        );
        assert!(
            store.put(row("r2")).await.is_err(),
            "no write over a bad bundle"
        );
    }

    #[tokio::test]
    async fn negative_rejected_put_leaves_the_bundle_untouched() {
        let dir = tempdir();
        let store = store_in(&dir);
        store.put(row("r1")).await.unwrap();
        let before = std::fs::read_to_string(store.path()).unwrap();
        let mut bad = row("r2");
        bad.token_ref = "ghp_raw".into();
        assert!(store.put(bad).await.is_err());
        assert_eq!(std::fs::read_to_string(store.path()).unwrap(), before);
    }

    #[tokio::test]
    async fn adversarial_oversized_file_refused() {
        let dir = tempdir();
        let store = store_in(&dir);
        std::fs::write(
            store.path(),
            format!("[{}]", "\"x\",".repeat(MAX_FLEET_BUNDLE_BYTES)),
        )
        .unwrap();
        let err = store.list().await.expect_err("size cap");
        assert!(err.to_string().contains("cap"), "{err}");
    }

    #[tokio::test]
    async fn positive_file_and_memory_stores_agree() {
        // The storage backends are interchangeable: same ops, same answers.
        let dir = tempdir();
        let file = store_in(&dir);
        let mem = crate::MemoryFleet::new();
        for store in [&file as &dyn FleetRegistry, &mem] {
            store.put(row("r1")).await.unwrap();
            store.put(row("r2")).await.unwrap();
            store.set_enabled("r2", false).await.unwrap();
        }
        assert_eq!(file.list().await.unwrap(), mem.list().await.unwrap());
        assert_eq!(file.get("r2").await.unwrap(), mem.get("r2").await.unwrap());
    }
}
