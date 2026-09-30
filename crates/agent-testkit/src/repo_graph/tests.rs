//! `MemRepoGraph`-only behaviour (tenant refusal, shared state across handles, clock injection),
//! plus the shared R3 rows stamped in by the conformance suite as
//! `repo_graph::tests::mem::r3::<row>`.

use super::conformance::Harness;
use super::MemRepoGraph;
use agent_core::repo_graph::{RepoGraphError, RepoGraphStore, RepoSpec};
use rstest::rstest;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

fn repo_spec(slug: &str) -> RepoSpec {
    RepoSpec {
        slug: slug.into(),
        forge: "github".into(),
        remote_url: "https://example.com/o/r.git".into(),
        default_branch: "main".into(),
        profile: serde_json::json!({}),
    }
}

#[rstest]
#[case::adversarial_tenant_traversal("../x")]
#[case::adversarial_tenant_space("a b")]
#[case::adversarial_tenant_slash("a/b")]
#[case::adversarial_tenant_dash("-x")]
#[case::adversarial_tenant_dotdot("..")]
#[case::adversarial_tenant_long(&"t".repeat(129))]
#[case::adversarial_tenant_empty("")]
fn adversarial_tenant_unsafe(#[case] tenant: &str) {
    let store = MemRepoGraph::new();
    let err = store.with_tenant(tenant).unwrap_err();
    assert!(
        matches!(err, RepoGraphError::Invalid(ref m) if m.starts_with("tenant:")),
        "{tenant:?}: {err}"
    );
}

#[tokio::test]
async fn positive_tenant_handles_share_state() {
    let a = MemRepoGraph::new().with_tenant("ta").unwrap();
    let repo = a.repo_put(&repo_spec("o__r")).await.unwrap();

    // A clone is the same backend under the same tenant.
    let a2 = a.clone();
    assert_eq!(a2.repo_get("o__r").await.unwrap().unwrap().id, repo);

    // Another tenant shares the backend but sees none of ta's rows.
    let b = a.with_tenant("tb").unwrap();
    assert_eq!(b.tenant(), "tb");
    assert!(b.repo_get("o__r").await.unwrap().is_none());

    // Identities are global: tb's first repo gets the next id, not 1 again.
    let rb = b.repo_put(&repo_spec("o__r")).await.unwrap();
    assert_eq!(rb.0, repo.0 + 1);
    assert!(a.repo_get("o__r").await.unwrap().unwrap().id == repo);
}

#[tokio::test]
async fn positive_clock_stamps_rows() {
    let clock = Arc::new(AtomicU64::new(1_000));
    let c = Arc::clone(&clock);
    let store = MemRepoGraph::new()
        .with_clock(Arc::new(move || c.load(Ordering::SeqCst)))
        .with_tenant("ta")
        .unwrap();
    let repo = store.repo_put(&repo_spec("o__r")).await.unwrap();
    assert_eq!(
        store.repo_get("o__r").await.unwrap().unwrap().created_at_ms,
        1_000
    );
    clock.store(9_000, Ordering::SeqCst);
    let repo2 = store.repo_put(&repo_spec("o2__r")).await.unwrap();
    assert_ne!(repo, repo2);
    assert_eq!(
        store
            .repo_get("o2__r")
            .await
            .unwrap()
            .unwrap()
            .created_at_ms,
        9_000
    );
}

#[test]
fn positive_debug_names_tenant_only() {
    let store = MemRepoGraph::new().with_tenant("ta").unwrap();
    let dbg = format!("{store:?}");
    assert!(dbg.contains("ta") && !dbg.contains("repos"), "{dbg}");
}

// The shared R3 rows (`08-test-matrix.md`) against `MemRepoGraph`:
// `repo_graph::tests::mem::r3::positive_repo_put_get`, …
crate::repo_graph_conformance_suite!(mem, Harness::mem());
