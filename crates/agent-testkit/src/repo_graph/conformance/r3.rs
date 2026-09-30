//! R3 — store conformance (`docs/design/repo-knowledge/08-test-matrix.md`). One
//! `pub async fn <row>(h: &Harness)` per shared row, written against the [`RepoGraphStore`] trait
//! so every tier ([`MemRepoGraph`] now, `PgRepoGraph` in RK-02) runs the identical assertions
//! through [`repo_graph_conformance_suite!`](crate::repo_graph_conformance_suite).

use super::*;
use agent_core::repo_graph::{
    node_id_for, Direction, ExtractReport, RepoGraphError, RepoId, RepoSpec, Scope, SnapshotBegin,
    SnapshotId, SnapshotStatus, MAX_PATHS, MAX_RESULT,
};
use serde_json::json;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

async fn put_repo(s: &dyn RepoGraphStore, slug: &str) -> RepoId {
    s.repo_put(&repo_spec(slug)).await.expect("repo_put")
}

async fn begin(s: &dyn RepoGraphStore, repo: RepoId, commit_sha: &str) -> SnapshotId {
    s.snapshot_begin(&begin_spec(repo, commit_sha))
        .await
        .expect("snapshot_begin")
}

async fn write_ready(s: &dyn RepoGraphStore, id: SnapshotId, g: &RepoGraph) {
    s.snapshot_write(id, g).await.expect("snapshot_write");
    s.snapshot_finish(id, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .expect("snapshot_finish");
}

/// Put a repo, write `g` as a ready snapshot at `sha(n)`, and return its read [`Scope`].
async fn seed(s: &dyn RepoGraphStore, slug: &str, n: u32, g: &RepoGraph) -> Scope {
    let repo = put_repo(s, slug).await;
    let id = begin(s, repo, &sha(n)).await;
    write_ready(s, id, g).await;
    Scope::new(repo, id)
}

/// A generated `rust:fn:ws_a::gen::f<i>` key (for the scale / clamp rows).
fn gen_key(i: usize) -> NodeKey {
    NodeKey::rust_item(NodeKind::Fn, "ws_a", "gen", &format!("f{i}")).expect("gen key")
}

/// `f0 → f1 → … → f<n-1>` over `calls`.
fn call_chain(n: usize) -> (RepoGraph, Vec<NodeKey>) {
    let mut b = GraphBuilder::default();
    let keys: Vec<NodeKey> = (0..n).map(gen_key).collect();
    for (i, k) in keys.iter().enumerate() {
        add(&mut b, k, &format!("f{i}"), "src/gen.rs", "s", "b");
    }
    for w in keys.windows(2) {
        link(&mut b, EdgeKind::Calls, &w[0], &w[1]);
    }
    (b.finish().expect("chain builds").graph, keys)
}

/// A center calling `leaves` distinct leaves over `calls`; returns the center key.
fn star(leaves: usize) -> (RepoGraph, NodeKey) {
    let mut b = GraphBuilder::default();
    let center = gen_key(0);
    add(&mut b, &center, "center", "src/gen.rs", "s", "b");
    for i in 1..=leaves {
        let k = gen_key(i);
        add(&mut b, &k, &format!("f{i}"), "src/gen.rs", "s", "b");
        link(&mut b, EdgeKind::Calls, &center, &k);
    }
    (b.finish().expect("star builds").graph, center)
}

/// `n` unconnected `fn` nodes.
fn many_fns(n: usize) -> (RepoGraph, Vec<NodeKey>) {
    let mut b = GraphBuilder::default();
    let keys: Vec<NodeKey> = (0..n).map(gen_key).collect();
    for (i, k) in keys.iter().enumerate() {
        add(&mut b, k, &format!("f{i}"), "src/gen.rs", "s", "b");
    }
    (b.finish().expect("fns build").graph, keys)
}

/// `src → mid_i → dst` for `mids` distinct mids (that many length-3 paths); returns `(src, dst)`.
fn diamond(mids: usize) -> (RepoGraph, NodeKey, NodeKey) {
    let mut b = GraphBuilder::default();
    let src = gen_key(0);
    let dst = gen_key(1);
    add(&mut b, &src, "src", "src/gen.rs", "s", "b");
    add(&mut b, &dst, "dst", "src/gen.rs", "s", "b");
    for i in 0..mids {
        let m = gen_key(i + 2);
        add(&mut b, &m, &format!("m{i}"), "src/gen.rs", "s", "b");
        link(&mut b, EdgeKind::Calls, &src, &m);
        link(&mut b, EdgeKind::Calls, &m, &dst);
    }
    (b.finish().expect("diamond builds").graph, src, dst)
}

fn node_keys(rows: &[agent_core::repo_graph::NodeRow]) -> Vec<NodeKey> {
    rows.iter().map(|r| r.node.key.clone()).collect()
}

// ===========================================================================
// Repos
// ===========================================================================

pub async fn positive_repo_put_get(h: &Harness) {
    let s = h.a();
    let id = put_repo(&*s, "o__r").await;
    let got = s.repo_get("o__r").await.unwrap().expect("present");
    assert_eq!(got.id, id);
    assert_eq!(got.slug, "o__r");
    assert_eq!(got.forge, "github");
    assert_eq!(got.created_at_ms, h.now_ms());
}

pub async fn positive_repo_put_upsert(h: &Harness) {
    let s = h.a();
    let id1 = s.repo_put(&repo_spec("o__r")).await.unwrap();
    let mut spec2 = repo_spec("o__r");
    spec2.default_branch = "develop".into();
    let id2 = s.repo_put(&spec2).await.unwrap();
    assert_eq!(id1, id2, "same slug keeps the id");
    assert_eq!(
        s.repo_get("o__r").await.unwrap().unwrap().default_branch,
        "develop"
    );
    assert_eq!(s.repos().await.unwrap().len(), 1);
}

pub async fn positive_repos_sorted(h: &Harness) {
    let s = h.a();
    for slug in ["mmm__r", "aaa__r", "zzz__r"] {
        put_repo(&*s, slug).await;
    }
    let slugs: Vec<String> = s
        .repos()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.slug)
        .collect();
    assert_eq!(slugs, vec!["aaa__r", "mmm__r", "zzz__r"]);
}

