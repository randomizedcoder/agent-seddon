//! The `PgRepoGraph` test suite, in three tiers (`docs/design/repo-knowledge/08-test-matrix.md`):
//!
//! * **P1 — in-gate units** (no database): the `with_tenant` refusals (provably synchronous on a
//!   lazily-connecting pool) and the `map_db` mapping. These are **not** `#[ignore]`; they run in
//!   `nix flake check` via `nix/checks/repo-graph.nix` (`cargo test --features repo-graph-postgres`,
//!   no `--ignored`). The pure array/clamp/chunk units live next to the code they cover, in
//!   [`super::sql`].
//! * **R3-pg — conformance reuse** (live; `#[ignore]`): the shared rows of
//!   [`repo_graph_conformance_suite!`](agent_testkit::repo_graph_conformance_suite), stamped
//!   `pg::r3::<row>`, run **unchanged** against `PgRepoGraph` with [`assert_invariants`] after each —
//!   identical outcomes to the `mem` tier are the acceptance bar.
//! * **R4 — pg-only** (live; `#[ignore]`): behaviour not expressible against `MemRepoGraph`
//!   (migration idempotence, shared-body row counts, the collision left-untouched guarantee,
//!   concurrent duplicate-identity, cross-tenant isolation at the SQL level, orphan-body sweeping, a
//!   corrupt stored row decoding to `Backend`, a real CHECK → `Invalid`, the diff cap).
//!
//! The live tiers need a server, so each is `#[ignore]`-gated on `AGENT_REPO_GRAPH_TEST_DSN` (set by
//! `nix run .#pg-integration`, which runs the suite single-threaded and resets the DB per test). The
//! module still compiles in-gate under `clippy --all-features` with no database.

use super::*;
use agent_core::repo_graph::{
    node_id_for, GraphBuilder, NodeOutcome, NodeSpec, MAX_DIFF as CAP_DIFF,
};
use agent_testkit::repo_graph::conformance::{
    fixture_v1, fixture_v2, key_alpha, key_file_m, Harness,
};
use rstest::rstest;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

// ===========================================================================
// P1 — in-gate units (no database)
// ===========================================================================

/// A handle over a lazily-connecting pool: the DSN parses but no connection is ever opened, so the
/// `with_tenant` refusals are provably synchronous (no database needed). `connect_lazy` itself needs
/// a Tokio context (the pool spawns a reaper), so the callers are `#[tokio::test]` — but no statement
/// is ever issued, so no server is required.
fn lazy() -> PgRepoGraph {
    PgRepoGraph::connect_lazy("postgres://u:p@localhost:5432/db", 1).expect("valid DSN parses")
}

#[tokio::test]
async fn positive_with_tenant_valid() {
    let s = lazy().with_tenant("ta").expect("safe tenant");
    assert_eq!(s.tenant(), "ta");
}

#[tokio::test]
async fn positive_with_tenant_clone_shares_pool() {
    let s = lazy().with_tenant("ta").unwrap();
    let c = s.clone();
    assert_eq!(c.tenant(), "ta");
}

#[rstest]
#[case::traversal("../x")]
#[case::space("a b")]
#[case::slash("a/b")]
#[case::dash("-x")]
#[case::dotdot("..")]
#[case::empty("")]
#[tokio::test]
async fn adversarial_tenant_refused_before_any_statement(#[case] tenant: &str) {
    // The lazy pool never connected; a refusal here is therefore synchronous, before SQL.
    let err = lazy()
        .with_tenant(tenant)
        .expect_err("unsafe tenant refused");
    assert!(matches!(err, RepoGraphError::Invalid(_)));
}

#[tokio::test]
async fn boundary_tenant_128_ok_129_refused() {
    assert!(lazy().with_tenant(&"a".repeat(128)).is_ok());
    assert!(lazy().with_tenant(&"a".repeat(129)).is_err());
}

#[test]
fn positive_map_db_row_not_found() {
    assert_eq!(map_db(sqlx::Error::RowNotFound), RepoGraphError::NotFound);
}

