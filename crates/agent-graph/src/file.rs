//! The file-backed [`GraphStore`]: one textproto document on disk (the
//! `--cognition-graph <file>` form). Reads re-parse and re-validate every time
//! — the file may be hand-edited out of band, and an invalid document must
//! fail closed at the seam, never reach the executor. Writes are
//! validate-then-persist via a same-directory temp file + atomic rename, so a
//! crash mid-write can never leave a half document behind.

use std::path::{Path, PathBuf};

use agent_core::{
    Error, GraphDoc, GraphIssue, GraphStore, NodeTypeSchema, Result, MAX_GRAPH_DOC_BYTES,
};
use async_trait::async_trait;

use crate::schema::NodeTypeRegistry;
use crate::{textproto, validate};

pub struct FileGraphs {
    path: PathBuf,
    registry: NodeTypeRegistry,
}

impl FileGraphs {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            registry: NodeTypeRegistry::builtin(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Result<GraphDoc> {
        // Size-gate on metadata before reading — the parse layer caps again,
        // but a huge file should not even be buffered.
        let meta = std::fs::metadata(&self.path)
            .map_err(|e| Error::Graph(format!("graph document `{}`: {e}", self.path.display())))?;
        if meta.len() > MAX_GRAPH_DOC_BYTES as u64 {
            return Err(Error::Graph(format!(
                "graph document `{}` is {} bytes (cap {MAX_GRAPH_DOC_BYTES})",
                self.path.display(),
                meta.len()
            )));
        }
        let text = std::fs::read_to_string(&self.path)
            .map_err(|e| Error::Graph(format!("graph document `{}`: {e}", self.path.display())))?;
        let doc = textproto::parse(&text)?;
        reject_invalid(validate::validate(&doc, &self.registry))?;
        Ok(doc)
    }
}

/// Wholesale reject on any issue: the summary line carries the first few typed
/// findings so a CLI user sees what to fix without a separate `Validate` call.
fn reject_invalid(issues: Vec<GraphIssue>) -> Result<()> {
    if issues.is_empty() {
        return Ok(());
    }
    let mut shown: Vec<String> = issues
        .iter()
        .take(3)
        .map(|i| format!("[{}] {}: {}", i.code.as_str(), i.node, i.detail))
        .collect();
    if issues.len() > shown.len() {
        shown.push(format!("… and {} more", issues.len() - shown.len()));
    }
    Err(Error::Graph(format!(
        "invalid graph document ({} issues): {}",
        issues.len(),
        shown.join("; ")
    )))
}

#[async_trait]
impl GraphStore for FileGraphs {
    async fn get(&self) -> Result<GraphDoc> {
        self.load()
    }

    async fn put(&self, doc: GraphDoc) -> Result<()> {
        reject_invalid(validate::validate(&doc, &self.registry))?;
        let text = textproto::print(&doc)?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        // Same-directory temp + rename: atomic on POSIX, and the temp name is
        // ours (derived from the target), not attacker-influenced.
        let tmp = self.path.with_extension("textproto.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            Error::Graph(format!("persist graph `{}`: {e}", self.path.display()))
        })?;
        Ok(())
    }

    async fn validate(&self, doc: &GraphDoc) -> Result<Vec<GraphIssue>> {
        Ok(validate::validate(doc, &self.registry))
    }

    async fn node_types(&self) -> Result<Vec<NodeTypeSchema>> {
        Ok(self.registry.schemas())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;
    use agent_testkit::tempdir;

    fn store_in(dir: &Path) -> FileGraphs {
        FileGraphs::new(dir.join("graph.textproto"))
    }

    #[tokio::test]
    async fn positive_put_then_get_roundtrips() {
        let dir = tempdir();
        let store = store_in(&dir);
        let doc = testdata::intermediate();
        store.put(doc.clone()).await.expect("put");
        assert_eq!(store.get().await.expect("get"), doc);
        // The on-disk form is the diffable textproto, not JSON/binary.
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("critic_gate"), "{text}");
    }

    #[tokio::test]
    async fn negative_put_of_invalid_document_rejected_and_not_persisted() {
        let dir = tempdir();
        let store = store_in(&dir);
        let (_, bad) = testdata::invalid_docs().remove(0);
        let err = store.put(bad).await.expect_err("rejected");
        assert!(err.to_string().contains("invalid graph document"), "{err}");
        assert!(!store.path().exists(), "nothing persisted");
    }

    #[tokio::test]
    async fn negative_get_missing_file_errors() {
        let dir = tempdir();
        assert!(store_in(&dir).get().await.is_err());
    }

    #[tokio::test]
    async fn corner_out_of_band_edit_is_revalidated_on_read() {
        // A valid file later hand-edited into an invalid one fails closed at
        // the next read, not at the executor.
        let dir = tempdir();
        let store = store_in(&dir);
        store.put(testdata::simple()).await.unwrap();
        let mut text = std::fs::read_to_string(store.path()).unwrap();
        text = text.replace("critic_gate", "quantum_oracle");
        std::fs::write(store.path(), text).unwrap();
        let err = store.get().await.expect_err("re-validated");
        assert!(err.to_string().contains("unknown_node_type"), "{err}");
    }

    #[tokio::test]
    async fn adversarial_oversized_file_refused_on_metadata() {
        let dir = tempdir();
        let store = store_in(&dir);
        std::fs::write(store.path(), testdata::textproto_bomb()).unwrap();
        let err = store.get().await.expect_err("size cap");
        assert!(err.to_string().contains("cap"), "{err}");
    }

    #[tokio::test]
    async fn positive_validate_reports_without_erroring() {
        let dir = tempdir();
        let store = store_in(&dir);
        for (code, doc) in testdata::invalid_docs() {
            let issues = store.validate(&doc).await.expect("validate never errors");
            assert!(issues.iter().any(|i| i.code == code), "{code:?}");
        }
        assert!(store
            .validate(&testdata::simple())
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn positive_node_types_lists_the_builtin_palette() {
        let dir = tempdir();
        let types = store_in(&dir).node_types().await.unwrap();
        assert_eq!(types.len(), 9);
        assert!(types.iter().any(|t| t.node_type == "critic_gate"));
    }
}