pub async fn negative_repo_get_unknown(h: &Harness) {
    let s = h.a();
    assert!(s.repo_get("nope__r").await.unwrap().is_none());
}

pub async fn adversarial_repo_slug_unsafe(h: &Harness) {
    let s = h.a();
    for bad in ["../x", "a b", &"x".repeat(129), ""] {
        let mut spec = repo_spec("ok__r");
        spec.slug = bad.into();
        let err = s.repo_put(&spec).await.unwrap_err();
        assert!(
            matches!(err, RepoGraphError::Invalid(ref m) if m == "slug"),
            "{bad:?}: {err:?}"
        );
    }
    assert!(s.repos().await.unwrap().is_empty(), "nothing was written");
}

pub async fn adversarial_repo_profile_huge(h: &Harness) {
    let s = h.a();
    let mut spec = repo_spec("o__r");
    spec.profile = json!({ "k": "x".repeat(5000) });
    let err = s.repo_put(&spec).await.unwrap_err();
    assert!(
        matches!(err, RepoGraphError::TooLong(ref m) if m == "profile"),
        "{err:?}"
    );
}

pub async fn adversarial_repo_remote_control_char(h: &Harness) {
    let s = h.a();
    let mut spec = repo_spec("o__r");
    spec.remote_url = "https://example.com/\u{7}".into();
    let err = s.repo_put(&spec).await.unwrap_err();
    assert!(
        matches!(err, RepoGraphError::Invalid(ref m) if m == "remote_url"),
        "{err:?}"
    );
}

pub async fn adversarial_repo_cross_tenant(h: &Harness) {
    let a = h.a();
    let b = h.b();
    put_repo(&*a, "o__r").await;
    assert!(b.repo_get("o__r").await.unwrap().is_none());
    assert!(b.repos().await.unwrap().is_empty());
}

// ===========================================================================
// Snapshots
// ===========================================================================

