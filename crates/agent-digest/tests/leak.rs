//! Heap leak + allocation-budget assertion for the digest ledger's hot cycle
//! under dhat: a distill put (replace on the `(session, seq, kind)` key — the
//! re-distillation path) followed by the compaction read must free everything it
//! allocates and stay under a per-cycle budget. Compiled only with
//! `--features dhat-heap,digest-sqlite`; `nix/checks/leak.nix` runs it.
#![cfg(all(feature = "dhat-heap", feature = "digest-sqlite"))]

use agent_core::{DigestKind, DigestQuery, DigestStore};
use agent_digest::{testdata, SqliteDigests};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[tokio::test]
async fn put_query_cycle_does_not_leak() {
    let _profiler = dhat::Profiler::builder().testing().build();
    let store = SqliteDigests::in_memory().expect("ledger");
    // A realistic resident ledger (12 exchanges) + one warm-up cycle so caches
    // (prepared statements, serde scratch) are counted as baseline, not growth.
    for row in testdata::session_rows("s0", 12) {
        store.put_sync(row).expect("seed");
    }
    let q = DigestQuery {
        session_id: "s0".into(),
        kind: Some(DigestKind::Summary),
        ..DigestQuery::default()
    };
    store
        .put_sync(testdata::digest("s0", 3, DigestKind::Summary))
        .unwrap();
    let _ = store.query(&q).await.unwrap();

    let base = dhat::HeapStats::get();
    const ITERS: u64 = 50;
    for i in 0..ITERS {
        // Replace the SAME key each cycle: the store's row count is constant, so
        // any live-block growth is a genuine leak, not data.
        let mut d = testdata::digest("s0", 3, DigestKind::Summary);
        d.text.push_str(&format!(" v{i}"));
        store.put_sync(d).expect("replace put");
        let rows = store.query(&q).await.expect("read");
        assert_eq!(rows.len(), 12, "constant ledger");
    }
    let after = dhat::HeapStats::get();

    dhat::assert!(
        after.curr_blocks <= base.curr_blocks + 16,
        "live blocks grew (leak?): {} -> {}",
        base.curr_blocks,
        after.curr_blocks
    );
    let per_iter = (after.total_blocks - base.total_blocks) / ITERS;
    // One replace + one 12-row decode per cycle; budget with headroom.
    dhat::assert!(per_iter < 1_200, "allocated {per_iter} blocks/cycle");
}
