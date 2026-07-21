//! `VectorBackend` — a semantic `SearchBackend` over the `Embedder` seam. It
//! embeds each file and answers a `Semantic` query by **exact** brute-force cosine
//! similarity (deterministic, trivially correct, fine for repo-sized corpora; an
//! ANN index is a capability-gated follow-up). Freshness/incremental reindex reuse
//! the same [`Manifest`] machinery as the tantivy backend.

use crate::manifest::{self, Manifest};
use agent_core::{
    cosine_similarity, Embedder, Error, IndexState, IndexStatus, ProgressFn, Result, SearchBackend,
    SearchCapabilities, SearchHit, SearchMode, SearchQuery,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct VectorBackend {
    root: PathBuf,
    index_dir: PathBuf,
    embedder: Arc<dyn Embedder>,
    /// repo-relative path → embedding vector.
    vectors: Mutex<BTreeMap<PathBuf, Vec<f32>>>,
}

impl VectorBackend {
    pub fn new(
        root: impl Into<PathBuf>,
        index_dir: impl Into<PathBuf>,
        embedder: Arc<dyn Embedder>,
    ) -> Self {
        let index_dir = index_dir.into();
        let vectors = load_vectors(&index_dir.join("vectors.json")).unwrap_or_default();
        Self {
            root: root.into(),
            index_dir,
            embedder,
            vectors: Mutex::new(vectors),
        }
    }

    fn manifest_path(&self) -> PathBuf {
        self.index_dir.join("vector-manifest.json")
    }
    fn vectors_path(&self) -> PathBuf {
        self.index_dir.join("vectors.json")
    }
}

#[async_trait]
impl SearchBackend for VectorBackend {
    fn capabilities(&self) -> SearchCapabilities {
        SearchCapabilities {
            backend: "vector".into(),
            modes: vec![SearchMode::Semantic],
            content_search: true,
            scored: true,
            incremental: true,
            max_concurrent_queries: 0,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        let stored = Manifest::load(&self.manifest_path());
        let root = self.root.clone();
        let stored_for_cmp = stored.clone();
        let state =
            tokio::task::spawn_blocking(move || manifest::compare(&root, stored_for_cmp.as_ref()))
                .await
                .map_err(|e| Error::Search(format!("scan task panicked: {e}")))?;
        let n = self.vectors.lock().expect("vectors poisoned").len() as u64;
        Ok(IndexStatus {
            state,
            indexed_files: n,
            last_indexed_ms: stored.as_ref().map(|m| m.built_ms).unwrap_or(0),
            manifest_digest: stored.map(|m| m.digest()).unwrap_or_default(),
        })
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        let root = self.root.clone();
        let current = tokio::task::spawn_blocking(move || Manifest::scan(&root))
            .await
            .map_err(|e| Error::Search(format!("scan task panicked: {e}")))?;
        let stored = Manifest::load(&self.manifest_path());
        let (upserts, deletes) = diff(stored.as_ref(), &current);

        // Read the changed files' contents (skip unreadable/binary).
        let mut paths = Vec::new();
        let mut texts = Vec::new();
        for rel in &upserts {
            if let Ok(content) = std::fs::read_to_string(self.root.join(rel)) {
                paths.push(rel.clone());
                texts.push(content);
            }
        }
        // Embed in batches (off the lock — this is the async part).
        let dims = self.embedder.dimensions();
        let mut vecs = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(self.embedder.max_batch().max(1)) {
            let embedded = self.embedder.embed_docs(chunk).await?;
            for v in embedded {
                if v.len() != dims {
                    return Err(Error::Search(format!(
                        "embedder produced a {}-dim vector, expected {dims} (dimension mismatch)",
                        v.len()
                    )));
                }
                vecs.push(v);
            }
        }

        let total = paths.len() as u64;
        {
            let mut store = self.vectors.lock().expect("vectors poisoned");
            for rel in &deletes {
                store.remove(rel);
            }
            for (rel, v) in paths.iter().zip(vecs) {
                store.insert(rel.clone(), v);
            }
            save_vectors(&self.vectors_path(), &store)?;
        }
        current.save(&self.manifest_path())?;
        progress(agent_core::ReindexProgress {
            files_done: total,
            files_total: total,
            done: true,
        });

        Ok(IndexStatus {
            state: IndexState::Fresh,
            indexed_files: self.vectors.lock().expect("vectors poisoned").len() as u64,
            last_indexed_ms: current.built_ms,
            manifest_digest: current.digest(),
        })
    }

    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        if q.text.trim().is_empty() {
            return Ok(Vec::new()); // empty query ⇒ no matches (not an error)
        }
        let qv = self.embedder.embed_query(&q.text).await?;

        let store = self.vectors.lock().expect("vectors poisoned");
        // Config-drift guard: a stored index at a different dimensionality than the
        // (swapped) query embedder is a clear error, not silent empty results.
        if let Some((_, first)) = store.iter().next() {
            if first.len() != qv.len() {
                return Err(Error::Search(format!(
                    "index built at {} dims, query embedder is {} dims (dimension mismatch — reindex after a model swap)",
                    first.len(),
                    qv.len()
                )));
            }
        }
        let mut scored: Vec<(PathBuf, f32)> = store
            .iter()
            .filter(|(p, _)| path_globs_ok(p, &q.path_globs))
            .map(|(p, v)| (p.clone(), cosine_similarity(&qv, v)))
            .collect();
        // Score desc, deterministic tie-break by path.
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(q.limit.max(1));
        Ok(scored
            .into_iter()
            .map(|(path, score)| SearchHit {
                path,
                line: 0,
                col_start: 0,
                col_end: 0,
                score,
                snippet: String::new(),
            })
            .collect())
    }

    async fn list_files(&self, globs: &[String]) -> Result<Vec<PathBuf>> {
        let store = self.vectors.lock().expect("vectors poisoned");
        Ok(store
            .keys()
            .filter(|p| path_globs_ok(p, globs))
            .cloned()
            .collect())
    }
}

/// Paths to (re)embed and paths to delete (mirrors the tantivy backend's diff).
fn diff(stored: Option<&Manifest>, current: &Manifest) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Some(stored) = stored else {
        return (current.entries.keys().cloned().collect(), Vec::new());
    };
    let upserts = current
        .entries
        .iter()
        .filter(|(p, st)| stored.entries.get(*p) != Some(*st))
        .map(|(p, _)| p.clone())
        .collect();
    let deletes = stored
        .entries
        .keys()
        .filter(|p| !current.entries.contains_key(*p))
        .cloned()
        .collect();
    (upserts, deletes)
}

