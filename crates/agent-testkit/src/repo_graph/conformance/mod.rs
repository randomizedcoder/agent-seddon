//! The repo-graph conformance suite: one `pub async fn <row_id>(h: &Harness)` per shared row of
//! `docs/design/repo-knowledge/08-test-matrix.md` R3, written once against the [`RepoGraphStore`]
//! trait and stamped into a tier's tests by
//! [`repo_graph_conformance_suite!`](crate::repo_graph_conformance_suite). The row list lives in
//! the macro, so `PgRepoGraph` (RK-02) cannot drift from [`MemRepoGraph`]; the generated names are
//! `<tier>::r3::positive_repo_put_get`, …, matching the doc's ids.
//!
//! The two fixtures ([`fixture_v1`] / [`fixture_v2`]) are built through the one [`GraphBuilder`],
//! so a tier only ever ingests graphs the real builder would produce (sorted, hashed, id-checked).
//! `v2` is `v1` with `beta`'s body changed, `gamma`'s signature changed, `delta` added, `S` (and
//! its edges) removed, and a `gamma → alpha` call added (the cycle the path / neighbor rows walk).

use crate::repo_graph::MemRepoGraph;
use agent_core::repo_graph::{
    EdgeKind, EdgeOutcome, GraphBuilder, NodeKey, NodeKind, NodeOutcome, NodeSpec, RepoGraph,
    RepoGraphResult, RepoGraphStore,
};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub mod r3;

type Open = dyn Fn(&str) -> RepoGraphResult<Arc<dyn RepoGraphStore>> + Send + Sync;

/// One tier under test: a clock the rows can advance and a factory that opens the backend under a
/// tenant. RK-02 builds a `pg` harness the same way, so R3 runs unchanged against Postgres.
pub struct Harness {
    pub clock: Arc<AtomicU64>,
    open: Arc<Open>,
}

impl Harness {
    /// A fresh [`MemRepoGraph`] on a settable clock.
    pub fn mem() -> Harness {
        let clock = Arc::new(AtomicU64::new(1_700_000_000_000));
        let c = Arc::clone(&clock);
        let base = MemRepoGraph::new().with_clock(Arc::new(move || c.load(Ordering::SeqCst)));
        Harness::from_factory(
            clock,
            Arc::new(move |tenant| {
                base.with_tenant(tenant)
                    .map(|s| Arc::new(s) as Arc<dyn RepoGraphStore>)
            }),
        )
    }

    /// Any tier: `open(tenant)` must return a store bound to `tenant` that reads `clock` for its
    /// time.
    pub fn from_factory(clock: Arc<AtomicU64>, open: Arc<Open>) -> Harness {
        Harness { clock, open }
    }

    /// The store under `tenant`; a tenant the tier refuses is a test failure.
    pub fn store(&self, tenant: &str) -> Arc<dyn RepoGraphStore> {
        (self.open)(tenant).unwrap_or_else(|e| panic!("open tenant {tenant:?}: {e}"))
    }

    /// The store under `tenant`, or the tier's refusal (for the tenant rows).
    pub fn try_store(&self, tenant: &str) -> RepoGraphResult<Arc<dyn RepoGraphStore>> {
        (self.open)(tenant)
    }

    /// Tenant A (`ta`).
    pub fn a(&self) -> Arc<dyn RepoGraphStore> {
        self.store("ta")
    }

    /// Tenant B (`tb`).
    pub fn b(&self) -> Arc<dyn RepoGraphStore> {
        self.store("tb")
    }

    pub fn now_ms(&self) -> u64 {
        self.clock.load(Ordering::SeqCst)
    }