pub async fn positive_snapshot_lifecycle(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let id = begin(&*s, repo, &sha(1)).await;
    s.snapshot_write(id, &fixture_v1()).await.unwrap();
    h.advance_secs(5);
    s.snapshot_finish(id, SnapshotStatus::Ready, "done", &ExtractReport::default())
        .await
        .unwrap();
    let snap = s
        .snapshot_find(repo, &sha(1))
        .await
        .unwrap()
        .expect("ready");
    let g = fixture_v1();
    assert_eq!(snap.status, SnapshotStatus::Ready);
    assert_eq!(snap.graph_hash, g.graph_hash());
    assert_eq!(snap.node_count, g.nodes().len());
    assert_eq!(snap.edge_count, g.edges().len());
    assert_eq!(snap.duration_ms, 5_000);
    assert_eq!(snap.reason, "done");
}

pub async fn positive_snapshot_find_by_sha(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let i1 = begin(&*s, repo, &sha(1)).await;
    write_ready(&*s, i1, &fixture_v1()).await;
    let i2 = begin(&*s, repo, &sha(2)).await;
    write_ready(&*s, i2, &fixture_v2()).await;
    assert_eq!(
        s.snapshot_find(repo, &sha(1)).await.unwrap().unwrap().id,
        i1
    );
    assert_eq!(
        s.snapshot_find(repo, &sha(2)).await.unwrap().unwrap().id,
        i2
    );
    assert!(s.snapshot_find(repo, &sha(9)).await.unwrap().is_none());
}

pub async fn positive_snapshot_latest_skips_failed_and_building(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let i1 = begin(&*s, repo, &sha(1)).await;
    write_ready(&*s, i1, &fixture_v1()).await;
    h.advance_secs(1);
    let i2 = begin(&*s, repo, &sha(2)).await;
    s.snapshot_finish(
        i2,
        SnapshotStatus::Failed,
        "boom",
        &ExtractReport::default(),
    )
    .await
    .unwrap();
    h.advance_secs(1);
    let _building = begin(&*s, repo, &sha(3)).await;
    let latest = s.snapshot_latest(repo).await.unwrap().expect("a ready one");
    assert_eq!(latest.id, i1);
}

pub async fn positive_snapshots_newest_first_limited(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let mut ids = Vec::new();
    for n in 0..4 {
        let id = begin(&*s, repo, &sha(n)).await;
        write_ready(&*s, id, &fixture_v1()).await;
        h.advance_secs(1);
        ids.push(id);
    }
    let list = s.snapshots(repo, 2).await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, ids[3]);
    assert_eq!(list[1].id, ids[2]);
    // `limit` is clamped to at least 1.
    assert_eq!(s.snapshots(repo, 0).await.unwrap().len(), 1);
}

pub async fn positive_bodies_shared(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let i1 = begin(&*s, repo, &sha(1)).await;
    write_ready(&*s, i1, &fixture_v1()).await;
    let i2 = begin(&*s, repo, &sha(2)).await;
    write_ready(&*s, i2, &fixture_v1()).await;
    let keys = [key_alpha()];
    let r1 = s.nodes_by_key(Scope::new(repo, i1), &keys).await.unwrap();
    let r2 = s.nodes_by_key(Scope::new(repo, i2), &keys).await.unwrap();
    assert_eq!(r1, r2);
    assert_eq!(r1[0].node.id, node_id_for(&key_alpha()));
}

pub async fn positive_snapshot_diff(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let i1 = begin(&*s, repo, &sha(1)).await;
    write_ready(&*s, i1, &fixture_v1()).await;
    let i2 = begin(&*s, repo, &sha(2)).await;
    write_ready(&*s, i2, &fixture_v2()).await;
    let d = s.snapshot_diff(i1, i2).await.unwrap();
    assert_eq!(d.added, vec![key_delta()]);
    assert_eq!(d.removed, vec![key_struct_s()]);
    assert_eq!(d.sig_changed, vec![key_gamma()]);
    assert_eq!(d.body_changed, vec![key_beta()]);
    assert!(
        d.edges_added
            .iter()
            .any(|e| e.kind == EdgeKind::Calls && e.src == key_gamma() && e.dst == key_alpha()),
        "gamma → alpha added: {:?}",
        d.edges_added
    );
    assert!(
        d.edges_removed
            .iter()
            .any(|e| e.kind == EdgeKind::ImplFor && e.src == key_impl() && e.dst == key_struct_s()),
        "impl_for removed: {:?}",
        d.edges_removed
    );
    assert!(!d.truncated);
}

