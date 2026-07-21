//! `agent-search` — the code-search seam's backends.
//!
//! Implements [`agent_core::SearchBackend`] over a full-text index so the agent
//! can find code fast. The default (and, for now, only) backend is
//! [`TantivyBackend`], a thread-safe on-disk inverted index; a DeepSearch backend
//! is reserved behind `search-deepsearch` for a follow-up.
//!
//! [`DispatchSearch`] composes one or more backends behind a single object so a
//! deployment can enable either or both and compare them head-to-head under the
//! same gRPC interface + metrics. Freshness (is the index up to date with the
//! working tree?) lives in [`manifest`]. See `docs/components/search.md`.

use agent_core::{
    Error, IndexStatus, ProgressFn, Result, SearchBackend, SearchCapabilities, SearchHit,
    SearchMode, SearchQuery,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub mod manifest;
pub use manifest::Manifest;

#[cfg(feature = "search-tantivy")]
mod tantivy;
#[cfg(feature = "search-tantivy")]
pub use tantivy::TantivyBackend;

#[cfg(feature = "search-vector")]
mod vector;
#[cfg(feature = "search-vector")]
pub use vector::VectorBackend;

/// The reciprocal-rank-fusion constant (`score = Σ 1/(k + rank)`). 60 is the
/// standard default; it needs no cross-backend score normalisation (BM25 and
/// cosine live on different scales), which is why RRF is the default fusion.
pub const RRF_K: usize = 60;

/// Fuse ranked hit lists with reciprocal-rank fusion, keyed by path. A doc that
/// ranks high in multiple lists rises above either single-list winner. Order is
/// deterministic (fused score desc, then path, then line); the fused score
/// replaces the per-backend score. Returns the top `limit`.
pub fn rrf_fuse(lists: &[Vec<SearchHit>], limit: usize) -> Vec<SearchHit> {
    let mut acc: HashMap<PathBuf, (f32, SearchHit)> = HashMap::new();
    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            let contribution = 1.0 / (RRF_K + rank + 1) as f32;
            acc.entry(hit.path.clone())
                .and_modify(|(s, _)| *s += contribution)
                .or_insert_with(|| (contribution, hit.clone()));
        }
    }
    let mut fused: Vec<(f32, SearchHit)> = acc.into_values().collect();
    fused.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.path.cmp(&b.1.path))
            .then_with(|| a.1.line.cmp(&b.1.line))
    });
    fused
        .into_iter()
        .take(limit)
        .map(|(score, mut hit)| {
            hit.score = score;
            hit
        })
        .collect()
}

/// Walk up from `start` looking for a `.git` directory; return that repo root, or
/// `start` itself if none is found. The index and its freshness manifest are
/// stored relative to this root so they are shared across sub-directory runs.
pub fn repo_root(start: &Path) -> PathBuf {
    let mut cur = start;
    loop {
        if cur.join(".git").exists() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return start.to_path_buf(),
        }
    }
}

/// The default on-disk index location for a backend: `<root>/.agent-seddon/index/<backend>`.
pub fn default_index_dir(root: &Path, backend: &str) -> PathBuf {
    root.join(".agent-seddon").join("index").join(backend)
}

/// Composes one or more named [`SearchBackend`]s behind a single object.
///
/// Through the [`SearchBackend`] trait it presents the **default** backend (the
/// first configured), so it drops in anywhere a single backend does — the loop's
/// tool, `= "grpc"` clients, `--serve-search`. Its inherent [`DispatchSearch::all`]
/// / [`DispatchSearch::backend`] expose every backend for callers that hold the
/// concrete type (the gRPC server's per-request `backend` selector, benchmarks).
pub struct DispatchSearch {
    backends: Vec<(String, Arc<dyn SearchBackend>)>,
}

impl DispatchSearch {
    /// Build from `(name, backend)` pairs; the first is the default. Errs if empty.
    pub fn new(backends: Vec<(String, Arc<dyn SearchBackend>)>) -> Result<Self> {
        if backends.is_empty() {
            return Err(Error::Search("no search backends configured".into()));
        }
        Ok(Self { backends })
    }

    /// All configured backends, in declaration order (first = default).
    pub fn all(&self) -> &[(String, Arc<dyn SearchBackend>)] {
        &self.backends
    }