#[test]
fn negative_map_db_other_is_backend() {
    // A non-`Database` error maps to `Backend` and never carries a DSN/password.
    let err = map_db(sqlx::Error::PoolClosed);
    match err {
        RepoGraphError::Backend(msg) => {
            assert!(msg.starts_with("repo-graph postgres:"));
            assert!(!msg.contains("localhost"));
            assert!(!msg.contains("password"));
        }
        other => panic!("expected Backend, got {other:?}"),
    }
}

// ===========================================================================
// Live-suite harness (R3-pg + R4)
// ===========================================================================

const REQUIRES_PG: &str =
    "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration";

fn dsn() -> String {
    std::env::var("AGENT_REPO_GRAPH_TEST_DSN").expect(REQUIRES_PG)
}

/// A migrated pool on the test DSN (no reset).
async fn test_pool() -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&dsn())
        .await
        .expect("connect postgres");
    PgRepoGraph::run_migrations(&pool).await.expect("migrate");
    pool
}

/// Reset the graph tables to a clean slate (newest → oldest by FK). `tenants` is shared with the
/// other Pg tiers and left alone (a stale tenant row is inert). `RESTART IDENTITY` keeps ids small
/// and `CASCADE` clears any dependents defensively.
async fn reset(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE graph_edges, graph_node_versions, graph_nodes, graph_snapshots, repos \
         RESTART IDENTITY CASCADE",
    )
    .execute(pool)
    .await
    .expect("reset repo-graph tables");
}

/// A store on `pool` bound to `tenant`, reading `clock` for its time.
fn tenant_store(pool: &PgPool, clock: &Arc<AtomicU64>, tenant: &str) -> PgRepoGraph {
    let c = Arc::clone(clock);
    PgRepoGraph::from_pool(pool.clone())
        .with_clock(Arc::new(move || c.load(Ordering::SeqCst)))
        .with_tenant(tenant)
        .expect("safe tenant")
}

/// A fresh clock (the conformance epoch) and a reset pool on it.
async fn fresh() -> (PgPool, Arc<AtomicU64>) {
    let pool = test_pool().await;
    reset(&pool).await;
    (pool, Arc::new(AtomicU64::new(1_700_000_000_000)))
}

/// The harness the conformance rows run against: `open(tenant)` = `with_tenant` on one shared pool,
/// reading the harness clock.
async fn pg_harness() -> Harness {
    let (pool, clock) = fresh().await;
    let base_clock = Arc::clone(&clock);
    let base = PgRepoGraph::from_pool(pool)
        .with_clock(Arc::new(move || base_clock.load(Ordering::SeqCst)));
    Harness::from_factory(
        clock,
        Arc::new(move |tenant| {
            base.with_tenant(tenant)
                .map(|s| Arc::new(s) as Arc<dyn RepoGraphStore>)
        }),
    )
}

// ---------------------------------------------------------------------------
// assert_invariants — the app-side invariants the schema CHECKs cannot express
// ---------------------------------------------------------------------------

/// `(snapshot_id, reason)` for every row that breaks an invariant, over the whole (reset-per-test)
/// DB: an edge endpoint absent from `graph_nodes` in-scope, or a `ready` snapshot whose recorded
/// `node_count` / `edge_count` disagrees with its stored rows. (Version / edge rows referencing a
/// missing snapshot are impossible — the FKs enforce that — so they are not re-checked here.)
const INVARIANTS: &str = "
    SELECT e.snapshot_id, 'edge_src_missing'
      FROM graph_edges e
     WHERE NOT EXISTS (SELECT 1 FROM graph_nodes n
                        WHERE n.tenant = e.tenant AND n.repo_id = e.repo_id AND n.node_id = e.src_id)
    UNION ALL
    SELECT e.snapshot_id, 'edge_dst_missing'
      FROM graph_edges e
     WHERE NOT EXISTS (SELECT 1 FROM graph_nodes n
                        WHERE n.tenant = e.tenant AND n.repo_id = e.repo_id AND n.node_id = e.dst_id)
    UNION ALL
    SELECT s.snapshot_id, 'node_count'
      FROM graph_snapshots s
     WHERE s.status = 'ready'
       AND s.node_count <> (SELECT count(*) FROM graph_node_versions v
                             WHERE v.tenant = s.tenant AND v.repo_id = s.repo_id
                               AND v.snapshot_id = s.snapshot_id)
    UNION ALL
    SELECT s.snapshot_id, 'edge_count'
      FROM graph_snapshots s
     WHERE s.status = 'ready'
       AND s.edge_count <> (SELECT count(*) FROM graph_edges e
                             WHERE e.tenant = s.tenant AND e.repo_id = s.repo_id
                               AND e.snapshot_id = s.snapshot_id)
