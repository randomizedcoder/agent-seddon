//! The campaign track's first autonomous pull request, end to end (CP-06b,
//! `docs/design/campaigns/05-increments.md`): the **real** wiring — `build_agent_with`
//! (registry → builder → metered seams → loop), the shipped [`Driver`] with the
//! shipped `FactoryPlanner` and `ForgePoller`, the in-process worker
//! (`InProcessExec` → `run_leaf`), a real `git` checkout with a bare origin on
//! disk — against a scripted model and a trait-level fake forge.
//!
//! Flow: `create` → tick (the planner splits the root) → tick (the planner
//! executes the child, the driver claims it and dispatches the worker: worktree,
//! Implement session that writes `hello.txt`, checkpoint, push to the bare
//! origin, `create_pr`) → `drain` → `in_review` with the PR → `approve` → the fake
//! reports `merged` → tick (the poller) → `done`, root `done`.
//!
//! The model is `agent_testkit::ScriptedProvider` (registered as `"scripted"`),
//! the forge a private [`FakeForge`] (registered as `"fake"`), the repo backend a
//! real `agent_git::CliBackend` rooted on the tempdir (registered as `"e2e"`, so
//! the process cwd never leaks into the fixture). Nothing else is a double.
//! `nix/checks/campaign-e2e.nix` runs exactly this file so the gate names the
//! increment.

use agent_campaign::{
    BriefSource, Driver, FactoryPlanner, FallbackBrief, Planner, PlannerFactory, Tenants,
    TouchResolver, WorktreeTouches,
};
use agent_core::campaign::{CampaignBackend, CampaignStore, Task, TaskId, TaskState};
use agent_core::{
    CompletionResponse, CreatePrRequest, Forge, Issue, LlmProvider, Page, PullRequest, RepoBackend,
    ReviewVerdict, ToolCall,
};
use agent_runtime::campaign_driver::{build_driver, WorkerDeps};
use agent_runtime::campaign_worker::{branch_name, worktree_id, WorkerCfg};
use agent_runtime::{
    build_agent_with, parse_config, register_builtins, CampaignCfg, Metrics, Registry,
};
use agent_testkit::campaign::conformance::{campaign, dave};
use agent_testkit::campaign::MemCampaigns;
use agent_testkit::{final_turn, tempdir, tool_turn, ScriptedProvider};
use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TENANT: &str = "acme";
const PR_NUMBER: u64 = 7;
const PR_URL: &str = "https://forge.test/org/repo/pull/7";
const HELLO: &str = "hello from the campaign\n";

// ---------------------------------------------------------------------------
// git fixture
// ---------------------------------------------------------------------------

/// Run `git -C dir <args>` for fixture setup, panicking on failure.
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

/// `git -C dir <args>` stdout on success, `None` on a non-zero exit.
fn git_out(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("spawn git");
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A working checkout on `main` (one commit, `a.txt`) with a bare `origin.git`
/// beside it that already holds `main`. The checkout carries its own identity
/// and `commit.gpgsign = false` so the worker's checkpoint commit needs no
/// `$HOME` git config (the nix sandbox has none).
fn repo_fixture(dir: &Path) -> (PathBuf, PathBuf) {
    let work = dir.join("work");
    let origin = dir.join("origin.git");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@e"]);
    git(&work, &["config", "commit.gpgsign", "false"]);
    std::fs::write(work.join("a.txt"), "hello\n").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "init"]);
    git(dir, &["init", "-q", "--bare", "origin.git"]);
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&work, &["push", "-q", "origin", "main"]);
    (work, origin)
}

// ---------------------------------------------------------------------------
// The fake forge
// ---------------------------------------------------------------------------

/// A forge that records `create_pr` and answers `get_pr` with a state the test
/// flips (`open` until told `merged`).
#[derive(Default)]
struct FakeForge {
    created: Mutex<Vec<CreatePrRequest>>,
    state: Mutex<String>,
}