pub async fn positive_retention(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let mut ids = Vec::new();
    for n in 0..4 {
        let id = begin(&*s, repo, &sha(n)).await;
        write_ready(&*s, id, &fixture_v1()).await;
        h.advance_secs(1);
        ids.push(id);
    }
    let failed = begin(&*s, repo, &sha(8)).await;
    s.snapshot_finish(
        failed,
        SnapshotStatus::Failed,
        "boom",
        &ExtractReport::default(),
    )
    .await
    .unwrap();
    let deleted = s.snapshot_delete_older_than(repo, 2).await.unwrap();
    assert_eq!(deleted, 3, "2 older ready + 1 failed");
    // The two newest ready snapshots are still readable.
    for id in &ids[2..] {
        let rows = s
            .nodes_by_key(Scope::new(repo, *id), &[key_alpha()])
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
    }
    // The oldest ones are gone.
    let err = s
        .nodes_by_key(Scope::new(repo, ids[0]), &[key_alpha()])
        .await
        .unwrap_err();
    assert_eq!(err, RepoGraphError::NotFound);
}

pub async fn negative_begin_unknown_repo(h: &Harness) {
    let s = h.a();
    let err = s
        .snapshot_begin(&begin_spec(RepoId(9_999), &sha(1)))
        .await
        .unwrap_err();
    assert_eq!(err, RepoGraphError::NotFound);
}

pub async fn negative_begin_duplicate_identity(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let _live = begin(&*s, repo, &sha(1)).await;
    let err = s
        .snapshot_begin(&begin_spec(repo, &sha(1)))
        .await
        .unwrap_err();
    assert!(matches!(err, RepoGraphError::Conflict(_)), "{err:?}");
}

pub async fn corner_begin_replaces_failed(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let i1 = begin(&*s, repo, &sha(1)).await;
    s.snapshot_finish(
        i1,
        SnapshotStatus::Failed,
        "boom",
        &ExtractReport::default(),
    )
    .await
    .unwrap();
    let i2 = s.snapshot_begin(&begin_spec(repo, &sha(1))).await.unwrap();
    assert_ne!(i1, i2);
    let live: Vec<_> = s.snapshots(repo, 10).await.unwrap();
    assert!(live.iter().all(|snp| snp.id != i1), "failed one replaced");
    assert!(live.iter().any(|snp| snp.id == i2));
}

pub async fn negative_begin_bad_sha(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    for bad in ["a".repeat(39), "A".repeat(40), "g".repeat(40)] {
        let err = s.snapshot_begin(&begin_spec(repo, &bad)).await.unwrap_err();
        assert!(
            matches!(err, RepoGraphError::Invalid(ref m) if m == "commit_sha"),
            "{bad:?}: {err:?}"
        );
    }
}

pub async fn adversarial_begin_extractor_name(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    for bad in ["../x", &"e".repeat(33)] {
        let mut spec = begin_spec(repo, &sha(1));
        spec.extractors = vec![bad.to_string()];
        let err = s.snapshot_begin(&spec).await.unwrap_err();
        assert!(
            matches!(err, RepoGraphError::Invalid(ref m) if m == "extractor_name"),
            "{bad:?}: {err:?}"
        );
    }
}

pub async fn negative_write_after_finish(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let id = begin(&*s, repo, &sha(1)).await;
    write_ready(&*s, id, &fixture_v1()).await;
    let err = s.snapshot_write(id, &fixture_v1()).await.unwrap_err();
    assert!(matches!(err, RepoGraphError::Conflict(_)), "{err:?}");
}

