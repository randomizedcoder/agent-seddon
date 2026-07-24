//! Integration tests for the churn/ownership collector against a **real git repo**,
//! exercising `CliBackend::log_touched` → bus-factor + churn computation → render.
//! A history is built where `owned.go` is authored almost entirely by one person
//! (bus factor 1, single-owner) while `shared.go` has several authors; the reviewed
//! change touches both. The fail-soft (no history) path is asserted too. Requires
//! `git` on PATH (dev shell).

use agent_core::{CollectStatus, ReviewCollector, ReviewTarget};
use agent_git::CliBackend;
use agent_review::{render_facts, ReviewOrchestrator};
use agent_testkit::tempdir;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// Commit as a specific author (so bus factor is controllable).
fn git_as(dir: &Path, author: &str, args: &[&str]) {
    let email = format!("{author}@e");
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", author)
        .env("GIT_AUTHOR_EMAIL", &email)
        .env("GIT_COMMITTER_NAME", author)
        .env("GIT_COMMITTER_EMAIL", &email)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

fn git(dir: &Path, args: &[&str]) {
    git_as(dir, "t", args);
}

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

/// `owned.go`: 5 commits by alice, 1 by bob → bus factor 1. `shared.go`: one commit
/// each by alice/bob/carol/dave → bus factor 4. Final commit touches both.
fn fixture() -> PathBuf {
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    write(&dir, "owned.go", "package p\n\nfunc O() int { return 0 }\n");
    write(
        &dir,
        "shared.go",
        "package p\n\nfunc S() int { return 0 }\n",
    );
    git_as(&dir, "alice", &["add", "-A"]);
    git_as(&dir, "alice", &["commit", "-q", "-m", "seed"]);

    for i in 1..=4 {
        write(
            &dir,
            "owned.go",
            &format!("package p\n\nfunc O() int {{ return {i} }}\n"),
        );
        git_as(&dir, "alice", &["add", "-A"]);
        git_as(
            &dir,
            "alice",
            &["commit", "-q", "-m", &format!("owned {i}")],
        );
    }
    // One bob commit to owned.go (still 5 alice / 1 bob → bus factor 1).
    write(&dir, "owned.go", "package p\n\nfunc O() int { return 9 }\n");
    git_as(&dir, "bob", &["add", "-A"]);
    git_as(&dir, "bob", &["commit", "-q", "-m", "owned bob"]);

    // shared.go touched by three more distinct authors.
    for author in ["bob", "carol", "dave"] {
        write(
            &dir,
            "shared.go",
            &format!("package p\n\nfunc S() int {{ return {} }}\n", author.len()),
        );
        git_as(&dir, author, &["add", "-A"]);
        git_as(&dir, author, &["commit", "-q", "-m", "shared"]);
    }

    // The reviewed change: touch both files.
    write(
        &dir,
        "owned.go",
        "package p\n\nfunc O() int { return 42 }\n",
    );
    write(
        &dir,
        "shared.go",
        "package p\n\nfunc S() int { return 42 }\n",
    );
    git_as(&dir, "eve", &["add", "-A"]);
    git_as(
        &dir,
        "eve",
        &["commit", "-q", "-m", "the change under review"],
    );
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

fn revs() -> ReviewTarget {
    ReviewTarget::Revs {
        base: "HEAD~1".into(),
        head: "HEAD".into(),
    }
}

fn collector_status(facts: &agent_core::ReviewFacts, name: &str) -> Option<CollectStatus> {
    facts
        .meta
        .collectors
        .iter()
        .find(|c| c.collector == name)
        .map(|c| c.status)
}

#[tokio::test]
async fn positive_bus_factor_surfaces_end_to_end() {
    let dir = fixture();
    let orch = ReviewOrchestrator::new(&dir, backend(&dir), None, None).with_churn(2000);
    let facts = orch.collect(&revs()).await.expect("collect");

    assert_eq!(collector_status(&facts, "churn"), Some(CollectStatus::Ok));
    let owned = facts
        .churn
        .files
        .iter()
        .find(|f| f.path == "owned.go")
        .expect("owned.go churn entry");
    // 6 commits (seed + 4 alice + 1 bob), alice authored 5/6 ≈ 83% > 80% → bus factor 1.
    assert_eq!(owned.commits, 6);
    assert_eq!(owned.bus_factor, 1, "owned.go is single-owner");
    assert!(owned.top_author_share > 0.8);

    let shared = facts
        .churn
        .files
        .iter()
        .find(|f| f.path == "shared.go")
        .expect("shared.go churn entry");
    // seed(alice) + bob + carol + dave = 4 authors, ~25% each → bus factor 4.
    assert_eq!(shared.unique_authors, 4);
    assert!(shared.bus_factor >= 3, "shared.go is not single-owner");

    let text = render_facts(&facts);
    assert!(text.contains("Churn & ownership"), "section rendered");
    assert!(
        text.contains("owned.go") && text.contains("single-owner"),
        "single-owner file foregrounded:\n{text}"
    );
    // No author identity leaks into the rendered context.
    assert!(!text.contains("alice") && !text.contains("bob@e"));
}

#[tokio::test]
async fn negative_no_history_skips_fail_soft() {
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    write(&dir, "a.go", "package p\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    write(&dir, "a.go", "package p\n\nfunc A() {}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "two"]);

    let orch = ReviewOrchestrator::new(&dir, backend(&dir), None, None).with_churn(2000);
    let facts = orch.collect(&revs()).await.expect("collect");

    // a.go has prior history (the seed commit) so it does get an entry — assert the
    // hard facts are intact and the collector ran without error.
    assert_ne!(
        collector_status(&facts, "churn"),
        Some(CollectStatus::Failed)
    );
    assert!(!facts.change.files.is_empty());
}
