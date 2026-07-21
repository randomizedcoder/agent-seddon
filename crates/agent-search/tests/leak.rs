//! Heap leak + allocation-budget assertion for the semantic-search query path
//! (embed the query + brute-force cosine over the stored corpus), under dhat. Each
//! query allocates a query vector + a scored list; this pins that they free across
//! iterations. Uses the dependency-free `LocalEmbedder` (no model/network).
//! Compiled only with `--features dhat-heap,search-vector`; `nix/checks/leak.nix`
//! runs it.
#![cfg(all(feature = "dhat-heap", feature = "search-vector"))]

use std::sync::Arc;

use agent_core::{SearchBackend, SearchMode, SearchQuery};
use agent_embed::LocalEmbedder;
use agent_search::VectorBackend;
use agent_testkit::tempdir;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

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

#[tokio::test]
async fn vector_query_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let root = tempdir();
    let idx = tempdir();
    for i in 0..20 {
        std::fs::write(
            root.join(format!("f{i}.rs")),
            format!("fn item_{i}() {{ retry backoff exponential delay {i} }}"),
        )
        .unwrap();
    }
    let b = VectorBackend::new(
        root.to_path_buf(),
        idx.to_path_buf(),
        Arc::new(LocalEmbedder::new(128)),
    );
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
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    dhat::assert!(per_iter < 256, "allocated {per_iter} blocks/run");
}