impl FakeForge {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            created: Mutex::new(Vec::new()),
            state: Mutex::new("open".into()),
        })
    }

    fn set_state(&self, state: &str) {
        *self.state.lock().unwrap() = state.into();
    }

    fn created(&self) -> Vec<CreatePrRequest> {
        self.created.lock().unwrap().clone()
    }

    fn pr(&self, req: &CreatePrRequest) -> PullRequest {
        PullRequest {
            number: PR_NUMBER,
            title: req.title.clone(),
            body: req.body.clone(),
            state: self.state.lock().unwrap().clone(),
            author: "campaign-bot".into(),
            url: PR_URL.into(),
            source_branch: req.source_branch.clone(),
            target_branch: req.target_branch.clone(),
            draft: req.draft,
        }
    }
}

#[async_trait]
impl Forge for FakeForge {
    fn name(&self) -> &str {
        "fake"
    }
    async fn get_pr(&self, number: u64) -> agent_core::Result<PullRequest> {
        let created = self.created.lock().unwrap();
        match created.last() {
            Some(req) if number == PR_NUMBER => Ok(self.pr(req)),
            _ => Err(agent_core::Error::Web(format!("no pull request #{number}"))),
        }
    }
    async fn list_prs(&self, _page: u32) -> agent_core::Result<Page<PullRequest>> {
        Ok(Page {
            items: vec![],
            next_page: None,
        })
    }
    async fn list_issues(&self, _page: u32) -> agent_core::Result<Page<Issue>> {
        Ok(Page {
            items: vec![],
            next_page: None,
        })
    }
    async fn import_issue(&self, number: u64) -> agent_core::Result<Issue> {
        Err(agent_core::Error::Web(format!("no issue #{number}")))
    }
    async fn create_pr(&self, req: &CreatePrRequest) -> agent_core::Result<PullRequest> {
        self.created.lock().unwrap().push(req.clone());
        Ok(self.pr(req))
    }
    async fn comment(&self, _number: u64, _body: &str) -> agent_core::Result<agent_core::Comment> {
        Err(agent_core::Error::Web(
            "comments are not part of this fixture".into(),
        ))
    }
    async fn review_pr(
        &self,
        _number: u64,
        _verdict: ReviewVerdict,
        _body: &str,
    ) -> agent_core::Result<agent_core::Comment> {
        Err(agent_core::Error::Web(
            "reviews are not part of this fixture".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// The scripted model
// ---------------------------------------------------------------------------

fn call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    }
}

/// The planner's first answer: split the root into one child.
fn split_turn() -> CompletionResponse {
    final_turn(
        json!({
            "decision": "split",
            "reason": "one greeting file, one pull request",
            "confidence": 0.9,
            "children": [
                { "title": "Add greeting", "goal": "create hello.txt with a greeting", "est_size": "s" }
            ],
        })
        .to_string(),
    )
}

/// The planner's second answer: the child is one pull request's worth.
fn execute_turn() -> CompletionResponse {
    final_turn(
        json!({
            "decision": "execute",
            "reason": "fits one pull request",
            "confidence": 0.9,
            "acceptance": ["hello.txt exists and greets"],
            "touches": ["a.txt"],
            "est_size": "s",
        })
        .to_string(),
    )
}

/// The worker's turns: write `hello.txt` in the worktree, then finish.
fn worker_writes_hello() -> Vec<CompletionResponse> {
    vec![
        tool_turn(vec![call(
            "1",
            "write_file",
            json!({"path": "hello.txt", "content": HELLO}),
        )]),
        final_turn("Added hello.txt."),
    ]
}

/// A worker that edits nothing.
fn worker_idle() -> Vec<CompletionResponse> {
    vec![final_turn("Nothing to do.")]
}

fn script(worker: Vec<CompletionResponse>) -> Vec<CompletionResponse> {
    let mut s = vec![split_turn(), execute_turn()];
    s.extend(worker);
    s
}

// ---------------------------------------------------------------------------
// The harness: real agent, real driver, mem store, tempdir git
// ---------------------------------------------------------------------------

/// A hermetic config: the scripted provider, `auto-approve` policy, every on-disk
/// seam under `dir`, the `"e2e"` repo backend (a real `CliBackend` rooted on the
/// fixture checkout, see [`E2e::new`]), the `"fake"` forge, and the in-process
/// campaign sandbox.
fn config_toml(dir: &Path, work: &Path, push_policy: &str) -> String {
    let d = dir.display();
    let w = work.display();
    format!(
        r#"
        [agent]
        provider = "scripted"
        policy = "auto-approve"
        stream = false
        working_dir = "{w}"
        max_iterations = 6

        [provider]
        model = "scripted-model"

        [memory]
        episodic_path = "{d}/.agent/episodic.jsonl"
        semantic_dir = "{d}/.agent/memory"

        [search]
        index_dir = "{d}/.agent/index"
        auto_index = false

        [git]
        backend = "e2e"
        mirror_dir = "{d}/.agent/mirror"
        worktrees_dir = "{d}/.agent/worktrees"
        auto_fetch_secs = 0
        push_policy = "{push_policy}"

        [forge]
        backend = "fake"
        dry_run = false

        [campaign]
        sandbox = "in_process"
        plan_per_tick = 4
        worker_timeout_secs = 60
        target_branch = "main"

        [tokenizer]
        backend = "approx"
    "#
    )
}

struct E2e {
    origin: PathBuf,
    worktrees: PathBuf,
    store: Arc<dyn CampaignStore>,
    driver: Driver,
    forge: Arc<FakeForge>,
    provider: Arc<ScriptedProvider>,
    root: Task,
}

impl E2e {
    /// Build the whole stack over `script`, with `[git] push_policy = push_policy`,
    /// and create one campaign.
    async fn new(script: Vec<CompletionResponse>, push_policy: &str) -> Self {
        let dir = tempdir();
        let (work, origin) = repo_fixture(&dir);
        let cfg = parse_config(&config_toml(&dir, &work, push_policy)).expect("parse config");
        let worker = WorkerCfg::from_config(&cfg);
        let campaign_cfg = CampaignCfg {
            enabled: true,
            ..cfg.campaign.clone()
        };
        let worktrees = PathBuf::from(&cfg.git.worktrees_dir);

        let provider = Arc::new(ScriptedProvider::new(script));
        let forge = FakeForge::new();
        let mut registry = Registry::new();
        register_builtins(&mut registry);
        let p = provider.clone();
        registry.provider(
            "scripted",
            move |_ctx| Ok(p.clone() as Arc<dyn LlmProvider>),
        );
        let f = forge.clone();
        registry.forge("fake", move |_ctx| Ok(f.clone() as Arc<dyn Forge>));
        let (root, mirror, wts) = (work.clone(), dir.join(".agent/mirror"), worktrees.clone());
        registry.repo("e2e", move |_ctx| {
            Ok(Arc::new(agent_git::CliBackend::new(
                root.clone(),
                mirror.clone(),
                wts.clone(),
                "",
            )) as Arc<dyn RepoBackend>)
        });
        let agent = build_agent_with(&registry, cfg, None, "e2e-session".into(), Metrics::new())
            .await
            .expect("build agent");

        let mem = MemCampaigns::new();
        let store: Arc<dyn CampaignStore> = Arc::new(mem.with_tenant(TENANT).expect("tenant"));
        let backend: Arc<dyn CampaignBackend> = Arc::new(mem);

        let planner_provider = agent.campaign_planner_provider();
        let brief: Arc<dyn BriefSource> = Arc::new(FallbackBrief::new(&work));
        let touches: Arc<dyn TouchResolver> = Arc::new(WorktreeTouches::new(&work));
        let factory: PlannerFactory = Arc::new(move |store| {
            Planner::draft07(
                store,
                planner_provider.clone(),
                brief.clone(),
                touches.clone(),
                "scripted-model",
            )
        });
        let driver = build_driver(
            &campaign_cfg,
            backend,
            Tenants::Fixed(vec![TENANT.into()]),
            Arc::new(FactoryPlanner(factory)),
            agent.forge(),
            WorkerDeps {
                agent: agent.clone(),
                agent_bin: None,
                config_path: None,
                worker,
            },
        )
        .expect("build driver");

        let root = campaign(store.as_ref(), "Greeting").await;
        Self {
            origin,
            worktrees,
            store,
            driver,
            forge,
            provider,
            root,
        }
    }

    /// Two ticks (split, then execute + claim + dispatch) and a drain; the leaf.
    async fn plan_claim_and_work(&self) -> Task {
        let t1 = self.driver.tick().await;
        assert_eq!(t1.errors, 0, "tick 1: {t1:?}");
        let kids = self.store.children(self.root.task_id).await.unwrap();
        assert_eq!(kids.len(), 1, "the split made one child: {kids:?}");
        assert!(t1.dispatched.is_empty(), "nothing claimable yet: {t1:?}");

        let t2 = self.driver.tick().await;
        assert_eq!(t2.errors, 0, "tick 2: {t2:?}");
        assert_eq!(
            t2.dispatched,
            vec![(TENANT.to_string(), kids[0].task_id)],
            "tick 2 dispatched the leaf: {t2:?}"
        );
        let drained = self.driver.drain(Duration::from_secs(120)).await;
        assert_eq!(drained.aborted, 0, "the worker settled: {drained:?}");
        assert_eq!(drained.settled.len(), 1, "{drained:?}");
        self.store.get(kids[0].task_id).await.unwrap()
    }

    async fn state(&self, id: TaskId) -> TaskState {
        self.store.get(id).await.unwrap().state
    }

    async fn last_error(&self, id: TaskId) -> String {
        self.store
            .attempts(id)
            .await
            .unwrap()
            .last()
            .and_then(|a| a.error.clone())
            .unwrap_or_default()
    }

    /// `hello.txt` at `branch` on the bare origin, `None` when the branch is absent.
    fn origin_hello(&self, branch: &str) -> Option<String> {
        git_out(
            &self.origin,
            &[
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{branch}"),
            ],
        )?;
        git_out(&self.origin, &["show", &format!("{branch}:hello.txt")])
    }

    fn origin_branches(&self) -> Vec<String> {
        git_out(
            &self.origin,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
        )
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
    }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

// positive: add → plan → claim → worker → push → create_pr → approve → poll → done.
#[tokio::test]
async fn positive_first_autonomous_pr() {
    let e = E2e::new(script(worker_writes_hello()), "branch").await;
    let leaf = e.plan_claim_and_work().await;
    let branch = branch_name(leaf.campaign_id, &leaf.path);

    // The leaf is in review with the fake's PR, on the branch the worker pushed.
    assert_eq!(leaf.state, TaskState::InReview, "{leaf:?}");
    assert_eq!(leaf.pr_number, Some(PR_NUMBER as i64));
    assert_eq!(leaf.pr_url.as_deref(), Some(PR_URL));
    assert_eq!(leaf.branch.as_deref(), Some(branch.as_str()));

    // The bare origin holds the branch with the session's file.
    assert_eq!(e.origin_hello(&branch).as_deref(), Some(HELLO));
    // The worktree was removed after the push.
    assert!(
        !e.worktrees
            .join(worktree_id(leaf.campaign_id, &leaf.path))
            .exists(),
        "worktree removed"
    );

    // One PR request: a draft (policy default) onto `target_branch`, titled from
    // the campaign / path / leaf, with the trailer the poller-era tooling greps.
    let created = e.forge.created();
    assert_eq!(created.len(), 1, "{created:?}");
    let req = &created[0];
    assert!(req.draft, "policy.draft_prs defaults on: {req:?}");
    assert_eq!(req.source_branch, branch);
    assert_eq!(req.target_branch, "main");
    assert_eq!(
        req.title,
        format!("Greeting / {}: Add greeting", leaf.path.as_str())
    );
    assert!(
        req.body.contains("hello.txt exists and greets"),
        "{}",
        req.body
    );
    assert!(
        req.body.trim_end().ends_with(&format!(
            "campaign:{} task:{}",
            leaf.campaign_id.0,
            leaf.path.as_str()
        )),
        "{}",
        req.body
    );
    // Two planner calls + two worker turns: every scripted turn was consumed once.
    assert_eq!(e.provider.calls(), 4);

    // Approve, merge on the forge, poll: leaf and root complete.
    e.store
        .approve(leaf.task_id, leaf.version, &dave())
        .await
        .expect("approve records pr_approved");
    e.forge.set_state("merged");
    let t3 = e.driver.tick().await;
    assert_eq!(t3.errors, 0, "tick 3: {t3:?}");
    assert_eq!(t3.per_tenant.len(), 1, "{t3:?}");
    assert_eq!(t3.per_tenant[0].poll.merged, 1, "{t3:?}");
    assert_eq!(e.state(leaf.task_id).await, TaskState::Done);
    assert_eq!(e.state(e.root.task_id).await, TaskState::Done, "rollup");
}

// negative: a merged PR without a recorded approval waits (`awaiting_pr_approval`
// once), then completes once approved.
#[tokio::test]
async fn negative_unapproved_merge_waits() {
    let e = E2e::new(script(worker_writes_hello()), "branch").await;
    let leaf = e.plan_claim_and_work().await;
    assert_eq!(leaf.state, TaskState::InReview, "{leaf:?}");

    e.forge.set_state("merged");
    let t3 = e.driver.tick().await;
    assert_eq!(t3.per_tenant[0].poll.awaiting, 1, "{t3:?}");
    assert_eq!(t3.per_tenant[0].poll.merged, 0, "{t3:?}");
    assert_eq!(e.state(leaf.task_id).await, TaskState::InReview);
    let awaiting = |events: &[agent_core::campaign::TaskEvent]| {
        events
            .iter()
            .filter(|ev| ev.detail.get("awaiting_pr_approval") == Some(&json!(true)))
            .count()
    };
    let events = e.store.events(leaf.task_id).await.unwrap();
    assert_eq!(awaiting(&events), 1, "{events:?}");

    // A second poll adds no second marker; approval unblocks it.
    let t4 = e.driver.tick().await;
    assert_eq!(t4.per_tenant[0].poll.awaiting, 1, "{t4:?}");
    let events = e.store.events(leaf.task_id).await.unwrap();
    assert_eq!(awaiting(&events), 1, "idempotent: {events:?}");
    let leaf = e.store.get(leaf.task_id).await.unwrap();
    e.store
        .approve(leaf.task_id, leaf.version, &dave())
        .await
        .expect("approve");
    let t5 = e.driver.tick().await;
    assert_eq!(t5.per_tenant[0].poll.merged, 1, "{t5:?}");
    assert_eq!(e.state(leaf.task_id).await, TaskState::Done);
    assert_eq!(e.state(e.root.task_id).await, TaskState::Done);
}

// corner: a session that commits nothing fails the leaf; nothing is pushed and no
// PR is opened.
#[tokio::test]
async fn corner_no_changes_fails_leaf() {
    let e = E2e::new(script(worker_idle()), "branch").await;
    let leaf = e.plan_claim_and_work().await;
    assert_eq!(leaf.state, TaskState::Failed, "{leaf:?}");
    let err = e.last_error(leaf.task_id).await;
    assert!(err.contains("no changes committed"), "{err}");
    assert_eq!(leaf.pr_number, None);
    assert_eq!(e.origin_branches(), vec!["main".to_string()]);
    assert!(e.forge.created().is_empty());
    assert!(
        !e.worktrees
            .join(worktree_id(leaf.campaign_id, &leaf.path))
            .exists(),
        "worktree removed"
    );
}

// negative: `[git] push_policy = "never"` fails the leaf before any token is spent;
// nothing is pushed and no PR is opened.
#[tokio::test]
async fn negative_push_policy_never() {
    let e = E2e::new(script(worker_writes_hello()), "never").await;
    let leaf = e.plan_claim_and_work().await;
    assert_eq!(leaf.state, TaskState::Failed, "{leaf:?}");
    let err = e.last_error(leaf.task_id).await;
    assert!(err.contains("[git] push_policy = never"), "{err}");
    assert_eq!(e.origin_branches(), vec!["main".to_string()]);
    assert!(e.forge.created().is_empty());
    // Only the two planner calls happened: the binding check ran before the session.
    assert_eq!(e.provider.calls(), 2);
}
