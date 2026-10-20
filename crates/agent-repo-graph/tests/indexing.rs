#![cfg(feature = "index")]
mod common;
use agent_core::repo_graph::*;
use agent_repo_graph::index::*;
use common::*;
use rstest::rstest;
use std::sync::Arc;

#[tokio::test]
async fn positive_index_reuse_diff() {
    let git = GitFixture::new();
    let store = Arc::new(MemRepoGraph::new());
    git.register(store.as_ref()).await;
    let indexer = git.indexer(store.clone(), IndexOptions::default());
    let a = indexer.index("owner__repo", "HEAD").await.unwrap();
    let again = indexer.index("owner__repo", &git.a).await.unwrap();
    assert!(!a.publication.reused);
    assert!(again.publication.reused);
    assert_eq!(a.publication.snapshot.id, again.publication.snapshot.id);
    let b = git.change();
    let result = indexer.index("owner__repo", &b).await.unwrap();
    let diff = store
        .snapshot_diff(a.publication.snapshot.id, result.publication.snapshot.id)
        .await
        .unwrap();
    assert!(!diff.added.is_empty());
    assert!(!result.checkout_cleanup_failed);
    assert!(!result.retention_failed);
    assert!(std::fs::read_dir(git.dir.join("worktrees"))
        .unwrap()
        .next()
        .is_none());
}
#[rstest]
#[case::adversarial_dirty("dirty")]
#[case::adversarial_wrong_remote("remote")]
#[case::negative_unregistered("missing")]
#[case::adversarial_revision("revision")]
#[case::adversarial_other_tenant("tenant")]
#[tokio::test]
async fn negative_index_input(#[case] mode: &str) {
    let git = GitFixture::new();
    let base = MemRepoGraph::new();
    let repo = git.register(&base).await;
    if mode == "dirty" {
        std::fs::write(git.root.join("src/lib.rs"), "dirty").unwrap();
    }
    if mode == "remote" {
        common::git(
            &git.root,
            &[
                "remote",
                "set-url",
                "origin",
                "https://example.invalid/wrong.git",
            ],
        );
    }
    let store = Arc::new(if mode == "tenant" {
        base.with_tenant("other").unwrap()
    } else {
        base.clone()
    });
    let indexer = git.indexer(store, IndexOptions::default());
    assert!(indexer
        .index(
            if mode == "missing" {
                "missing"
            } else {
                "owner__repo"
            },
            if mode == "revision" { "--help" } else { "HEAD" }
        )
        .await
        .is_err());
    assert!(base.snapshots(repo, 200).await.unwrap().is_empty());
}
#[tokio::test]
async fn boundary_budget_identity() {
    let git = GitFixture::new();
    let store = Arc::new(MemRepoGraph::new());
    git.register(store.as_ref()).await;
    let a = git
        .indexer(
            store.clone(),
            IndexOptions {
                max_files: 1,
                ..Default::default()
            },
        )
        .index("owner__repo", "HEAD")
        .await
        .unwrap();
    assert!(a.report.unwrap().truncated);
    let b = git
        .indexer(store, IndexOptions::default())
        .index("owner__repo", "HEAD")
        .await
        .unwrap();
    assert!(!b.publication.reused);
    assert_ne!(
        a.publication.snapshot.extractor_version,
        b.publication.snapshot.extractor_version
    );
}
#[test]
fn corner_identity_normalization() {
    let a = IndexOptions::default();
    let mut b = a.clone();
    b.extractors.reverse();
    assert_eq!(a.identity().unwrap(), b.identity().unwrap());
    b.retention = 2;
    assert_eq!(a.identity().unwrap(), b.identity().unwrap());
    assert_eq!(
        remote_identity("git@github.com:owner/repo.git").unwrap(),
        remote_identity("https://github.com/owner/repo").unwrap()
    );
}
#[rstest]
#[case::adversarial_extractor("scip")]
#[case::negative_unknown("typo")]
fn negative_extractor_selection(#[case] extractor: &str) {
    assert!(IndexOptions {
        extractors: vec![extractor.into()],
        ..Default::default()
    }
    .validate()
    .is_err());
}
