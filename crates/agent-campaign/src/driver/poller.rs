//! The forge PR poller (`docs/design/campaigns/04-executor.md` "PR poller", CP-06):
//! the [`PrPoller`] the shipped driver runs when a `[forge]` backend is configured.
//! Per tenant per tick it takes up to `poll_batch` `in_review` leaves (oldest
//! first), asks the forge for each leaf's PR and moves the leaf on:
//!
//! | forge says | `require_pr_approval` | `pr_approved` event | result |
//! |---|---|---|---|
//! | `merged` | `false` | — | `resolve_review(Merged)` → `done` |
//! | `merged` | `true` | present | `resolve_review(Merged)` → `done` |
//! | `merged` | `true` | absent | `review_note(AwaitingApproval)` (once), leaf stays |
//! | `closed` | — | — | `resolve_review(Closed)` → `failed`, dependents `blocked` |
//! | `open` | — | — | nothing |
//! | anything else | — | — | nothing written; counted as an error |
//!
//! The **approval gate lives here**, not in the store: `resolve_review` is the
//! poller's verb and the stores do not read policy for it, so the check that the
//! campaign's `require_pr_approval` has been satisfied by a human `approve` (the
//! `pr_approved` marker) is this module's one rule.
//!
//! Every forge value is untrusted: the returned PR number must equal the row's
//! (a forge that answers with another PR is a `PollError`, never a transition),
//! the `state` string is matched exactly and anything unknown moves nothing, and a
//! call is bounded by [`POLL_PR_TIMEOUT_SECS`] so one hung request cannot stall
//! the tick. A forge error, a timeout or a mismatch is recorded on the leaf as a
//! `poll_error` event (bounded text) and the batch continues.

use agent_core::campaign::{CampaignStore, Policy, ReviewNote, ReviewOutcome, Task, TaskId};
use agent_core::Forge;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::{PollReport, PrPoller};

/// Wall clock per `get_pr`; past it the leaf gets a `poll_error` and the batch
/// moves on.
pub const POLL_PR_TIMEOUT_SECS: u64 = 30;

/// How much of an unknown `state` string the warning shows (escaped).
const STATE_LOG_CHARS: usize = 40;

/// The shipped [`PrPoller`]: resolves `in_review` leaves against a [`Forge`].
pub struct ForgePoller {
    forge: Arc<dyn Forge>,
}

impl ForgePoller {
    pub fn new(forge: Arc<dyn Forge>) -> Self {
        Self { forge }
    }

    /// One leaf: the forge answer folded into the store. Returns what happened
    /// so the batch loop can count it.
    async fn poll_leaf(&self, store: &dyn CampaignStore, leaf: &Task, policy: &Policy) -> Step {
        let Some(number) = leaf.pr_number.filter(|n| *n >= 1) else {
            // `complete` validates the PR, so this is a store row nothing wrote.
            tracing::warn!(task = %leaf.task_id, "campaign.poll: in_review leaf has no pr_number");
            return Step::Error;
        };
        let wanted = number.unsigned_abs();
        let fetched = tokio::time::timeout(
            Duration::from_secs(POLL_PR_TIMEOUT_SECS),
            self.forge.get_pr(wanted),
        )
        .await;
        let pr = match fetched {
            Ok(Ok(pr)) => pr,
            Ok(Err(e)) => {
                return self
                    .note_error(store, leaf.task_id, format!("forge get_pr {wanted}: {e}"))
                    .await;
            }
            Err(_) => {
                return self
                    .note_error(
                        store,
                        leaf.task_id,
                        format!("forge get_pr {wanted}: timed out after {POLL_PR_TIMEOUT_SECS}s"),
                    )
                    .await;
            }
        };
        if pr.number != wanted {
            return self
                .note_error(
                    store,
                    leaf.task_id,
                    format!("forge returned pr {} for {wanted}", pr.number),
                )
                .await;
        }
        match pr.state.as_str() {
            "open" => Step::Open,
            "closed" => {
                self.resolve(store, leaf.task_id, ReviewOutcome::Closed)
                    .await
            }
            "merged" => {
                let approved = !policy.require_pr_approval
                    || match has_pr_approval(store, leaf.task_id).await {
                        Ok(a) => a,
                        Err(e) => {
                            tracing::warn!(task = %leaf.task_id, error = %e, "campaign.poll: events failed");
                            return Step::Error;
                        }
                    };
                if approved {
                    self.resolve(store, leaf.task_id, ReviewOutcome::Merged)
                        .await
                } else {
                    match store
                        .review_note(leaf.task_id, ReviewNote::AwaitingApproval)
                        .await
                    {
                        Ok(_) => Step::Awaiting,
                        Err(e) => {
                            tracing::warn!(task = %leaf.task_id, error = %e, "campaign.poll: review_note failed");
                            Step::Error
                        }
                    }
                }
            }
            other => {
                let shown: String = other.chars().take(STATE_LOG_CHARS).collect();
                tracing::warn!(
                    task = %leaf.task_id,
                    state = ?shown,
                    "campaign.poll: forge returned an unknown PR state; leaf untouched"
                );
                Step::Error
            }
        }
    }