    /// Look up a backend by name.
    pub fn backend(&self, name: &str) -> Option<&Arc<dyn SearchBackend>> {
        self.backends
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b)
    }

    fn default_backend(&self) -> &Arc<dyn SearchBackend> {
        &self.backends[0].1
    }

    /// Resolve a wire selector: `""` ⇒ default; a name ⇒ that backend; unknown ⇒ error.
    pub fn resolve(&self, selector: &str) -> Result<&Arc<dyn SearchBackend>> {
        if selector.is_empty() {
            return Ok(self.default_backend());
        }
        self.backend(selector).ok_or_else(|| {
            let known: Vec<&str> = self.backends.iter().map(|(n, _)| n.as_str()).collect();
            Error::Search(format!(
                "unknown search backend `{selector}` (known: {})",
                known.join(", ")
            ))
        })
    }

    /// Fan out to every backend (each with a mode it supports — semantic where
    /// available, else literal) and fuse the ranked lists with RRF. A backend that
    /// errors on its sub-query is skipped (best-effort). This is the hybrid
    /// lexical+semantic path (parity spec 15).
    pub async fn hybrid_query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        let mut lists: Vec<Vec<SearchHit>> = Vec::new();
        for (_, backend) in &self.backends {
            let caps = backend.capabilities();
            let mode = if caps.supports(SearchMode::Semantic) {
                SearchMode::Semantic
            } else if caps.supports(SearchMode::Literal) {
                SearchMode::Literal
            } else {
                continue;
            };
            let sub = SearchQuery { mode, ..q.clone() };
            if let Ok(hits) = backend.query(&sub).await {
                lists.push(hits);
            }
        }
        Ok(rrf_fuse(&lists, q.limit.max(1)))
    }
}

#[async_trait]
impl SearchBackend for DispatchSearch {
    fn capabilities(&self) -> SearchCapabilities {
        self.default_backend().capabilities()
    }
    async fn status(&self) -> Result<IndexStatus> {
        self.default_backend().status().await
    }
    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        self.default_backend().reindex(progress).await
    }
    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        if q.mode == SearchMode::Hybrid {
            return self.hybrid_query(q).await;
        }
        let backend = self.default_backend();
        reject_unsupported(&backend.capabilities(), q)?;
        backend.query(q).await
    }
    async fn list_files(&self, globs: &[String]) -> Result<Vec<std::path::PathBuf>> {
        self.default_backend().list_files(globs).await
    }
}