/// Empty globs ⇒ everything; else keep a path matching any minimal `*`-glob.
fn path_globs_ok(path: &Path, globs: &[String]) -> bool {
    if globs.is_empty() {
        return true;
    }
    let s = path.to_string_lossy();
    globs.iter().any(|g| glob_match(g, &s))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            Some(&c) => !t.is_empty() && t[0] == c && go(&p[1..], &t[1..]),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

fn load_vectors(path: &Path) -> Option<BTreeMap<PathBuf, Vec<f32>>> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_vectors(path: &Path, vectors: &BTreeMap<PathBuf, Vec<f32>>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec(vectors)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::{tempdir, FakeEmbedder};

    fn noop() -> impl Fn(agent_core::ReindexProgress) {
        |_| {}
    }
    fn write(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }
    // The index dir is kept OUTSIDE the scanned root so the walk doesn't index the
    // backend's own vectors.json/manifest (in production `.agent-seddon/` is
    // gitignored, so the gitignore-aware scan already excludes it).
    fn backend(dir: &Path, idx: &Path, embedder: Arc<dyn Embedder>) -> VectorBackend {
        VectorBackend::new(dir.to_path_buf(), idx.to_path_buf(), embedder)
    }
    fn sem(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            mode: SearchMode::Semantic,
            path_globs: vec![],
            lang: None,
            limit: 10,
            fuzzy_distance: None,
        }
    }
    async fn paths(b: &VectorBackend, q: &str) -> Vec<String> {
        b.query(&sem(q))
            .await
            .unwrap()
            .into_iter()
            .map(|h| h.path.to_string_lossy().into_owned())
            .collect()
    }

    // cosine ranks nearest-first; a doc sharing no query tokens still wins if its
    // vector is nearest (semantic finds lexically-disjoint code).
    #[tokio::test]
    async fn positive_cosine_ranks_nearest_first() {
        let dir = tempdir();
        let idx = tempdir();
        write(&dir, "a.rs", "sleep loop");
        write(&dir, "b.rs", "exponential delay");
        write(&dir, "c.rs", "print hello");
        let e = FakeEmbedder::new(
            3,
            vec![
                ("retry backoff", vec![0.0, 1.0, 0.0]),
                ("sleep loop", vec![0.7, 0.7, 0.0]),
                ("exponential delay", vec![0.0, 1.0, 0.0]),
                ("print hello", vec![1.0, 0.0, 0.0]),
            ],
        );
        let b = backend(&dir, &idx, Arc::new(e));
        b.reindex(&noop()).await.unwrap();
        let got = paths(&b, "retry backoff").await;
        assert_eq!(&got[..2], &["b.rs", "a.rs"], "got {got:?}");
    }

    #[tokio::test]
    async fn corner_oov_query_still_returns_nearest() {
        let dir = tempdir();
        let idx = tempdir();
        write(&dir, "a.rs", "alpha");
        write(&dir, "b.rs", "beta");
        // Unknown query → the default vector [1,0]; a.rs is nearer to it than b.rs.
        let e = FakeEmbedder::new(2, vec![("alpha", vec![1.0, 0.2]), ("beta", vec![0.2, 1.0])]);
        let b = backend(&dir, &idx, Arc::new(e));
        b.reindex(&noop()).await.unwrap();
        let got = paths(&b, "zzzz totally unknown").await;
        assert_eq!(&got[..2], &["a.rs", "b.rs"], "got {got:?}");
    }

    #[tokio::test]
    async fn boundary_empty_query_no_matches() {
        let dir = tempdir();
        let idx = tempdir();
        write(&dir, "a.rs", "alpha");
        let b = backend(&dir, &idx, Arc::new(FakeEmbedder::new(2, vec![])));
        b.reindex(&noop()).await.unwrap();
        assert!(b.query(&sem("")).await.unwrap().is_empty());
    }

    // Config-drift: index built at 2-d, query embedder swapped to 4-d ⇒ clear error.
    #[tokio::test]
    async fn negative_dims_mismatch_after_model_swap() {
        let dir = tempdir();
        let idx = tempdir();
        write(&dir, "a.rs", "alpha");
        // Build the index with a 2-d embedder.
        backend(&dir, &idx, Arc::new(FakeEmbedder::new(2, vec![])))
            .reindex(&noop())
            .await
            .unwrap();
        // Swap the model (4-d) but keep the stale 2-d index (loaded from disk).
        let swapped = backend(&dir, &idx, Arc::new(FakeEmbedder::new(4, vec![])));
        let err = swapped.query(&sem("x")).await.unwrap_err().to_string();
        assert!(err.contains("dimension"), "{err}");
    }

    // Incremental: editing one file re-embeds only that file, and the result updates.
    #[tokio::test]
    async fn positive_incremental_add_updates_results() {
        let dir = tempdir();
        let idx = tempdir();
        write(&dir, "a.rs", "sleepy");
        write(&dir, "b.rs", "winner");
        let e = Arc::new(FakeEmbedder::new(
            2,
            vec![
                ("q", vec![1.0, 0.0]),
                ("sleepy", vec![0.0, 1.0]), // far from q
                ("winner", vec![1.0, 0.0]), // near q
                ("now-winner", vec![1.0, 0.0]),
            ],
        ));
        let b = backend(&dir, &idx, e.clone());
        b.reindex(&noop()).await.unwrap();
        assert_eq!(paths(&b, "q").await[0], "b.rs");

        // Rewrite a.rs to the winning vector; reindex should re-embed ONLY a.rs.
        write(&dir, "a.rs", "now-winner");
        let before = e.embedded();
        b.reindex(&noop()).await.unwrap();
        assert_eq!(
            e.embedded() - before,
            1,
            "only the changed file re-embedded"
        );
        // a.rs now sorts first (tie on score → path order puts a before b).
        assert_eq!(paths(&b, "q").await[0], "a.rs");
    }
}
