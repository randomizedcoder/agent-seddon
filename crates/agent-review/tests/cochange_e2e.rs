//! Integration tests for the co-change collector against a **real git repo**,
//! exercising the whole path: `CliBackend::log_touched` (a real `git log
//! --numstat` parse) → the collector's mining → the rendered section. A history
//! is built where `handler.go` and `schema.go` habitually change together; the
//! reviewed change then touches only `handler.go`, so `schema.go` must surface as
//! an absent partner. The fail-soft path (a shallow/empty history) is asserted too.
//! Requires `git` on PATH (dev shell).

use agent_core::{CollectStatus, ReviewCollector, ReviewTarget};
use agent_git::CliBackend;
use agent_review::{render_facts, ReviewOrchestrator};
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

fn write(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).unwrap();
}

/// A history where `handler.go` and `schema.go` co-change 4× (a strong coupling),
/// `util.go` moves independently, then the final reviewed commit touches only
/// `handler.go` — leaving `schema.go` behind.
fn fixture() -> PathBuf {
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    // Seed.
    write(
        &dir,
        "handler.go",
        "package p\n\nfunc H() int { return 0 }\n",
    );
    write(&dir, "schema.go", "package p\n\ntype S struct{ V int }\n");
    write(&dir, "util.go", "package p\n\nfunc U() {}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "seed"]);
    // Four commits that touch handler.go AND schema.go together.
    for i in 1..=4 {
        write(
            &dir,
            "handler.go",
            &format!("package p\n\nfunc H() int {{ return {i} }}\n"),
        );
        write(
            &dir,
            "schema.go",
            &format!("package p\n\ntype S struct{{ V int; N{i} int }}\n"),
        );
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", &format!("pair {i}")]);
    }
    // A couple of independent util.go commits (noise).
    for i in 1..=2 {
        write(
            &dir,
            "util.go",
            &format!("package p\n\nfunc U() {{ _ = {i} }}\n"),
        );
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", &format!("util {i}")]);
    }
    // The reviewed change: touch ONLY handler.go (schema.go left behind).
    write(
        &dir,
        "handler.go",
        "package p\n\nfunc H() int { return 42 }\n",
    );
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "the change under review"]);
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
async fn positive_absent_partner_surfaces_end_to_end() {
    let dir = fixture();
    let orch = ReviewOrchestrator::new(&dir, backend(&dir), None, None).with_cochange(2000);
    let facts = orch.collect(&revs()).await.expect("collect");

    assert_eq!(
        collector_status(&facts, "cochange"),
        Some(CollectStatus::Ok)
    );
    // handler.go is the only changed file; schema.go is its strong partner and is
    // NOT in this diff → surfaced as missing.
    let entry = facts
        .cochange
        .entries
        .iter()
        .find(|e| e.path == "handler.go")
        .expect("handler.go entry");
    let schema = entry
        .partners
        .iter()
        .find(|p| p.path == "schema.go")
        .expect("schema.go partner");
    assert!(!schema.in_diff, "schema.go is absent from the diff");
    assert!(schema.co_occurrences >= 4, "co-occurred at least 4×");
    assert!(facts.cochange.missing_partners >= 1);
    // util.go coupling is weak/independent → not a partner of handler.go.
    assert!(
        !entry.partners.iter().any(|p| p.path == "util.go"),
        "util.go should not be a handler.go partner"
    );

    let text = render_facts(&facts);
    assert!(text.contains("Historical co-change"), "section rendered");
    assert!(
        text.contains("schema.go") && text.contains("NOT in this diff"),
        "the absent partner is foregrounded:\n{text}"
    );
}

#[tokio::test]
async fn negative_no_history_skips_fail_soft() {
    // A repo with a single commit: `HEAD~1..HEAD` still has a change, but there is
    // no prior history to mine → the collector skips fail-soft, facts intact.
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    write(&dir, "a.go", "package p\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    write(&dir, "a.go", "package p\n\nfunc A() {}\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "two"]);

    let orch = ReviewOrchestrator::new(&dir, backend(&dir), None, None).with_cochange(2000);
    let facts = orch.collect(&revs()).await.expect("collect");

    // Two commits of history exist, but no file pair clears the thresholds → the
    // report has no entries (Partial), and the hard facts are unaffected.
    assert!(facts.cochange.entries.is_empty());
    assert!(!facts.change.files.is_empty());
}