pub async fn negative_finish_twice(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let id = begin(&*s, repo, &sha(1)).await;
    s.snapshot_finish(id, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap();
    let err = s
        .snapshot_finish(id, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap_err();
    assert!(matches!(err, RepoGraphError::Conflict(_)), "{err:?}");
}

pub async fn negative_finish_unknown(h: &Harness) {
    let s = h.a();
    let err = s
        .snapshot_finish(
            SnapshotId(9_999),
            SnapshotStatus::Ready,
            "",
            &ExtractReport::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(err, RepoGraphError::NotFound);
}

pub async fn corner_write_empty_graph(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let id = begin(&*s, repo, &sha(1)).await;
    let empty = GraphBuilder::default().finish().unwrap().graph;
    s.snapshot_write(id, &empty).await.unwrap();
    s.snapshot_finish(id, SnapshotStatus::Ready, "", &ExtractReport::default())
        .await
        .unwrap();
    let snap = s.snapshot_find(repo, &sha(1)).await.unwrap().unwrap();
    assert_eq!(snap.node_count, 0);
    assert_eq!(snap.edge_count, 0);
    assert_eq!(snap.graph_hash, empty.graph_hash());
}

pub async fn negative_write_id_collision(h: &Harness) {
    let s = h.a();
    let repo = put_repo(&*s, "o__r").await;
    let i1 = begin(&*s, repo, &sha(1)).await;
    write_ready(&*s, i1, &fixture_v1()).await;

    // A second snapshot whose one node has a different key but `alpha`'s id.
    let i2 = begin(&*s, repo, &sha(2)).await;
    let alpha_id = node_id_for(&key_alpha());
    let collide = NodeKey::rust_item(NodeKind::Fn, "ws_a", "m", "zzz").unwrap();
    let ck = collide.clone();
    let mut b =
        GraphBuilder::default().with_id_fn(
            move |k: &NodeKey| {
                if *k == ck {
                    alpha_id
                } else {
                    node_id_for(k)
                }
            },
        );
    b.node(NodeSpec::new(collide, "zzz").with_file("src/m.rs"));
    let g = b.finish().unwrap().graph;
    let err = s.snapshot_write(i2, &g).await.unwrap_err();
    assert!(
        matches!(err, RepoGraphError::Conflict(ref m) if m.contains("collision")),
        "{err:?}"
    );
    // Nothing was written; the snapshot is still building.
    let snap = s
        .snapshots(repo, 10)
        .await
        .unwrap()
        .into_iter()
        .find(|snp| snp.id == i2)
        .unwrap();
    assert_eq!(snap.status, SnapshotStatus::Building);
    assert_eq!(snap.node_count, 0);
}

pub async fn adversarial_snapshot_cross_repo(h: &Harness) {
    let s = h.a();
    let repo_a = put_repo(&*s, "a__r").await;
    let repo_b = put_repo(&*s, "b__r").await;
    let ia = begin(&*s, repo_a, &sha(1)).await;
    write_ready(&*s, ia, &fixture_v1()).await;
    // The snapshot is real, but addressed under the wrong repo.
    let err = s
        .nodes_by_key(Scope::new(repo_b, ia), &[key_alpha()])
        .await
        .unwrap_err();
    assert_eq!(err, RepoGraphError::NotFound);
}

pub async fn adversarial_snapshot_cross_tenant(h: &Harness) {
    let a = h.a();
    let repo = put_repo(&*a, "o__r").await;
    let ia = begin(&*a, repo, &sha(1)).await;
    write_ready(&*a, ia, &fixture_v1()).await;
    let b = h.b();
    let err = b
        .nodes_by_key(Scope::new(repo, ia), &[key_alpha()])
        .await
        .unwrap_err();
    assert_eq!(err, RepoGraphError::NotFound);
}

pub async fn adversarial_diff_cross_repo(h: &Harness) {
    let s = h.a();
    let repo_a = put_repo(&*s, "a__r").await;
    let repo_b = put_repo(&*s, "b__r").await;
    let ia = begin(&*s, repo_a, &sha(1)).await;
    write_ready(&*s, ia, &fixture_v1()).await;
    let ib = begin(&*s, repo_b, &sha(1)).await;
    write_ready(&*s, ib, &fixture_v2()).await;
    let err = s.snapshot_diff(ia, ib).await.unwrap_err();
    assert_eq!(err, RepoGraphError::NotFound);
}

// ===========================================================================
// Reads
// ===========================================================================

pub async fn positive_nodes_by_key(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let rows = s
        .nodes_by_key(sc, &[key_gamma(), key_alpha()])
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].node.key, key_alpha(), "sorted by key");
    assert_eq!(rows[1].node.key, key_gamma());
}

pub async fn positive_nodes_by_file(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let rows = s.nodes_by_file(sc, &["src/m.rs".into()]).await.unwrap();
    let keys = node_keys(&rows);
    assert!(keys.contains(&key_alpha()));
    assert!(keys.contains(&key_mod()));
    assert!(keys.contains(&key_test_alpha()));
    assert!(!keys.contains(&key_trait_t()), "trait T is in lib.rs");
}

pub async fn positive_nodes_by_name_kind_filter(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    // `S` is both the struct and the impl's display name.
    let all = s.nodes_by_name(sc, "S", None, 500).await.unwrap();
    assert_eq!(all.len(), 2);
    let structs = s
        .nodes_by_name(sc, "S", Some(NodeKind::Struct), 500)
        .await
        .unwrap();
    assert_eq!(structs.len(), 1);
    assert_eq!(structs[0].node.key, key_struct_s());
}

pub async fn negative_unknown_key_empty(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let unknown = NodeKey::rust_item(NodeKind::Fn, "ws_a", "m", "nonexistent").unwrap();
    assert!(s.nodes_by_key(sc, &[unknown]).await.unwrap().is_empty());
}

pub async fn adversarial_name_huge(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let huge = "a".repeat(1 << 20);
    assert!(s
        .nodes_by_name(sc, &huge, None, 500)
        .await
        .unwrap()
        .is_empty());
}

pub async fn adversarial_name_whitespace(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    assert!(s
        .nodes_by_name(sc, "al pha", None, 500)
        .await
        .unwrap()
        .is_empty());
}

pub async fn positive_neighbors_out_calls_2_hops(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let seeds = [node_id_for(&key_alpha())];
    let ns = s
        .neighbors(sc, &seeds, EdgeKind::Calls, Direction::Out, 2, 500)
        .await
        .unwrap();
    assert_eq!(ns.len(), 2);
    assert_eq!(ns[0].row.node.key, key_beta());
    assert_eq!(ns[0].depth, 1);
    assert_eq!(ns[1].row.node.key, key_gamma());
    assert_eq!(ns[1].depth, 2);
}

pub async fn positive_neighbors_in(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let seeds = [node_id_for(&key_gamma())];
    let ns = s
        .neighbors(sc, &seeds, EdgeKind::Calls, Direction::In, 2, 500)
        .await
        .unwrap();
    assert_eq!(ns.len(), 2);
    assert_eq!(ns[0].row.node.key, key_beta());
    assert_eq!(ns[0].depth, 1);
    assert_eq!(ns[1].row.node.key, key_alpha());
    assert_eq!(ns[1].depth, 2);
}

pub async fn corner_neighbors_cycle_terminates(h: &Harness) {
    let s = h.a();
    // v2 has the `gamma → alpha` cycle.
    let sc = seed(&*s, "o__r", 1, &fixture_v2()).await;
    let seeds = [node_id_for(&key_alpha())];
    let ns = s
        .neighbors(sc, &seeds, EdgeKind::Calls, Direction::Out, 4, 500)
        .await
        .unwrap();
    let keys: HashSet<NodeKey> = ns.iter().map(|n| n.row.node.key.clone()).collect();
    assert!(keys.contains(&key_beta()));
    assert!(keys.contains(&key_gamma()));
    assert!(keys.contains(&key_delta()));
    assert!(
        !keys.contains(&key_alpha()),
        "seed excluded despite the cycle"
    );
}

pub async fn boundary_hops_clamped(h: &Harness) {
    let s = h.a();
    let (g, keys) = call_chain(8);
    let sc = seed(&*s, "o__r", 1, &g).await;
    let ns = s
        .neighbors(
            sc,
            &[node_id_for(&keys[0])],
            EdgeKind::Calls,
            Direction::Out,
            99,
            500,
        )
        .await
        .unwrap();
    // 99 hops clamp to MAX_NEIGHBOR_HOPS = 4.
    assert_eq!(ns.len(), 4);
    assert_eq!(ns.iter().map(|n| n.depth).max().unwrap(), 4);
}

pub async fn boundary_cap_clamped(h: &Harness) {
    let s = h.a();
    let (g, center) = star(600);
    let sc = seed(&*s, "o__r", 1, &g).await;
    let ns = s
        .neighbors(
            sc,
            &[node_id_for(&center)],
            EdgeKind::Calls,
            Direction::Out,
            1,
            10_000,
        )
        .await
        .unwrap();
    assert_eq!(ns.len(), MAX_RESULT);
}

pub async fn boundary_keys_64(h: &Harness) {
    let s = h.a();
    let (g, keys) = many_fns(70);
    let sc = seed(&*s, "o__r", 1, &g).await;
    let query: Vec<NodeKey> = keys.iter().take(65).cloned().collect();
    let rows = s.nodes_by_key(sc, &query).await.unwrap();
    // 65 keys clamp to MAX_KEYS = 64.
    assert_eq!(rows.len(), 64);
}

pub async fn positive_blast_radius(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let files = s
        .blast_radius(sc, &["src/m.rs".into()], 3, 500)
        .await
        .unwrap();
    // `run` (src/lib.rs) calls into src/m.rs, so it is in the radius.
    assert!(files.contains(&"src/lib.rs".to_string()), "{files:?}");
}

pub async fn positive_tests_covering_via(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let hits = s
        .tests_covering(sc, &[node_id_for(&key_alpha())], 3, 500)
        .await
        .unwrap();
    let hit = hits
        .iter()
        .find(|t| t.row.node.key == key_test_alpha())
        .expect("positive_alpha_ok covers alpha");
    assert_eq!(hit.via, "name");
}

pub async fn positive_path_between(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let paths = s
        .path_between(
            sc,
            node_id_for(&key_alpha()),
            node_id_for(&key_gamma()),
            6,
            32,
        )
        .await
        .unwrap();
    assert_eq!(paths, vec![vec![key_alpha(), key_beta(), key_gamma()]]);
}

pub async fn negative_path_none(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    // No outbound call path from gamma back to alpha in v1.
    let paths = s
        .path_between(
            sc,
            node_id_for(&key_gamma()),
            node_id_for(&key_alpha()),
            6,
            32,
        )
        .await
        .unwrap();
    assert!(paths.is_empty());
}

pub async fn corner_path_cycle_guard(h: &Harness) {
    let s = h.a();
    // v2's `gamma → alpha` closes a cycle; the search must still terminate with simple paths.
    let sc = seed(&*s, "o__r", 1, &fixture_v2()).await;
    let paths = s
        .path_between(
            sc,
            node_id_for(&key_alpha()),
            node_id_for(&key_gamma()),
            6,
            32,
        )
        .await
        .unwrap();
    assert!(paths.contains(&vec![key_alpha(), key_beta(), key_gamma()]));
    for p in &paths {
        let uniq: HashSet<&NodeKey> = p.iter().collect();
        assert_eq!(uniq.len(), p.len(), "no path revisits a node");
    }
}

pub async fn boundary_paths_clamped(h: &Harness) {
    let s = h.a();
    let (g, src, dst) = diamond(50);
    let sc = seed(&*s, "o__r", 1, &g).await;
    let paths = s
        .path_between(sc, node_id_for(&src), node_id_for(&dst), 6, 10_000)
        .await
        .unwrap();
    // 50 distinct paths clamp to MAX_PATHS = 32.
    assert_eq!(paths.len(), MAX_PATHS);
}

pub async fn positive_shape(h: &Harness) {
    let s = h.a();
    let sc = seed(&*s, "o__r", 1, &fixture_v1()).await;
    let shape = s.shape(sc).await.unwrap();
    assert_eq!(shape.files, 2);
    assert_eq!(shape.crates, 1);
    assert_eq!(
        shape.nodes_by_kind.get(&NodeKind::Fn).copied().unwrap_or(0),
        3
    );
    assert_eq!(
        shape
            .edges_by_kind
            .get(&EdgeKind::Calls)
            .copied()
            .unwrap_or(0),
        3
    );
    assert_eq!(shape.snapshot.id, sc.snapshot);
}
