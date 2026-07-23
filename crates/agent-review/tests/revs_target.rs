//! Hermetic test for `ReviewTarget::Revs { base, head }` — feeding explicit commit
//! ids. Builds its own temp git repo (no real history needed), so it runs in the
//! gate. Mirrors `repo_facts_e2e.rs`.

use agent_core::{ReviewCollector, ReviewTarget};
use agent_git::CliBackend;
use agent_review::ReviewOrchestrator;
use agent_testkit::tempdir;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `main` (a.txt) + `feature` (modifies a.txt, adds b.txt). Returns (dir, base_oid, head_oid).
fn fixture() -> (PathBuf, String, String) {
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = git(&dir, &["rev-parse", "HEAD"]);

    git(&dir, &["switch", "-q", "-c", "feature"]);
    std::fs::write(dir.join("a.txt"), "hello\nmore\n").unwrap();
    std::fs::write(dir.join("b.txt"), "world\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "feature work"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    git(&dir, &["switch", "-q", "main"]);
    (dir, base, head)
}

fn orchestrator(root: &Path) -> ReviewOrchestrator {
    let repo = Arc::new(CliBackend::new(
        root,
        root.join(".agent-seddon/mirror"),
        root.join(".agent-seddon/worktrees"),
        "",
    ));
    ReviewOrchestrator::new(root, repo, None, None)
}

/// Explicit commit ids produce the same change set as the equivalent branch target.
#[tokio::test]
async fn positive_revs_matches_branch() {
    let (dir, base, head) = fixture();
    let by_revs = orchestrator(&dir)
        .collect(&ReviewTarget::Revs {
            base: base.clone(),
            head: head.clone(),
        })
        .await
        .expect("revs collect");
    let by_branch = orchestrator(&dir)
        .collect(&ReviewTarget::Branch("feature".into()))
        .await
        .expect("branch collect");

    let names = |f: &agent_core::ReviewFacts| {
        let mut v: Vec<String> = f
            .change
            .files
            .iter()
            .map(|c| c.path.display().to_string())
            .collect();
        v.sort();
        v
    };
    assert_eq!(by_revs.change.files.len(), 2);
    assert_eq!(names(&by_revs), names(&by_branch));
    assert_eq!(by_revs.change.base_rev, base);
    assert_eq!(by_revs.change.head_rev, head);
}

/// Adversarial: an empty, option-injecting, or space/shell-bearing revision is
/// rejected fail-closed before git resolves it — never a shell/option escape.
#[rstest::rstest]
#[case::empty_base("", "HEAD")]
#[case::leading_dash("--upload-pack=evil", "HEAD")]
#[case::space("a b", "HEAD")]
#[case::shell_meta("HEAD;rm -rf /", "HEAD")]
#[case::empty_head("HEAD", "")]
#[tokio::test]
async fn adversarial_unsafe_revs_are_rejected(#[case] base: &str, #[case] head: &str) {
    let (dir, _b, _h) = fixture();
    let err = orchestrator(&dir)
        .collect(&ReviewTarget::Revs {
            base: base.into(),
            head: head.into(),
        })
        .await;
    assert!(
        err.is_err(),
        "unsafe revs `{base}..{head}` must be rejected"
    );
}

/// A well-formed but non-existent commit id fails **soft**: the diff can't be
/// computed, so the collector reports a non-`ok` status with an empty change set —
/// the bundle is still assembled (the fail-soft contract), never a silent success
/// pretending there were no changes.
#[tokio::test]
async fn boundary_nonexistent_rev_is_partial_not_silent() {
    let (dir, _b, _h) = fixture();
    let facts = orchestrator(&dir)
        .collect(&ReviewTarget::Revs {
            base: "0000000000000000000000000000000000000000".into(),
            head: "1111111111111111111111111111111111111111".into(),
        })
        .await
        .expect("fail-soft: a bundle is still assembled");
    assert!(facts.change.files.is_empty(), "no diff was computable");
    let status = &facts.meta.collectors[0].status;
    assert_ne!(
        *status,
        agent_core::CollectStatus::Ok,
        "an unresolvable range must not report `ok`"
    );
}
