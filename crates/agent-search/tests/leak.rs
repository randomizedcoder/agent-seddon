//! Heap leak + allocation-budget assertions for the search backends' query paths,
//! under dhat. Compiled only with `--features dhat-heap`; `nix/checks/leak.nix`
//! runs it with both backend features enabled.
//!
//! dhat's profiler is process-global (only one may exist at a time), so a single
//! test brackets whichever backends are compiled in — the vector (semantic) path
//! and the tantivy `DocumentSource`-corpus path (the seam cross-session recall
//! reuses, parity spec 20).
//!
//! Flatness is asserted over **two consecutive windows** (the `agent-tools` leak
//! pattern), not as one absolute delta: `query` runs on `spawn_blocking`, so tokio
//! may add a blocking-pool thread (and its permanent buffers) on demand, and under
//! the gate's parallel build that first fire landed inside a single measured window
//! and read as a leak (`818 -> 846` against `+16`). A real leak grows *every*
//! window; one-time init only shows in the first, so window 1 is absorbed and window
//! 2 must be flat.
#![cfg(feature = "dhat-heap")]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Run `body` over two consecutive windows of `ITERS` runs each (after a warm-up):
/// window 1 (`base -> mid`) absorbs one-time lazy init, window 2 (`mid -> after`)
/// must stay flat within `window_slack` live blocks, and the cumulative allocation
/// rate over both windows must stay under `max_blocks_per_run`.
async fn assert_flat<F, Fut>(label: &str, window_slack: usize, max_blocks_per_run: u64, mut body: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    const ITERS: u64 = 50;

    body().await; // warm up
    let base = dhat::HeapStats::get();
    for _ in 0..ITERS {
        body().await;
    }
    let mid = dhat::HeapStats::get();
    for _ in 0..ITERS {
        body().await;
    }
    let after = dhat::HeapStats::get();

    let window2_growth = after.curr_blocks.saturating_sub(mid.curr_blocks);
    dhat::assert!(
        window2_growth <= window_slack,
        "{label}: live blocks still growing in window 2 (leak?): base {} -> mid {} -> after {}",
        base.curr_blocks,
        mid.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / (2 * ITERS);
    dhat::assert!(
        per_iter < max_blocks_per_run,
        "{label}: allocated {per_iter} blocks/run (> {max_blocks_per_run})"
    );
}

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

    assert_flat("vector", 8, 256, || async {
        let hits = b.query(&sem("retry backoff")).await.unwrap();
        assert!(!hits.is_empty());
    })
    .await;
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

    // `16`, not `8`: tokio's blocking pool may still add one thread inside window 2
    // (each `query` is a `spawn_blocking`); a real leak grows every window regardless.
    assert_flat("tantivy corpus", 16, 2048, || async {
        let hits = backend.query(&lit("retry backoff")).await.unwrap();
        assert!(!hits.is_empty());
    })
    .await;
}