";

async fn invariant_violations(pool: &PgPool) -> Vec<(i64, String)> {
    sqlx::query(INVARIANTS)
        .fetch_all(pool)
        .await
        .expect("invariants query")
        .iter()
        .map(|r| (r.get::<i64, _>(0), r.get::<String, _>(1)))
        .collect()
}

/// The `after` hook of every conformance row: the invariants hold over the whole DB.
async fn assert_invariants(_h: &Harness) {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn())
        .await
        .expect("connect postgres");
    let bad = invariant_violations(&pool).await;
    assert!(bad.is_empty(), "invariants violated: {bad:?}");
}

// ===========================================================================
// R3-pg — the shared conformance rows, unchanged, on Postgres
// ===========================================================================

agent_testkit::repo_graph_conformance_suite!(
    pg,
    pg_harness().await,
    after = assert_invariants,
    ignore =
        "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"
);

// ===========================================================================
// R4 — pg-only rows
// ===========================================================================

/// A distinct, valid 40-char lowercase-hex commit sha per `n`.
fn sha(n: u32) -> String {
    format!("{n:040x}")
}

fn repo_spec(slug: &str) -> RepoSpec {
    RepoSpec {
        slug: slug.into(),
        forge: "github".into(),
        remote_url: "https://example.com/o/r.git".into(),
        default_branch: "main".into(),
        profile: json!({}),
    }
}

fn begin_spec(repo: RepoId, commit_sha: &str) -> SnapshotBegin {
    SnapshotBegin {
        repo,
        commit_sha: commit_sha.into(),
        extractors: vec!["rust-syn".into()],
        extractor_version: "rust-syn@1".into(),
    }
}