    pub fn advance_secs(&self, secs: u64) {
        self.clock.fetch_add(secs * 1000, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Fixture keys (a small `ws_a` crate; shared by the rows so they can name nodes)
// ---------------------------------------------------------------------------

/// `rust:crate:ws_a`.
pub fn key_crate() -> NodeKey {
    NodeKey::rust_crate("ws_a").expect("crate key")
}
/// `rust:mod:ws_a::m`.
pub fn key_mod() -> NodeKey {
    NodeKey::rust_mod("ws_a", "m").expect("mod key")
}
/// `rust:fn:ws_a::m::alpha`.
pub fn key_alpha() -> NodeKey {
    NodeKey::rust_item(NodeKind::Fn, "ws_a", "m", "alpha").expect("alpha key")
}
/// `rust:fn:ws_a::m::beta`.
pub fn key_beta() -> NodeKey {
    NodeKey::rust_item(NodeKind::Fn, "ws_a", "m", "beta").expect("beta key")
}
/// `rust:fn:ws_a::m::gamma`.
pub fn key_gamma() -> NodeKey {
    NodeKey::rust_item(NodeKind::Fn, "ws_a", "m", "gamma").expect("gamma key")
}
/// `rust:fn:ws_a::m::delta` (v2 only).
pub fn key_delta() -> NodeKey {
    NodeKey::rust_item(NodeKind::Fn, "ws_a", "m", "delta").expect("delta key")
}
/// `rust:struct:ws_a::S` (removed in v2).
pub fn key_struct_s() -> NodeKey {
    NodeKey::rust_item(NodeKind::Struct, "ws_a", "", "S").expect("struct key")
}
/// `rust:trait:ws_a::T`.
pub fn key_trait_t() -> NodeKey {
    NodeKey::rust_item(NodeKind::Trait, "ws_a", "", "T").expect("trait key")
}
/// `rust:impl:ws_a::S#T`.
pub fn key_impl() -> NodeKey {
    NodeKey::rust_impl("ws_a", "", "S", Some("T")).expect("impl key")
}
/// `rust:method:ws_a::S#T::run`.
pub fn key_method_run() -> NodeKey {
    NodeKey::rust_method("ws_a", "", "S", Some("T"), "run").expect("method key")
}
/// `rust:test:ws_a::m::positive_alpha_ok`.
pub fn key_test_alpha() -> NodeKey {
    NodeKey::rust_test("ws_a", "m", "positive_alpha_ok", None).expect("test key")
}
/// `rust:test:ws_a::positive_run_ok`.
pub fn key_test_run() -> NodeKey {
    NodeKey::rust_test("ws_a", "", "positive_run_ok", None).expect("test key")
}
/// `rust:feature:ws_a/extra`.
pub fn key_feature() -> NodeKey {
    NodeKey::rust_feature("ws_a", "extra").expect("feature key")
}
/// `doc:docs/a.md`.
pub fn key_doc() -> NodeKey {
    NodeKey::doc("docs/a.md").expect("doc key")
}
/// `file:src/lib.rs`.
pub fn key_file_lib() -> NodeKey {
    NodeKey::file("src/lib.rs").expect("file key")
}
/// `file:src/m.rs`.
pub fn key_file_m() -> NodeKey {
    NodeKey::file("src/m.rs").expect("file key")
}

const FILE_LIB: &str = "src/lib.rs";
const FILE_M: &str = "src/m.rs";

fn add(b: &mut GraphBuilder, key: &NodeKey, name: &str, file: &str, sig: &str, body: &str) {
    let mut spec = NodeSpec::new(key.clone(), name)
        .with_lines(1, 2)
        .with_sig(sig)
        .with_body(body);
    if !file.is_empty() {
        spec = spec.with_file(file);
    }
    let out = b.node(spec);
    assert!(
        matches!(out, NodeOutcome::Added(_)),
        "fixture node {name}: {out:?}"
    );
}

fn link(b: &mut GraphBuilder, kind: EdgeKind, src: &NodeKey, dst: &NodeKey) {
    let out = b.edge(kind, src, dst, 1.0, serde_json::Value::Null);
    assert_eq!(out, EdgeOutcome::Added, "fixture edge {kind:?}");
}

fn link_via(b: &mut GraphBuilder, kind: EdgeKind, src: &NodeKey, dst: &NodeKey, via: &str) {
    let out = b.edge(kind, src, dst, 1.0, json!({ "via": via }));
    assert_eq!(out, EdgeOutcome::Added, "fixture edge {kind:?}");
}

/// Build the fixture (`v2 = false` → v1). See the module docs for the v1→v2 delta.
fn fixture(v2: bool) -> RepoGraph {
    let (krate, m) = (key_crate(), key_mod());
    let (alpha, beta, gamma) = (key_alpha(), key_beta(), key_gamma());
    let (s, t, imp, run) = (key_struct_s(), key_trait_t(), key_impl(), key_method_run());
    let (ta, tr) = (key_test_alpha(), key_test_run());
    let (feat, doc) = (key_feature(), key_doc());
    let (file_lib, file_m) = (key_file_lib(), key_file_m());

    let mut b = GraphBuilder::default();

    // -- nodes (bodies shared across snapshots) ----------------------------
    add(&mut b, &krate, "ws_a", "", "", "");
    add(&mut b, &file_lib, "lib.rs", "", "", "");
    add(&mut b, &file_m, "m.rs", "", "", "");
    add(&mut b, &m, "m", FILE_M, "mod_sig", "mod_body");
    add(&mut b, &alpha, "alpha", FILE_M, "alpha_sig", "alpha_body");
    add(
        &mut b,
        &beta,
        "beta",
        FILE_M,
        "beta_sig",
        if v2 { "beta_body_v2" } else { "beta_body_v1" },
    );
    add(
        &mut b,
        &gamma,
        "gamma",
        FILE_M,
        if v2 { "gamma_sig_v2" } else { "gamma_sig_v1" },
        "gamma_body",
    );
    add(&mut b, &t, "T", FILE_LIB, "t_sig", "t_body");
    add(&mut b, &imp, "S", FILE_LIB, "impl_sig", "impl_body");
    add(&mut b, &run, "run", FILE_LIB, "run_sig", "run_body");
    add(
        &mut b,
        &ta,
        "positive_alpha_ok",
        FILE_M,
        "ta_sig",
        "ta_body",
    );
    add(
        &mut b,
        &tr,
        "positive_run_ok",
        FILE_LIB,
        "tr_sig",
        "tr_body",
    );
    add(&mut b, &feat, "extra", "", "", "");
    add(&mut b, &doc, "a.md", "", "", "");
    if v2 {
        add(
            &mut b,
            &key_delta(),
            "delta",
            FILE_M,
            "delta_sig",
            "delta_body",
        );
    } else {
        add(&mut b, &s, "S", FILE_LIB, "s_sig", "s_body");
    }

    // -- edges common to both snapshots ------------------------------------
    link(&mut b, EdgeKind::Contains, &krate, &m);
    link(&mut b, EdgeKind::Contains, &m, &alpha);
    link(&mut b, EdgeKind::Contains, &m, &beta);
    link(&mut b, EdgeKind::Contains, &m, &gamma);
    link(&mut b, EdgeKind::Contains, &krate, &t);
    link(&mut b, EdgeKind::Contains, &krate, &imp);
    link(&mut b, EdgeKind::Contains, &imp, &run);

    link(&mut b, EdgeKind::DefinedIn, &m, &file_m);
    link(&mut b, EdgeKind::DefinedIn, &alpha, &file_m);
    link(&mut b, EdgeKind::DefinedIn, &beta, &file_m);
    link(&mut b, EdgeKind::DefinedIn, &gamma, &file_m);
    link(&mut b, EdgeKind::DefinedIn, &t, &file_lib);
    link(&mut b, EdgeKind::DefinedIn, &run, &file_lib);

    link(&mut b, EdgeKind::Calls, &alpha, &beta);
    link(&mut b, EdgeKind::Calls, &beta, &gamma);
    link(&mut b, EdgeKind::Calls, &run, &alpha);

    link(&mut b, EdgeKind::Implements, &imp, &t);
    link(&mut b, EdgeKind::GatedBy, &gamma, &feat);
    link_via(&mut b, EdgeKind::Tests, &ta, &alpha, "name");
    link_via(&mut b, EdgeKind::Tests, &tr, &run, "name");
    link(&mut b, EdgeKind::Documents, &doc, &krate);

    // -- the v1 / v2 delta -------------------------------------------------
    if v2 {
        let delta = key_delta();
        link(&mut b, EdgeKind::Contains, &m, &delta);
        link(&mut b, EdgeKind::Calls, &gamma, &delta);
        link(&mut b, EdgeKind::Calls, &gamma, &alpha); // the cycle
    } else {
        link(&mut b, EdgeKind::Contains, &krate, &s);
        link(&mut b, EdgeKind::DefinedIn, &s, &file_lib);
        link(&mut b, EdgeKind::ImplFor, &imp, &s);
        link(&mut b, EdgeKind::Imports, &m, &s);
    }

    b.finish().expect("fixture builds").graph
}

/// The v1 fixture graph.
pub fn fixture_v1() -> RepoGraph {
    fixture(false)
}

/// The v2 fixture graph (see the module docs for the delta).
pub fn fixture_v2() -> RepoGraph {
    fixture(true)
}

// ---------------------------------------------------------------------------
// The suite macro
// ---------------------------------------------------------------------------

/// Stamp the repo-graph conformance suite into a tier's tests.
///
/// ```ignore
/// agent_testkit::repo_graph_conformance_suite!(mem, Harness::mem());
/// agent_testkit::repo_graph_conformance_suite!(pg, pg_harness().await,
///     ignore = "needs a live Postgres");
/// ```
///
/// Generates `mod <tier> { mod r3 { #[tokio::test] async fn <row>() … } }`. `$make` is evaluated
/// once per test (it may `.await`); `after` names an `async fn(&Harness)` run after every row.
#[macro_export]
macro_rules! repo_graph_conformance_suite {
    ($m:ident, $make:expr) => {
        $crate::repo_graph_conformance_suite!(@gen $m, $make, [], []);
    };
    ($m:ident, $make:expr, after = $after:path) => {
        $crate::repo_graph_conformance_suite!(@gen $m, $make, [$after], []);
    };
    ($m:ident, $make:expr, ignore = $why:literal) => {
        $crate::repo_graph_conformance_suite!(@gen $m, $make, [], [ignore = $why]);
    };
    ($m:ident, $make:expr, after = $after:path, ignore = $why:literal) => {
        $crate::repo_graph_conformance_suite!(@gen $m, $make, [$after], [ignore = $why]);
    };
    (@gen $m:ident, $make:expr, $after:tt, $ig:tt) => {
        mod $m {
            #[allow(unused_imports)]
            use super::*;

            $crate::__repo_graph_table!(r3, $make, $after, $ig, [
                positive_repo_put_get,
                positive_repo_put_upsert,
                positive_repos_sorted,
                negative_repo_get_unknown,
                adversarial_repo_slug_unsafe,
                adversarial_repo_profile_huge,
                adversarial_repo_remote_control_char,
                adversarial_repo_cross_tenant,
                positive_snapshot_lifecycle,
                positive_snapshot_find_by_sha,
                positive_snapshot_latest_skips_failed_and_building,
                positive_snapshots_newest_first_limited,
                positive_bodies_shared,
                positive_snapshot_diff,
                positive_retention,
                negative_begin_unknown_repo,
                negative_begin_duplicate_identity,
                corner_begin_replaces_failed,
                negative_begin_bad_sha,
                adversarial_begin_extractor_name,
                negative_write_after_finish,
                negative_finish_twice,
                negative_finish_unknown,
                corner_write_empty_graph,
                negative_write_id_collision,
                adversarial_snapshot_cross_repo,
                adversarial_snapshot_cross_tenant,
                adversarial_diff_cross_repo,
                positive_nodes_by_key,
                positive_nodes_by_file,
                positive_nodes_by_name_kind_filter,
                negative_unknown_key_empty,
                adversarial_name_huge,
                adversarial_name_whitespace,
                positive_neighbors_out_calls_2_hops,
                positive_neighbors_in,
                corner_neighbors_cycle_terminates,
                boundary_hops_clamped,
                boundary_cap_clamped,
                boundary_keys_64,
                positive_blast_radius,
                positive_tests_covering_via,
                positive_path_between,
                negative_path_none,
                corner_path_cycle_guard,
                boundary_paths_clamped,
                positive_shape,
            ]);
        }
    };
}

/// One table of the suite (internal to [`repo_graph_conformance_suite!`]).
#[doc(hidden)]
#[macro_export]
macro_rules! __repo_graph_table {
    ($t:ident, $make:expr, $after:tt, $ig:tt, [$($row:ident),* $(,)?]) => {
        mod $t {
            #[allow(unused_imports)]
            use super::*;

            $( $crate::__repo_graph_row!($t, $row, $make, $after, $ig); )*
        }
    };
}

/// One row of the suite (internal to [`repo_graph_conformance_suite!`]).
#[doc(hidden)]
#[macro_export]
macro_rules! __repo_graph_row {
    ($t:ident, $row:ident, $make:expr, [$($after:path)?], [$($ig:meta)?]) => {
        #[$crate::tokio::test]
        $(#[$ig])?
        async fn $row() {
            let h: $crate::repo_graph::conformance::Harness = $make;
            $crate::repo_graph::conformance::$t::$row(&h).await;
            $( $after(&h).await; )?
        }
    };
}
