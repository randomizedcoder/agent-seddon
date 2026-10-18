#![cfg(all(feature = "dhat-heap", feature = "extract-rust"))]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn positive_extraction_releases_allocations() {
    let _profiler = dhat::Profiler::builder().testing().build();
    // Initialize parser/TLS caches before measuring live blocks.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ws");
    drop(agent_repo_graph::extract::extract(&root, &Default::default()).unwrap());
    let before = dhat::HeapStats::get().curr_blocks;
    {
        let (graph, _) = agent_repo_graph::extract::extract(&root, &Default::default()).unwrap();
        assert!(!graph.nodes().is_empty());
    }
    let stats = dhat::HeapStats::get();
    dhat::assert_eq!(stats.curr_blocks, before);
    dhat::assert!(stats.total_blocks < 200_000);
}
