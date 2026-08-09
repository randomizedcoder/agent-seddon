//! Deterministic instruction-count bench for the digest ledger's **read** path —
//! the instant-compaction hot path (increment 03 assembles from exactly this
//! query). Corpus is the standard `testdata` session mix (4 sessions × 40
//! exchanges ≈ 368 realistic rows); sqlite in-memory is the harness — created in
//! setup, dropped after, nothing to clean up. The measured call is one
//! kind-filtered ordered read plus one keyword-prefiltered read (fetch → decode →
//! filter → cap), driven by a current-thread runtime (the store is synchronous
//! under the hood, so the async shim is constant overhead).
//! Ceiling is an absolute `Ir` `hard_limits`; a regression fails `cargo bench`.

use std::hint::black_box;

use agent_core::{DigestKind, DigestQuery, DigestStore};
use agent_digest::{testdata, SqliteDigests};
use iai_callgrind::{
    library_benchmark, library_benchmark_group, main, Callgrind, EventKind, LibraryBenchmarkConfig,
};

fn populated() -> SqliteDigests {
    testdata::populated_sqlite(4, 40)
}

// Observed 2,064,651 → 2,053,229 Ir after the optimization pass (2026-08-09:
// `prepare_cached` — constant SQL, skip re-parsing on repeated compaction reads;
// remaining cost is rusqlite row stepping + the per-row keyword JSON decode,
// both load-bearing). Ceiling ~2.5×. The read runs at compaction time, never on
// the turn hot path.
#[library_benchmark(config = LibraryBenchmarkConfig::default()
    .tool(Callgrind::default().hard_limits([(EventKind::Ir, 5_200_000u64)])))]
#[bench::session_reads(setup = populated)]
fn query_summaries_and_keywords(store: SqliteDigests) -> usize {
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("rt");
    let summaries = rt
        .block_on(store.query(&DigestQuery {
            session_id: "s2".into(),
            kind: Some(DigestKind::Summary),
            ..DigestQuery::default()
        }))
        .expect("summaries");
    let by_keyword = rt
        .block_on(store.query(&DigestQuery {
            session_id: "s2".into(),
            keywords_any: vec!["sqlite".into(), "debugging".into()],
            ..DigestQuery::default()
        }))
        .expect("keyword read");
    black_box(summaries.len() + by_keyword.len())
}

library_benchmark_group!(name = digest_query; benchmarks = query_summaries_and_keywords);
main!(library_benchmark_groups = digest_query);
