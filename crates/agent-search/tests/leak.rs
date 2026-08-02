//! Heap leak + allocation-budget assertions for the search backends' query paths,
//! under dhat. Compiled only with `--features dhat-heap`; `nix/checks/leak.nix`
//! runs it with both backend features enabled.
//!
//! dhat's profiler is process-global (only one may exist at a time), so a single
//! test brackets whichever backends are compiled in — the vector (semantic) path
//! and the tantivy `DocumentSource`-corpus path (the seam cross-session recall
//! reuses, parity spec 20).
#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::test]
async fn search_query_paths_do_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();

    #[cfg(feature = "search-vector")]
    vector_path().await;

    #[cfg(feature = "search-tantivy")]
    tantivy_corpus_path().await;
}

/// Semantic query path: embed the query + brute-force cosine over the stored
/// corpus. Each query allocates a query vector + a scored list; pin that they free
/// across iterations. Dependency-free `LocalEmbedder` (no model/network).
#[cfg(feature = "search-vector")]
async fn vector_path() {
    use agent_core::{SearchBackend, SearchMode, SearchQuery};
    use agent_embed::LocalEmbedder;
    use agent_search::VectorBackend;
    use agent_testkit::tempdir;
    use std::sync::Arc;

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

    let root = tempdir();
    let idx = tempdir();
    for i in 0..20 {
        std::fs::write(
            root.join(format!("f{i}.rs")),
            format!("fn item_{i}() {{ retry backoff exponential delay {i} }}"),
        )
        .unwrap();
    }
    let b = VectorBackend::new(root.clone(), idx.clone(), Arc::new(LocalEmbedder::new(128)));
    b.reindex(&|_| {}).await.unwrap();

    let _ = b.query(&sem("retry backoff")).await.unwrap(); // warm up
    let base = dhat::HeapStats::get();

    const ITERS: u64 = 100;
    for _ in 0..ITERS {
        let hits = b.query(&sem("retry backoff")).await.unwrap();
        assert!(!hits.is_empty());
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 8,
        "vector: live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 256, "vector: allocated {per_iter} blocks/run");
}

/// Tantivy query path over a **`DocumentSource` corpus** — the seam cross-session
/// recall uses (a non-filesystem source fed to `open_with_source`). Pin that
/// repeated queries free everything across iterations.
#[cfg(feature = "search-tantivy")]
async fn tantivy_corpus_path() {
    use agent_core::{IndexState, SearchBackend, SearchMode, SearchQuery};
    use agent_search::manifest::FileStamp;
    use agent_search::{DocumentSource, Manifest, SourceDoc, TantivyBackend};
    use agent_testkit::tempdir;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    // An in-memory corpus keyed by opaque ids — models the session corpus without
    // touching the filesystem.
    struct MemCorpus {
        docs: Vec<(String, String)>,
    }
    impl DocumentSource for MemCorpus {
        fn scan(&self) -> Manifest {
            let entries: BTreeMap<PathBuf, FileStamp> = self
                .docs
                .iter()
                .map(|(id, text)| {
                    (
                        PathBuf::from(id),
                        FileStamp {
                            mtime_ms: 1,
                            size: text.len() as u64,
                        },
                    )
                })
                .collect();
            Manifest {
                entries,
                git_head: None,
                built_ms: 1,
            }
        }
        fn compare(&self, stored: Option<&Manifest>) -> IndexState {
            if stored.is_some() {
                IndexState::Fresh
            } else {
                IndexState::Missing
            }
        }
        fn load(&self, id: &Path) -> Option<SourceDoc> {
            let key = id.to_string_lossy();
            self.docs
                .iter()
                .find(|(i, _)| i.as_str() == key)
                .map(|(_, text)| SourceDoc {
                    text: text.clone(),
                    lang: "interactive".into(),
                })
        }
    }

    fn lit(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            mode: SearchMode::Literal,
            path_globs: vec![],
            lang: None,
            limit: 10,
            fuzzy_distance: None,
        }
    }

    let idx = tempdir();
    let docs = (0..20)
        .map(|i| {
            (
                format!("s{i}"),
                format!("session {i} about retry backoff exponential delay"),
            )
        })
        .collect();
    let backend =
        TantivyBackend::open_with_source(Arc::new(MemCorpus { docs }), idx.join("idx")).unwrap();
    backend.reindex(&|_| {}).await.unwrap();

    let _ = backend.query(&lit("retry backoff")).await.unwrap(); // warm up
    let base = dhat::HeapStats::get();

    const ITERS: u64 = 100;
    for _ in 0..ITERS {
        let hits = backend.query(&lit("retry backoff")).await.unwrap();
        assert!(!hits.is_empty());
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 16,
        "tantivy corpus: live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(
        per_iter < 2048,
        "tantivy corpus: allocated {per_iter} blocks/run"
    );
}
