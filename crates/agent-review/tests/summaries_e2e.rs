//! Integration tests for the cheap-LLM summaries collector against a real git repo,
//! driving the pool through an in-process `FakePool` — so the happy path (fan-out +
//! render) is proven **offline**, with no network and no real model. The fail-soft
//! paths (no pool, dead pool) are asserted too. Requires `git` on PATH (dev shell).

use agent_core::{
    CollectStatus, CompletionRequest, CompletionResponse, HealthReport, LlmPool, Message,
    PoolMemberHealth, PoolMemberResult, PoolTier, Result, ReviewCollector, ReviewTarget,
};
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

/// A 2-commit Go history on `main`: `Handle`'s body is modified and `Compute` is
/// added between HEAD~1 and HEAD.
fn fixture() -> PathBuf {
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(
        dir.join("app.go"),
        "package app\n\nfunc Handle() int {\n\treturn 1\n}\n",
    )
    .unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    std::fs::write(
        dir.join("app.go"),
        "package app\n\nfunc Handle() int {\n\treturn Compute()\n}\n\nfunc Compute() int {\n\treturn 2\n}\n",
    )
    .unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "head"]);
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

fn canned(text: &str) -> CompletionResponse {
    CompletionResponse {
        message: Message::assistant(text),
        finish_reason: "stop".into(),
        usage: None,
    }
}

/// A healthy pool that answers every request with a canned summary.
struct FakePool {
    alive: bool,
}

#[async_trait::async_trait]
impl LlmPool for FakePool {
    fn name(&self) -> &str {
        "fake"
    }
    async fn health(&self) -> HealthReport {
        HealthReport {
            members: vec![PoolMemberHealth {
                name: "m".into(),
                tier: PoolTier::Medium,
                alive: self.alive,
                consecutive_failures: 0,
                last_probe_ms: 1,
            }],
        }
    }
    async fn complete_all(
        &self,
        _req: CompletionRequest,
        _tier: PoolTier,
        _fanout: usize,
    ) -> Vec<PoolMemberResult> {
        vec![PoolMemberResult {
            member: "m".into(),
            duration_ms: 1,
            response: Some(canned("It summarizes the change.")),
            error: None,
        }]
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse> {
        Ok(canned("It summarizes the change."))
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
async fn positive_summaries_produced_and_rendered_soft() {
    let dir = fixture();
    let orch = ReviewOrchestrator::new(&dir, backend(&dir), None, None)
        .with_summaries(Some(Arc::new(FakePool { alive: true })));
    let facts = orch.collect(&revs()).await.expect("collect");

    assert!(
        facts.summaries.produced >= 1,
        "at least one summary produced"
    );
    assert!(
        facts
            .summaries
            .summaries
            .iter()
            .any(|s| s.name == "Handle" || s.name == "Compute"),
        "a changed function was summarized"
    );
    // The `Compute` function is newly added.
    assert!(facts
        .summaries
        .summaries
        .iter()
        .any(|s| s.name == "Compute" && s.kind == "added"));

    let text = render_facts(&facts);
    assert!(
        text.contains("Summaries (soft"),
        "the section is explicitly labelled soft"
    );
}

#[tokio::test]
async fn negative_no_pool_skips_fail_soft() {
    let dir = fixture();
    let orch = ReviewOrchestrator::new(&dir, backend(&dir), None, None).with_summaries(None);
    let facts = orch.collect(&revs()).await.expect("collect");

    assert!(facts.summaries.summaries.is_empty());
    assert_eq!(
        collector_status(&facts, "summaries"),
        Some(CollectStatus::Skipped)
    );
    // The hard facts are unaffected — the change set is still collected.
    assert!(!facts.change.files.is_empty());
}

#[tokio::test]
async fn negative_dead_pool_skips_without_dispatch() {
    let dir = fixture();
    let orch = ReviewOrchestrator::new(&dir, backend(&dir), None, None)
        .with_summaries(Some(Arc::new(FakePool { alive: false })));
    let facts = orch.collect(&revs()).await.expect("collect");

    assert!(facts.summaries.summaries.is_empty());
    assert_eq!(
        collector_status(&facts, "summaries"),
        Some(CollectStatus::Skipped)
    );
}