    async fn resolve(
        &self,
        store: &dyn CampaignStore,
        task: TaskId,
        outcome: ReviewOutcome,
    ) -> Step {
        match store.resolve_review(task, outcome).await {
            Ok(_) => match outcome {
                ReviewOutcome::Merged => Step::Merged,
                ReviewOutcome::Closed => Step::Closed,
            },
            Err(e) => {
                tracing::warn!(task = %task, error = %e, "campaign.poll: resolve_review failed");
                Step::Error
            }
        }
    }

    /// Record `text` on the leaf (bounded by the store) and count an error; a
    /// store failure while recording is logged and still counted once.
    async fn note_error(&self, store: &dyn CampaignStore, task: TaskId, text: String) -> Step {
        tracing::warn!(task = %task, error = %text, "campaign.poll: forge lookup failed");
        if let Err(e) = store.review_note(task, ReviewNote::PollError(text)).await {
            tracing::warn!(task = %task, error = %e, "campaign.poll: review_note failed");
        }
        Step::Error
    }
}

/// Whether a human `approve` has marked the leaf (`detail.pr_approved = true`).
async fn has_pr_approval(
    store: &dyn CampaignStore,
    task: TaskId,
) -> agent_core::campaign::CampaignResult<bool> {
    Ok(store
        .events(task)
        .await?
        .iter()
        .any(|e| e.detail["pr_approved"] == serde_json::Value::Bool(true)))
}

/// What one leaf's poll did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Open,
    Merged,
    Closed,
    Awaiting,
    Error,
}

