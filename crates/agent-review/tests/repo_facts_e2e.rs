//! Integration tests for the repo/change/git-state collector against a real git
//! repo in a temp dir (mirrors `agent-git`'s `objects_fixture`). Requires `git`
//! on `PATH` — supplied by the nix dev shell.

use agent_core::{
    ForgeHost, RepoLanguage, RepoRelation, ReviewCollector, ReviewTarget, TaskClassifier,
};
use agent_git::CliBackend;
use agent_review::{HybridClassifier, ReviewOrchestrator};
use agent_testkit::tempdir;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// `main` (a.txt) + a `feature` branch that modifies a.txt and adds b.txt.
fn fixture() -> PathBuf {
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);

    git(&dir, &["switch", "-q", "-c", "feature"]);
    std::fs::write(dir.join("a.txt"), "hello\nmore\n").unwrap();
    std::fs::write(dir.join("b.txt"), "world\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "feature work"]);
    git(&dir, &["switch", "-q", "main"]);
    dir
}

fn backend(root: &Path) -> Arc<CliBackend> {
    Arc::new(CliBackend::new(
        root,
        root.join(".agent-seddon/mirror"),
        root.join(".agent-seddon/worktrees"),
        "",
    ))
}

fn orchestrator(root: &Path) -> ReviewOrchestrator {
    ReviewOrchestrator::new(root, backend(root), None, None)
}

#[tokio::test]
async fn positive_collects_change_set_and_git_state() {
    let dir = fixture();
    let facts = orchestrator(&dir)
        .collect(&ReviewTarget::Branch("feature".into()))
        .await
        .expect("collect");

    // Change set: a.txt modified, b.txt added.
    assert_eq!(facts.change.files.len(), 2, "two changed files");
    assert!(facts.change.repo_file_count >= 2);
    let names: Vec<String> = facts
        .change
        .files
        .iter()
        .map(|f| f.path.display().to_string())
        .collect();
    assert!(names.iter().any(|n| n.ends_with("a.txt")));
    assert!(names.iter().any(|n| n.ends_with("b.txt")));

    // Git state: default branch main, no remote ⇒ unknown relationship/host.
    assert_eq!(facts.git_state.default_branch, "main");
    assert_eq!(facts.git_state.relationship, RepoRelation::Unknown);
    assert_eq!(facts.git_state.host, ForgeHost::None);
    assert!(facts.git_state.remote_url_hash.is_empty());

    // Meta: the collector ran and is recorded with a duration.
    assert_eq!(facts.meta.collectors.len(), 1);
    assert_eq!(facts.meta.collectors[0].collector, "repo-change");
}

#[tokio::test]
async fn positive_remote_url_is_hashed_never_raw() {
    let dir = fixture();
    let url = "https://github.com/randomizedcoder/agent-seddon.git";
    git(&dir, &["remote", "add", "origin", url]);

    let facts = orchestrator(&dir)
        .collect(&ReviewTarget::Branch("feature".into()))
        .await
        .expect("collect");

    assert_eq!(facts.git_state.host, ForgeHost::GitHub);
    assert_eq!(facts.git_state.relationship, RepoRelation::Clone);
    assert!(!facts.git_state.remote_url_hash.is_empty());

    // The raw URL must never appear in the facts or its rendering.
    let rendered = agent_review::render_facts(&facts);
    assert!(
        !rendered.contains("github.com/randomizedcoder"),
        "{rendered}"
    );
    assert!(
        !format!("{facts:?}").contains("randomizedcoder"),
        "raw url leaked into facts"
    );
}

#[tokio::test]
async fn positive_fork_detected_from_upstream_ref() {
    let dir = fixture();
    git(
        &dir,
        &["remote", "add", "origin", "https://github.com/me/fork.git"],
    );
    // Simulate a fetched `upstream` remote-tracking ref.
    git(&dir, &["update-ref", "refs/remotes/upstream/main", "main"]);

    let facts = orchestrator(&dir)
        .collect(&ReviewTarget::Branch("feature".into()))
        .await
        .expect("collect");
    assert_eq!(facts.git_state.relationship, RepoRelation::Fork);
}

#[tokio::test]
async fn positive_language_detected_from_go_mod() {
    let dir = fixture();
    std::fs::write(dir.join("go.mod"), "module x\n\ngo 1.22\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "add go.mod"]);

    let facts = orchestrator(&dir)
        .collect(&ReviewTarget::Branch("feature".into()))
        .await
        .expect("collect");
    assert_eq!(facts.git_state.project, RepoLanguage::Go);
}

/// Adversarial: a branch name with a traversal/ref-injection segment is rejected
/// before it ever reaches git.
#[tokio::test]
async fn adversarial_unsafe_branch_name_is_rejected() {
    let dir = fixture();
    let err = orchestrator(&dir)
        .collect(&ReviewTarget::Branch("../../heads/main".into()))
        .await;
    assert!(err.is_err(), "unsafe ref must be rejected, not resolved");
}

/// The classifier + orchestrator compose: a review prompt classifies as Review,
/// and the orchestrator produces a grounded, hallucination-free bundle.
#[tokio::test]
async fn positive_classify_then_collect_end_to_end() {
    let dir = fixture();
    let classifier = HybridClassifier::new(None);
    let verdict = classifier
        .classify(&agent_core::ClassifyCtx {
            prompt: "please review this branch",
            history: &[],
        })
        .await;
    assert_eq!(verdict.mode, agent_core::TaskMode::Review);

    let facts = orchestrator(&dir)
        .collect(&ReviewTarget::Branch("feature".into()))
        .await
        .expect("collect");
    let rendered = agent_review::render_facts(&facts);
    assert!(rendered.contains("Grounded review facts"), "{rendered}");
    assert!(rendered.contains("changed file"), "{rendered}");
}