/// Put a repo, write `g` as a ready snapshot at `sha(n)`, return its read [`Scope`].
async fn seed(s: &PgRepoGraph, slug: &str, n: u32, g: &RepoGraph) -> Scope {
    let repo = s.repo_put(&repo_spec(slug)).await.expect("repo_put");
    let id = s
        .snapshot_begin(&begin_spec(repo, &sha(n)))
        .await
        .expect("snapshot_begin");
    s.snapshot_write(id, g).await.expect("snapshot_write");
    s.snapshot_finish(id, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .expect("snapshot_finish");
    Scope::new(repo, id)
}

/// A graph of `n` distinct `rust:fn:ws_a::gen::f<i>` nodes, no edges (for the diff-cap row).
fn gen_graph(n: usize) -> RepoGraph {
    let mut b = GraphBuilder::default();
    for i in 0..n {
        let key =
            NodeKey::rust_item(NodeKind::Fn, "ws_a", "gen", &format!("f{i}")).expect("gen key");
        let out = b.node(
            NodeSpec::new(key, format!("f{i}"))
                .with_lines(1, 2)
                .with_file("src/gen.rs")
                .with_sig("s")
                .with_body("b"),
        );
        assert!(
            matches!(out, NodeOutcome::Added(_)),
            "gen node {i}: {out:?}"
        );
    }
    b.finish().expect("gen graph builds").graph
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_migration_idempotent() {
    let pool = test_pool().await; // one migrate
    PgRepoGraph::run_migrations(&pool)
        .await
        .expect("re-migrate"); // second run no-ops
    let versions: Vec<i64> =
        sqlx::query("SELECT version FROM _repo_graph_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .expect("ledger")
            .iter()
            .map(|r| r.get::<i64, _>(0))
            .collect();
    assert_eq!(versions, vec![1], "version 1 recorded exactly once");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn positive_shared_body_row_count() {
    let (pool, clock) = fresh().await;
    let s = tenant_store(&pool, &clock, "ta");
    let g = fixture_v1();
    let repo = s.repo_put(&repo_spec("owner__repo")).await.unwrap();
    // Two snapshots of the identical graph: bodies are shared, versions are per-snapshot.
    for n in 1..=2u32 {
        let id = s.snapshot_begin(&begin_spec(repo, &sha(n))).await.unwrap();
        s.snapshot_write(id, &g).await.unwrap();
        s.snapshot_finish(id, SnapshotStatus::Ready, "", &ExtractReport::default())
            .await
            .unwrap();
    }
    let nodes: i64 = sqlx::query("SELECT count(*) FROM graph_nodes WHERE tenant='ta'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    let versions: i64 = sqlx::query("SELECT count(*) FROM graph_node_versions WHERE tenant='ta'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    let body_count = g.nodes().len() as i64;
    assert_eq!(nodes, body_count, "bodies shared: one row per key");
    assert_eq!(versions, body_count * 2, "versions duplicated per snapshot");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn negative_write_id_collision_leaves_building() {
    let (pool, clock) = fresh().await;
    let s = tenant_store(&pool, &clock, "ta");
    let repo = s.repo_put(&repo_spec("owner__repo")).await.unwrap();
    // Snapshot 1: store the v1 bodies.
    let id1 = s.snapshot_begin(&begin_spec(repo, &sha(1))).await.unwrap();
    s.snapshot_write(id1, &fixture_v1()).await.unwrap();
    s.snapshot_finish(id1, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap();

    // Snapshot 2: forge a graph whose single node maps a *new* key onto alpha's stored node_id, via
    // the doc-hidden id override the collision tests use (applied on the builder, before `finish`).
    let alpha_id = node_id_for(&key_alpha());
    let collide = NodeKey::rust_item(NodeKind::Fn, "ws_a", "gen", "collide").expect("key");
    let mut b = GraphBuilder::default().with_id_fn(move |_key| alpha_id);
    let out = b.node(
        NodeSpec::new(collide, "collide")
            .with_lines(1, 2)
            .with_file("src/gen.rs")
            .with_sig("s")
            .with_body("b"),
    );
    assert!(matches!(out, NodeOutcome::Added(_)));
    let g2 = b.finish().expect("builds").graph;
    let id2 = s.snapshot_begin(&begin_spec(repo, &sha(2))).await.unwrap();

    let err = s.snapshot_write(id2, &g2).await.expect_err("collision");
    assert!(matches!(err, RepoGraphError::Conflict(_)), "got {err:?}");

    // The tx rolled back: snapshot 2 is still `building`, with zero versions / edges.
    let status: String =
        sqlx::query("SELECT status FROM graph_snapshots WHERE tenant='ta' AND snapshot_id=$1")
            .bind(id2.0)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
    assert_eq!(status, "building");
    let vcount: i64 = sqlx::query(
        "SELECT count(*) FROM graph_node_versions WHERE tenant='ta' AND snapshot_id=$1",
    )
    .bind(id2.0)
    .fetch_one(&pool)
    .await
    .unwrap()
    .get(0);
    assert_eq!(vcount, 0, "nothing written for the failed snapshot");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_concurrent_begin_duplicate_identity() {
    let (pool, clock) = fresh().await;
    let s = tenant_store(&pool, &clock, "ta");
    let repo = s.repo_put(&repo_spec("owner__repo")).await.unwrap();

    let (a, b) = (s.clone(), s.clone());
    let spec_a = begin_spec(repo, &sha(7));
    let spec_b = begin_spec(repo, &sha(7));
    let ta = tokio::spawn(async move { a.snapshot_begin(&spec_a).await });
    let tb = tokio::spawn(async move { b.snapshot_begin(&spec_b).await });
    let (ra, rb) = (ta.await.unwrap(), tb.await.unwrap());

    let oks = [&ra, &rb].iter().filter(|r| r.is_ok()).count();
    let conflicts = [&ra, &rb]
        .iter()
        .filter(|r| matches!(r, Err(RepoGraphError::Conflict(_))))
        .count();
    assert_eq!(oks, 1, "exactly one begin wins: {ra:?} / {rb:?}");
    assert_eq!(conflicts, 1, "the other is a Conflict: {ra:?} / {rb:?}");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_cross_tenant_reads_return_nothing() {
    let (pool, clock) = fresh().await;
    let a = tenant_store(&pool, &clock, "ta");
    let b = tenant_store(&pool, &clock, "tb");
    // Write a full graph under `ta`.
    let scope = seed(&a, "owner__repo", 1, &fixture_v1()).await;

    // `tb` shares neither the repo nor the snapshot: `repos()` is empty, `repo_get` is None.
    assert!(b.repos().await.unwrap().is_empty(), "tb sees no repos");
    assert!(
        b.repo_get("owner__repo").await.unwrap().is_none(),
        "tb sees no repo by slug"
    );

    // Every read verb, run under `tb` with `ta`'s scope, sees `NotFound` (scope preflight) — never a
    // foreign row.
    let seed_id = node_id_for(&key_alpha());
    macro_rules! assert_not_found {
        ($e:expr) => {
            assert!(
                matches!($e.await, Err(RepoGraphError::NotFound)),
                "cross-tenant read leaked"
            );
        };
    }
    assert_not_found!(b.nodes_by_key(scope, &[key_alpha()]));
    assert_not_found!(b.nodes_by_file(scope, &["src/m.rs".into()]));
    assert_not_found!(b.nodes_by_name(scope, "alpha", None, 10));
    assert_not_found!(b.neighbors(scope, &[seed_id], EdgeKind::Calls, Direction::In, 2, 10));
    assert_not_found!(b.blast_radius(scope, &["src/m.rs".into()], 2, 10));
    assert_not_found!(b.tests_covering(scope, &[seed_id], 2, 10));
    assert_not_found!(b.path_between(scope, seed_id, node_id_for(&key_file_m()), 4, 4));
    assert_not_found!(b.shape(scope));
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn corner_retention_sweeps_orphan_bodies() {
    let (pool, clock) = fresh().await;
    let s = tenant_store(&pool, &clock, "ta");
    let repo = s.repo_put(&repo_spec("owner__repo")).await.unwrap();

    // Snapshot 1 carries `struct S` (v1 only); snapshot 2 (v2) drops it. Keeping only the newest
    // sweeps S's now-orphaned body row.
    let id1 = s.snapshot_begin(&begin_spec(repo, &sha(1))).await.unwrap();
    s.snapshot_write(id1, &fixture_v1()).await.unwrap();
    s.snapshot_finish(id1, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap();
    clock.fetch_add(1000, Ordering::SeqCst);
    let id2 = s.snapshot_begin(&begin_spec(repo, &sha(2))).await.unwrap();
    s.snapshot_write(id2, &fixture_v2()).await.unwrap();
    s.snapshot_finish(id2, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap();

    let s_key = agent_testkit::repo_graph::conformance::key_struct_s();
    let s_id = node_id_for(&s_key);
    let before: i64 =
        sqlx::query("SELECT count(*) FROM graph_nodes WHERE tenant='ta' AND node_id=$1")
            .bind(s_id.0)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
    assert_eq!(before, 1, "S's body present before retention");

    let deleted = s.snapshot_delete_older_than(repo, 1).await.unwrap();
    assert_eq!(deleted, 1, "snapshot 1 deleted");

    let after: i64 =
        sqlx::query("SELECT count(*) FROM graph_nodes WHERE tenant='ta' AND node_id=$1")
            .bind(s_id.0)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
    assert_eq!(after, 0, "S's orphaned body swept");
    // A body still referenced by the survivor (alpha) is kept.
    let alpha_kept: i64 =
        sqlx::query("SELECT count(*) FROM graph_nodes WHERE tenant='ta' AND node_id=$1")
            .bind(node_id_for(&key_alpha()).0)
            .fetch_one(&pool)
            .await
            .unwrap()
            .get(0);
    assert_eq!(alpha_kept, 1, "still-referenced body kept");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn adversarial_connect_bad_dsn_does_not_echo() {
    // A DSN carrying a password, pointed at a port nothing listens on: connect fails, and the error
    // string must contain neither the password nor the DSN.
    let bad = "postgres://user:hunter2@127.0.0.1:1/nope";
    let err = PgRepoGraph::connect(bad, 1, false)
        .await
        .expect_err("connect fails");
    let msg = format!("{err:?}");
    assert!(!msg.contains("hunter2"), "password leaked: {msg}");
    assert!(!msg.contains("nope"), "DSN db name leaked: {msg}");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn corner_decode_corrupt_row_is_backend() {
    let (pool, clock) = fresh().await;
    let s = tenant_store(&pool, &clock, "ta");
    let scope = seed(&s, "owner__repo", 1, &fixture_v1()).await;

    // Corrupt a stored `lang` to a value the decoder does not know (the column has no CHECK, unlike
    // `kind`); reading the row back must be a `Backend` fault, never a panic.
    sqlx::query("UPDATE graph_nodes SET lang='klingon' WHERE tenant='ta' AND node_id=$1")
        .bind(node_id_for(&key_alpha()).0)
        .execute(&pool)
        .await
        .unwrap();
    let err = s
        .nodes_by_key(scope, &[key_alpha()])
        .await
        .expect_err("corrupt row rejected");
    assert!(matches!(err, RepoGraphError::Backend(_)), "got {err:?}");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn negative_map_db_check_is_invalid() {
    let (pool, clock) = fresh().await;
    let s = tenant_store(&pool, &clock, "ta");
    let repo = s.repo_put(&repo_spec("owner__repo")).await.unwrap();
    // A commit_sha that fails the `^[0-9a-f]{40}$` CHECK reaches the DB and maps to `Invalid`.
    let err = s
        .snapshot_begin(&begin_spec(repo, "not-a-valid-sha"))
        .await
        .expect_err("check violation");
    assert!(matches!(err, RepoGraphError::Invalid(_)), "got {err:?}");
}

#[tokio::test]
#[ignore = "requires a live Postgres (AGENT_REPO_GRAPH_TEST_DSN); run via nix run .#pg-integration"]
async fn boundary_diff_capped() {
    let (pool, clock) = fresh().await;
    let s = tenant_store(&pool, &clock, "ta");
    let repo = s.repo_put(&repo_spec("owner__repo")).await.unwrap();

    // Snapshot 1: empty. Snapshot 2: MAX_DIFF + 1 added nodes ⇒ the added list is capped.
    let id1 = s.snapshot_begin(&begin_spec(repo, &sha(1))).await.unwrap();
    s.snapshot_write(id1, &gen_graph(0)).await.unwrap();
    s.snapshot_finish(id1, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap();
    let big = gen_graph(CAP_DIFF + 1);
    let id2 = s.snapshot_begin(&begin_spec(repo, &sha(2))).await.unwrap();
    s.snapshot_write(id2, &big).await.unwrap();
    s.snapshot_finish(id2, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap();

    let diff = s.snapshot_diff(id1, id2).await.unwrap();
    assert!(diff.truncated, "diff over MAX_DIFF is truncated");
    assert!(
        diff.added.len() <= CAP_DIFF,
        "added list capped to MAX_DIFF"
    );
}