#[async_trait]
impl PrPoller for ForgePoller {
    async fn poll(&self, store: Arc<dyn CampaignStore>, batch: usize) -> PollReport {
        let mut report = PollReport::default();
        let leaves = match store.in_review(batch).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "campaign.poll: in_review failed");
                report.errors += 1;
                return report;
            }
        };
        // The root policy per campaign, read once per batch.
        let mut policies: HashMap<TaskId, Policy> = HashMap::new();
        for leaf in &leaves {
            report.polled += 1;
            let policy = match policies.get(&leaf.campaign_id) {
                Some(p) => p.clone(),
                None => match store.get(leaf.campaign_id).await {
                    Ok(root) => {
                        let p = root.policy.unwrap_or_default();
                        policies.insert(leaf.campaign_id, p.clone());
                        p
                    }
                    Err(e) => {
                        tracing::warn!(task = %leaf.task_id, error = %e, "campaign.poll: root lookup failed");
                        report.errors += 1;
                        continue;
                    }
                },
            };
            match self.poll_leaf(&*store, leaf, &policy).await {
                Step::Open => {}
                Step::Merged => report.merged += 1,
                Step::Closed => report.closed += 1,
                Step::Awaiting => report.awaiting += 1,
                Step::Error => report.errors += 1,
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    //! T13 — the PR poller (`06-test-matrix.md`), over `MemCampaigns` and a
    //! scripted forge, one test per row id (`corner_changes_requested` is the
    //! forge's `open` state). `negative_noop_poller_no_store_calls` lives with the
    //! T11 recording store in `driver/tests.rs`; the `poll_batch` config bounds are
    //! `campaign_validate_cases` rows in `agent-runtime`.

    use super::*;
    use agent_core::campaign::{Actor, CampaignError, Policy, TaskState};
    use agent_core::{Comment, CreatePrRequest, Issue, Page, PullRequest, ReviewVerdict};
    use agent_testkit::campaign::conformance::{
        campaign_with, children, dave, events, in_review, leaf, owner, ready_leaves, split_with,
        state, Harness,
    };
    use std::collections::HashSet;
    use std::sync::Mutex;

    /// A forge that answers `get_pr` from a script: a PR per number, an error
    /// per number, or a hang; every call is recorded.
    #[derive(Default)]
    struct ScriptedForge {
        prs: Mutex<HashMap<u64, Result<PullRequest, String>>>,
        hang: Mutex<HashSet<u64>>,
        calls: Mutex<Vec<u64>>,
    }

    fn pr(number: u64, state: &str) -> PullRequest {
        PullRequest {
            number,
            title: "t".into(),
            body: String::new(),
            state: state.into(),
            author: "bot".into(),
            url: format!("https://github.com/org/repo/pull/{number}"),
            source_branch: format!("campaign/leaf-{number}"),
            target_branch: "main".into(),
            draft: false,
        }
    }

    impl ScriptedForge {
        fn with(self, number: u64, state: &str) -> Self {
            self.with_pr(number, pr(number, state))
        }

        fn with_pr(self, number: u64, pr: PullRequest) -> Self {
            self.prs.lock().unwrap().insert(number, Ok(pr));
            self
        }

        fn failing(self, number: u64, msg: &str) -> Self {
            self.prs.lock().unwrap().insert(number, Err(msg.into()));
            self
        }

        fn hanging(self, number: u64) -> Self {
            self.hang.lock().unwrap().insert(number);
            self
        }

        fn calls(&self) -> Vec<u64> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Forge for ScriptedForge {
        fn name(&self) -> &str {
            "scripted"
        }
        async fn get_pr(&self, number: u64) -> agent_core::Result<PullRequest> {
            self.calls.lock().unwrap().push(number);
            if self.hang.lock().unwrap().contains(&number) {
                std::future::pending::<()>().await;
            }
            match self.prs.lock().unwrap().get(&number) {
                Some(Ok(pr)) => Ok(pr.clone()),
                Some(Err(m)) => Err(agent_core::Error::Provider(m.clone())),
                None => Err(agent_core::Error::Provider(format!("no such pr {number}"))),
            }
        }
        async fn list_prs(&self, _page: u32) -> agent_core::Result<Page<PullRequest>> {
            unimplemented!("not a poller verb")
        }
        async fn list_issues(&self, _page: u32) -> agent_core::Result<Page<Issue>> {
            unimplemented!("not a poller verb")
        }
        async fn import_issue(&self, _number: u64) -> agent_core::Result<Issue> {
            unimplemented!("not a poller verb")
        }
        async fn create_pr(&self, _req: &CreatePrRequest) -> agent_core::Result<PullRequest> {
            unimplemented!("not a poller verb")
        }
        async fn comment(&self, _number: u64, _body: &str) -> agent_core::Result<Comment> {
            unimplemented!("not a poller verb")
        }
        async fn review_pr(
            &self,
            _number: u64,
            _verdict: ReviewVerdict,
            _body: &str,
        ) -> agent_core::Result<Comment> {
            unimplemented!("not a poller verb")
        }
    }

    fn poller(forge: &Arc<ScriptedForge>) -> ForgePoller {
        ForgePoller::new(Arc::clone(forge) as Arc<dyn Forge>)
    }

    /// A campaign policy that needs no human approval (`require_pr_approval`
    /// off), with the fixture's open approval levels.
    fn no_approval() -> Policy {
        Policy {
            approve_levels: vec![],
            require_pr_approval: false,
            ..Policy::default()
        }
    }

    /// The default policy minus the approval levels: `require_pr_approval` stays
    /// on, so a merged PR waits for `approve`.
    fn approval_required() -> Policy {
        Policy {
            approve_levels: vec![],
            ..Policy::default()
        }
    }

    /// One `in_review` leaf under `policy`, on PR `n`.
    async fn one_in_review(store: &dyn CampaignStore, policy: Policy, n: i64) -> Task {
        let root = campaign_with(store, policy).await;
        let d = split_with(store, root.task_id, children(1), 7_000 + n as u64).await;
        let l = leaf(store, d.children[0].task_id).await;
        in_review(store, l.task_id, &owner("w1"), n).await
    }

    /// Two `in_review` leaves of one campaign under [`no_approval`], on PRs
    /// `n1` and `n2`.
    async fn two_in_review(store: &dyn CampaignStore, n1: i64, n2: i64) -> (Task, Task) {
        let root = campaign_with(store, no_approval()).await;
        let d = split_with(store, root.task_id, children(2), 7_200 + n1 as u64).await;
        let a = leaf(store, d.children[0].task_id).await;
        let b = leaf(store, d.children[1].task_id).await;
        let a = in_review(store, a.task_id, &owner("w1"), n1).await;
        let b = in_review(store, b.task_id, &owner("w1"), n2).await;
        (a, b)
    }

    fn last_detail(ev: &[agent_core::campaign::TaskEvent]) -> serde_json::Value {
        ev.last().map(|e| e.detail.clone()).unwrap_or_default()
    }

    #[tokio::test]
    async fn positive_merged_with_approval() {
        let h = Harness::mem();
        let s = h.a();
        let r = one_in_review(&*s, approval_required(), 7).await;
        s.approve(r.task_id, r.version, &dave()).await.unwrap();
        let forge = Arc::new(ScriptedForge::default().with(7, "merged"));
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(
            report,
            PollReport {
                polled: 1,
                merged: 1,
                ..PollReport::default()
            }
        );
        assert_eq!(state(&*s, r.task_id).await, TaskState::Done);
        let ev = events(&*s, r.task_id).await;
        let last = ev.last().unwrap();
        assert_eq!(last.actor, Actor::Poller.render());
        assert_eq!(last.detail, serde_json::json!({"review": "merged"}));
        // The root rolled up.
        assert_eq!(state(&*s, r.campaign_id).await, TaskState::Done);
        assert_eq!(forge.calls(), vec![7]);
    }

    #[tokio::test]
    async fn positive_merged_no_approval_required() {
        let h = Harness::mem();
        let s = h.a();
        let r = one_in_review(&*s, no_approval(), 8).await;
        let forge = Arc::new(ScriptedForge::default().with(8, "merged"));
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(report.merged, 1);
        assert_eq!(report.awaiting, 0);
        assert_eq!(state(&*s, r.task_id).await, TaskState::Done);
    }

    #[tokio::test]
    async fn negative_merged_without_approval() {
        let h = Harness::mem();
        let s = h.a();
        let r = one_in_review(&*s, approval_required(), 9).await;
        let forge = Arc::new(ScriptedForge::default().with(9, "merged"));
        let p = poller(&forge);
        let report = p.poll(Arc::clone(&s), 20).await;
        assert_eq!(
            report,
            PollReport {
                polled: 1,
                awaiting: 1,
                ..PollReport::default()
            }
        );
        let t = s.get(r.task_id).await.unwrap();
        assert_eq!(t, r, "no transition, no version bump");
        let ev = events(&*s, r.task_id).await;
        assert_eq!(
            last_detail(&ev),
            serde_json::json!({"awaiting_pr_approval": true})
        );
        // A second tick adds no second marker; approval then lets it through.
        let report = p.poll(Arc::clone(&s), 20).await;
        assert_eq!(report.awaiting, 1);
        assert_eq!(events(&*s, r.task_id).await.len(), ev.len());
        s.approve(r.task_id, r.version, &dave()).await.unwrap();
        let report = p.poll(Arc::clone(&s), 20).await;
        assert_eq!(report.merged, 1);
        assert_eq!(state(&*s, r.task_id).await, TaskState::Done);
    }

    #[tokio::test]
    async fn negative_closed_failed_blocks_dependents() {
        let h = Harness::mem();
        let s = h.a();
        let root = campaign_with(&*s, no_approval()).await;
        let mut specs = children(2);
        specs[1].depends_on = vec![1];
        let d = split_with(&*s, root.task_id, specs, 7_100).await;
        let first = leaf(&*s, d.children[0].task_id).await;
        let second = leaf(&*s, d.children[1].task_id).await;
        let r = in_review(&*s, first.task_id, &owner("w1"), 11).await;
        let forge = Arc::new(ScriptedForge::default().with(11, "closed"));
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(report.closed, 1);
        assert_eq!(report.errors, 0);
        assert_eq!(state(&*s, r.task_id).await, TaskState::Failed);
        assert_eq!(state(&*s, second.task_id).await, TaskState::Blocked);
        assert_eq!(
            last_detail(&events(&*s, r.task_id).await),
            serde_json::json!({"review": "closed"})
        );
    }

    #[tokio::test]
    async fn corner_changes_requested_open_untouched() {
        let h = Harness::mem();
        let s = h.a();
        let r = one_in_review(&*s, no_approval(), 12).await;
        let before = events(&*s, r.task_id).await;
        let forge = Arc::new(ScriptedForge::default().with(12, "open"));
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(
            report,
            PollReport {
                polled: 1,
                ..PollReport::default()
            }
        );
        assert_eq!(s.get(r.task_id).await.unwrap(), r);
        assert_eq!(events(&*s, r.task_id).await, before);
    }

    #[tokio::test]
    async fn corner_forge_error_logged_continues() {
        let h = Harness::mem();
        let s = h.a();
        let (a, b) = two_in_review(&*s, 21, 22).await;
        let forge = Arc::new(
            ScriptedForge::default()
                .failing(21, "502 bad gateway")
                .with(22, "merged"),
        );
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(
            report,
            PollReport {
                polled: 2,
                merged: 1,
                errors: 1,
                ..PollReport::default()
            }
        );
        assert_eq!(s.get(a.task_id).await.unwrap(), a);
        let d = last_detail(&events(&*s, a.task_id).await);
        let text = d["poll_error"].as_str().unwrap();
        assert!(text.contains("502 bad gateway"), "{text}");
        assert_eq!(state(&*s, b.task_id).await, TaskState::Done);
        assert_eq!(forge.calls(), vec![21, 22]);
    }

    #[tokio::test]
    async fn corner_pr_not_found() {
        // The forge has no such PR (a 404): one bounded `poll_error` event, the
        // leaf stays, the next leaf in the batch still resolves, and the next
        // tick asks again (the retry is one call per tick, never a loop).
        let h = Harness::mem();
        let s = h.a();
        let (a, b) = two_in_review(&*s, 23, 24).await;
        let forge = Arc::new(ScriptedForge::default().with(24, "merged"));
        let p = poller(&forge);
        let report = p.poll(Arc::clone(&s), 20).await;
        assert_eq!(report.errors, 1);
        assert_eq!(report.merged, 1);
        assert_eq!(s.get(a.task_id).await.unwrap(), a);
        let text = last_detail(&events(&*s, a.task_id).await)["poll_error"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(text.contains("no such pr 23"), "{text}");
        assert_eq!(state(&*s, b.task_id).await, TaskState::Done);
        let report = p.poll(Arc::clone(&s), 20).await;
        assert_eq!(report.errors, 1);
        assert_eq!(forge.calls(), vec![23, 24, 23]);
    }

    #[tokio::test]
    async fn positive_oldest_first() {
        // Three leaves completed out of id order on an advancing clock are
        // polled by `updated_at`, not by id.
        let h = Harness::mem();
        let s = h.a();
        let root = campaign_with(&*s, no_approval()).await;
        let d = split_with(&*s, root.task_id, children(3), 7_300).await;
        let mut ids = Vec::new();
        for c in &d.children {
            ids.push(leaf(&*s, c.task_id).await.task_id);
        }
        let mut forge = ScriptedForge::default();
        for i in [2usize, 0, 1] {
            h.advance_secs(10);
            in_review(&*s, ids[i], &owner("w1"), ids[i].0).await;
            forge = forge.with(ids[i].0.unsigned_abs(), "open");
        }
        let forge = Arc::new(forge);
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(report.polled, 3);
        let want: Vec<u64> = [2usize, 0, 1]
            .iter()
            .map(|i| ids[*i].0.unsigned_abs())
            .collect();
        assert_eq!(forge.calls(), want);
    }

    #[tokio::test(start_paused = true)]
    async fn corner_forge_timeout() {
        let h = Harness::mem();
        let s = h.a();
        let (a, b) = two_in_review(&*s, 31, 32).await;
        let forge = Arc::new(ScriptedForge::default().hanging(31).with(32, "merged"));
        let started = tokio::time::Instant::now();
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert!(started.elapsed() >= Duration::from_secs(POLL_PR_TIMEOUT_SECS));
        assert_eq!(report.errors, 1);
        assert_eq!(report.merged, 1);
        assert_eq!(s.get(a.task_id).await.unwrap(), a);
        let d = last_detail(&events(&*s, a.task_id).await);
        assert!(
            d["poll_error"].as_str().unwrap().contains("timed out"),
            "{d}"
        );
        assert_eq!(state(&*s, b.task_id).await, TaskState::Done);
    }

    #[tokio::test]
    async fn boundary_poll_batch() {
        let h = Harness::mem();
        let s = h.a();
        let mut forge = ScriptedForge::default();
        let mut all = Vec::new();
        for _ in 0..4 {
            let (_, leaves) = ready_leaves(&*s, 7).await;
            for l in leaves {
                let n = l.task_id.0;
                in_review(&*s, l.task_id, &owner("w1"), n).await;
                forge = forge.with(n.unsigned_abs(), "open");
                all.push(n.unsigned_abs());
            }
        }
        assert_eq!(all.len(), 28);
        let forge = Arc::new(forge);
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(report.polled, 20);
        assert_eq!(report.errors, 0);
        // Oldest first: the first 20 in creation order.
        assert_eq!(forge.calls(), all[..20].to_vec());
    }

    #[tokio::test]
    async fn boundary_poll_batch_one() {
        let h = Harness::mem();
        let s = h.a();
        let (_, leaves) = ready_leaves(&*s, 3).await;
        let mut forge = ScriptedForge::default();
        for l in &leaves {
            in_review(&*s, l.task_id, &owner("w1"), l.task_id.0).await;
            forge = forge.with(l.task_id.0.unsigned_abs(), "open");
        }
        let forge = Arc::new(forge);
        let report = poller(&forge).poll(Arc::clone(&s), 1).await;
        assert_eq!(report.polled, 1);
        assert_eq!(forge.calls().len(), 1);
        let report = poller(&forge).poll(Arc::clone(&s), 0).await;
        assert_eq!(report.polled, 0);
    }

    #[tokio::test]
    async fn adversarial_forge_state_garbage() {
        let h = Harness::mem();
        let s = h.a();
        let r = one_in_review(&*s, no_approval(), 41).await;
        let before = events(&*s, r.task_id).await;
        for hostile in ["MERGED\n../x", "merged ", "Merged", "", "closed\u{0}"] {
            let forge = Arc::new(ScriptedForge::default().with(41, hostile));
            let report = poller(&forge).poll(Arc::clone(&s), 20).await;
            assert_eq!(report.errors, 1, "state {hostile:?}");
            assert_eq!(report.merged + report.closed + report.awaiting, 0);
            assert_eq!(s.get(r.task_id).await.unwrap(), r, "state {hostile:?}");
        }
        assert_eq!(events(&*s, r.task_id).await, before, "nothing written");
    }

    #[tokio::test]
    async fn adversarial_forge_merged_wrong_number() {
        let h = Harness::mem();
        let s = h.a();
        let r = one_in_review(&*s, no_approval(), 7).await;
        let forge = Arc::new(ScriptedForge::default().with_pr(7, pr(99, "merged")));
        let report = poller(&forge).poll(Arc::clone(&s), 20).await;
        assert_eq!(report.errors, 1);
        assert_eq!(report.merged, 0);
        assert_eq!(s.get(r.task_id).await.unwrap(), r);
        let d = last_detail(&events(&*s, r.task_id).await);
        let text = d["poll_error"].as_str().unwrap();
        assert!(text.contains("99") && text.contains("for 7"), "{text}");
    }

    #[tokio::test]
    async fn adversarial_cross_tenant_pr() {
        let h = Harness::mem();
        let a = h.a();
        let b = h.b();
        let ra = one_in_review(&*a, no_approval(), 7).await;
        let rb = one_in_review(&*b, no_approval(), 7).await;
        let forge = Arc::new(ScriptedForge::default().with(7, "merged"));
        let report = poller(&forge).poll(Arc::clone(&a), 20).await;
        assert_eq!(report.merged, 1);
        assert_eq!(state(&*a, ra.task_id).await, TaskState::Done);
        // B's leaf on the same PR number is untouched by A's poll.
        assert_eq!(b.get(rb.task_id).await.unwrap(), rb);
        assert_eq!(
            a.get(rb.task_id).await.unwrap_err(),
            CampaignError::NotFound
        );
        assert_eq!(forge.calls(), vec![7]);
    }
}