/// Guard: a backend must not silently degrade an unsupported [`agent_core::SearchMode`].
pub fn reject_unsupported(caps: &SearchCapabilities, q: &SearchQuery) -> Result<()> {
    if caps.supports(q.mode) {
        Ok(())
    } else {
        Err(Error::Search(format!(
            "backend `{}` does not support {} search",
            caps.backend,
            q.mode.as_str()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{IndexState, SearchMode};
    use agent_testkit::tempdir;

    // A backend that advertises only Literal and records nothing — for exercising
    // DispatchSearch routing + capability rejection without pulling in tantivy.
    struct LiteralOnly;
    #[async_trait]
    impl SearchBackend for LiteralOnly {
        fn capabilities(&self) -> SearchCapabilities {
            SearchCapabilities {
                backend: "literal-only".into(),
                modes: vec![SearchMode::Literal],
                content_search: true,
                scored: false,
                incremental: false,
                max_concurrent_queries: 0,
            }
        }
        async fn status(&self) -> Result<IndexStatus> {
            Ok(IndexStatus {
                state: IndexState::Fresh,
                indexed_files: 0,
                last_indexed_ms: 0,
                manifest_digest: String::new(),
            })
        }
        async fn reindex(&self, _p: ProgressFn<'_>) -> Result<IndexStatus> {
            self.status().await
        }
        async fn query(&self, _q: &SearchQuery) -> Result<Vec<SearchHit>> {
            Ok(vec![])
        }
    }

    fn q(mode: SearchMode) -> SearchQuery {
        SearchQuery {
            text: "x".into(),
            mode,
            path_globs: vec![],
            lang: None,
            limit: 10,
            fuzzy_distance: None,
        }
    }

    #[test]
    fn new_rejects_empty() {
        assert!(DispatchSearch::new(vec![]).is_err());
    }

    #[test]
    fn resolve_default_and_named_and_unknown() {
        let d = DispatchSearch::new(vec![("literal-only".into(), Arc::new(LiteralOnly))]).unwrap();
        assert!(d.resolve("").is_ok(), "empty selector ⇒ default");
        assert!(d.resolve("literal-only").is_ok());
        assert!(d.resolve("nope").is_err());
    }

    #[tokio::test]
    async fn dispatch_rejects_unsupported_mode() {
        let d = DispatchSearch::new(vec![("literal-only".into(), Arc::new(LiteralOnly))]).unwrap();
        assert!(d.query(&q(SearchMode::Literal)).await.is_ok());
        let err = d.query(&q(SearchMode::Regex)).await.unwrap_err();
        assert!(
            err.to_string().contains("does not support regex"),
            "got: {err}"
        );
    }

    // --- hybrid RRF fusion --------------------------------------------------
    fn hits(paths: &[&str]) -> Vec<SearchHit> {
        paths
            .iter()
            .map(|p| SearchHit {
                path: PathBuf::from(p),
                line: 0,
                col_start: 0,
                col_end: 0,
                score: 0.0,
                snippet: String::new(),
            })
            .collect()
    }

    #[rstest::rstest]
    // a doc high in BOTH lists rises above either single-list winner
    #[case::blends(vec!["a.rs", "b.rs", "c.rs"], vec!["b.rs", "c.rs", "a.rs"], vec!["b.rs", "a.rs", "c.rs"])]
    // a doc lexical never returns still appears (semantic contributed it)
    #[case::semantic_only_survives(vec!["a.rs"], vec!["z.rs", "a.rs"], vec!["a.rs", "z.rs"])]
    #[case::identical_preserves_order(vec!["a.rs", "b.rs"], vec!["a.rs", "b.rs"], vec!["a.rs", "b.rs"])]
    fn rrf_fuse_cases(
        #[case] lexical: Vec<&str>,
        #[case] semantic: Vec<&str>,
        #[case] fused: Vec<&str>,
    ) {
        let lists = vec![hits(&lexical), hits(&semantic)];
        let out: Vec<String> = rrf_fuse(&lists, 10)
            .into_iter()
            .map(|h| h.path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(out, fused);
    }

    // A backend that returns a fixed ordered result and advertises one mode.
    struct Ordered {
        name: &'static str,
        mode: SearchMode,
        paths: Vec<&'static str>,
    }
    #[async_trait]
    impl SearchBackend for Ordered {
        fn capabilities(&self) -> SearchCapabilities {
            SearchCapabilities {
                backend: self.name.into(),
                modes: vec![self.mode],
                content_search: true,
                scored: true,
                incremental: false,
                max_concurrent_queries: 0,
            }
        }
        async fn status(&self) -> Result<IndexStatus> {
            Ok(IndexStatus {
                state: IndexState::Fresh,
                indexed_files: 0,
                last_indexed_ms: 0,
                manifest_digest: String::new(),
            })
        }
        async fn reindex(&self, _p: ProgressFn<'_>) -> Result<IndexStatus> {
            self.status().await
        }
        async fn query(&self, _q: &SearchQuery) -> Result<Vec<SearchHit>> {
            Ok(hits(&self.paths))
        }
    }

    // Hybrid mode fans out to a lexical + a semantic backend and fuses the lists.
    #[tokio::test]
    async fn hybrid_fans_out_and_fuses() {
        let lex = Arc::new(Ordered {
            name: "lex",
            mode: SearchMode::Literal,
            paths: vec!["a.rs", "b.rs", "c.rs"],
        });
        let vec = Arc::new(Ordered {
            name: "vec",
            mode: SearchMode::Semantic,
            paths: vec!["b.rs", "c.rs", "a.rs"],
        });
        let d = DispatchSearch::new(vec![
            ("lex".into(), lex as Arc<dyn SearchBackend>),
            ("vec".into(), vec as Arc<dyn SearchBackend>),
        ])
        .unwrap();
        let out: Vec<String> = d
            .query(&q(SearchMode::Hybrid))
            .await
            .unwrap()
            .into_iter()
            .map(|h| h.path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(out, vec!["b.rs", "a.rs", "c.rs"]);
    }

    #[test]
    fn repo_root_finds_git_dir() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("src/inner")).unwrap();
        assert_eq!(repo_root(&dir.join("src/inner")), dir);
    }

    #[test]
    fn repo_root_falls_back_to_start() {
        let dir = tempdir();
        assert_eq!(repo_root(&dir), dir);
    }

    #[test]
    fn default_index_dir_layout() {
        let root = Path::new("/repo");
        assert_eq!(
            default_index_dir(root, "tantivy"),
            Path::new("/repo/.agent-seddon/index/tantivy")
        );
    }
}
