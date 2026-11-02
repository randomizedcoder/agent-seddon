//! Forge poll (review-fleet **C6**, increment 4a): the first real trigger source.
//!
//! One scheduled job per enabled session (driven by `agent-scheduler`'s overlap-guarded
//! `every {poll_secs}` spec — the guard means a poll that runs long never stacks a second
//! copy) calls [`poll_session`], which lists the session's open PRs, filters out drafts,
//! and emits a [`FleetTrigger`] for each onto the orchestrator's bounded, coalescing
//! [`TriggerQueue`](crate::TriggerQueue). Everything downstream of the queue is
//! trigger-source-agnostic — a polled PR and a Slack-posted link (C7, next) produce the
//! identical trigger.
//!
//! **The forge response is untrusted.** Every field is data, never a directive:
//! - `draft` filters (only non-draft PRs are reviewed);
//! - `next_page` is followed **only when it strictly advances** and never past
//!   [`MAX_POLL_PAGES`], so a forge that keeps claiming "there's more" (hostile or
//!   buggy) can never spin the poll unbounded;
//! - at most [`MAX_TRIGGERS_PER_TICK`] triggers leave one tick, so a PR flood becomes a
//!   fixed, bounded amount of downstream work.
//!
//! Dedup here is coarse — by PR number, via the queue's coalescing plus the
//! orchestrator's per-`(session, pr)` in-flight guard. Each trigger also carries the head
//! commit the PR list reported (`head_sha`), so the orchestrator answers `UpToDate` for a
//! head that already has a draft **before** fetching anything: an unchanged PR costs the
//! forge one list call per poll. A coalesced trigger keeps the first head it was queued
//! with, so a head that moves while a trigger waits is picked up by the next poll.
//!
//! A row whose polls keep failing is backed off between ticks ([`PollBackoff`]).

use agent_core::{FleetTrigger, Forge, Result, TriggerSink};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Hard cap on forge PR-list pages fetched in one poll tick. A forge response is
/// attacker-controlled, so its `next_page` can never walk us past this many fetches.
pub const MAX_POLL_PAGES: u32 = 10;

/// Hard cap on triggers emitted from a single poll tick — bounds a PR flood (a forge
/// returning thousands of open PRs) into a fixed amount of downstream review work.
pub const MAX_TRIGGERS_PER_TICK: usize = 64;

/// What one [`poll_session`] pass observed and did — returned for logging/metrics and
/// asserted by the tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PollReport {
    /// PRs examined across every fetched page.
    pub scanned: usize,
    /// Non-draft PRs whose trigger was handed to the sink (within the per-tick cap).
    pub emitted: usize,
    /// Pages actually fetched from the forge.
    pub pages: u32,
    /// `true` if the per-tick trigger cap was hit (further non-draft PRs went unemitted
    /// this tick; the next tick picks them up).
    pub capped: bool,
}

/// Poll one session's forge for open, non-draft PRs and emit a [`FleetTrigger`] for each
/// onto `sink`. `max_triggers` is clamped to [`MAX_TRIGGERS_PER_TICK`]. `tenant` is the
/// roster scope the row was read from (forge-auth F6; empty ⇒ unscoped) and is stamped on
/// every trigger so the drain resolves the row in that tenant. See the module docs for the
/// untrusted-response contract.
pub async fn poll_session(
    forge: &dyn Forge,
    tenant: &str,
    session_id: &str,
    sink: &dyn TriggerSink,
    max_triggers: usize,
) -> Result<PollReport> {
    let cap = max_triggers.min(MAX_TRIGGERS_PER_TICK);
    let mut report = PollReport::default();
    let mut page: u32 = 1;
    loop {
        if report.pages >= MAX_POLL_PAGES {
            break;
        }
        let result = forge.list_prs(page).await?;
        report.pages += 1;
        for pr in &result.items {
            report.scanned += 1;
            if pr.draft {
                continue;
            }
            if report.emitted >= cap {
                // A flood: stop mid-walk rather than emit past the cap. The remaining
                // non-draft PRs are picked up on a later tick.
                report.capped = true;
                return Ok(report);
            }
            sink.enqueue(FleetTrigger {
                // The listed head lets the orchestrator skip an unchanged PR unfetched.
                head_sha: pr.head_sha.clone(),
                session_id: session_id.to_string(),
                pr_number: pr.number,
                requested_by: Vec::new(),
                tenant: tenant.to_string(),
            });
            report.emitted += 1;
        }
        match result.next_page {
            // Follow the forge's paging only when it *advances*. A response pointing at
            // the same or an earlier page (hostile or buggy) ends the walk rather than
            // looping — the page cap above is the belt to this suspenders.
            Some(next) if next > page => page = next,
            _ => break,
        }
    }
    Ok(report)
}

/// The first wait after a failed poll. Each further consecutive failure doubles it.
pub const POLL_BACKOFF_BASE: Duration = Duration::from_secs(60);
/// The longest wait between polls of a failing row.
pub const POLL_BACKOFF_MAX: Duration = Duration::from_secs(30 * 60);
/// Most failing poll jobs tracked at once (jobs are roster rows, so this is headroom).
/// Past it, the job whose backoff ends soonest is forgotten.
pub const MAX_BACKOFF_JOBS: usize = 4096;

/// Backoff **between** forge polls. A job's regular cadence (`poll_secs`) keeps firing
/// while the forge is failing or asking us to slow down, so each failure pushes that
/// job's next poll out: `POLL_BACKOFF_BASE` doubling per consecutive failure, capped at
/// `POLL_BACKOFF_MAX`, with equal jitter so many rows don't return in step. A clean poll
/// clears it. The schedule is `agent_retry`'s, not a second implementation.
///
/// This is the across-ticks half; the forge client paces each request itself
/// (`agent_forge` gates a quota on `Retry-After`/reset, and its failure lands here).
#[derive(Default)]
pub struct PollBackoff {
    failing: Mutex<HashMap<String, Failing>>,
}

struct Failing {
    failures: u32,
    until: Instant,
}

impl PollBackoff {
    pub fn new() -> Self {
        Self::default()
    }

    fn policy() -> agent_retry::RetryPolicy {
        agent_retry::RetryPolicy::new(0)
            .with_base_delay(POLL_BACKOFF_BASE)
            .with_max_delay(POLL_BACKOFF_MAX)
            .with_jitter(agent_retry::Jitter::Equal)
    }

    /// How much longer `job` must wait before it polls again, or `None` to poll now.
    pub fn wait(&self, job: &str, now: Instant) -> Option<Duration> {
        let failing = self
            .failing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        failing
            .get(job)
            .and_then(|f| f.until.checked_duration_since(now))
            .filter(|left| !left.is_zero())
    }

    /// Record a poll's outcome. A success clears the job; a failure pushes its next poll
    /// out and returns the wait set. Jitter comes from `agent_retry::rand01`.
    pub fn record(&self, job: &str, ok: bool, now: Instant) -> Option<Duration> {
        self.record_with(job, ok, now, agent_retry::rand01())
    }

    /// [`Self::record`] with the jitter draw `rand01` ∈ `[0, 1)` supplied (tests).
    pub fn record_with(&self, job: &str, ok: bool, now: Instant, rand01: f64) -> Option<Duration> {
        let mut failing = self
            .failing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ok {
            failing.remove(job);
            return None;
        }
        if !failing.contains_key(job) && failing.len() >= MAX_BACKOFF_JOBS {
            if let Some(soonest) = failing
                .iter()
                .min_by_key(|(_, f)| f.until)
                .map(|(k, _)| k.clone())
            {
                failing.remove(&soonest);
            }
        }
        let entry = failing.entry(job.to_string()).or_insert(Failing {
            failures: 0,
            until: now,
        });
        // `delay(n)` is the wait after the (n+1)th consecutive failure.
        let wait = Self::policy().delay(entry.failures, rand01);
        entry.failures = entry.failures.saturating_add(1);
        entry.until = now + wait;
        Some(wait)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{
        Comment, CreatePrRequest, Error, Issue, Page, PullRequest, ReviewVerdict, TriggerOutcome,
    };
    use async_trait::async_trait;
    use rstest::rstest;
    use std::sync::Mutex;

    // ---- doubles ---------------------------------------------------------

    /// A forge returning scripted PR pages; only `list_prs` is exercised by the poll,
    /// so every other method is unreachable. `err_on` makes `list_prs` fail on a given
    /// page (to prove errors propagate). A page beyond the script is an empty final page.
    struct FakeForge {
        pages: Vec<Page<PullRequest>>,
        err_on: Option<u32>,
    }

    impl FakeForge {
        fn new(pages: Vec<Page<PullRequest>>, err_on: Option<u32>) -> Self {
            Self { pages, err_on }
        }
    }

    #[async_trait]
    impl Forge for FakeForge {
        fn name(&self) -> &str {
            "fake"
        }
        async fn get_pr(&self, _: u64) -> Result<PullRequest> {
            unreachable!("poll never calls get_pr")
        }
        async fn list_prs(&self, page: u32) -> Result<Page<PullRequest>> {
            if self.err_on == Some(page) {
                return Err(Error::Repo(format!("forge boom on page {page}")));
            }
            let idx = (page as usize).saturating_sub(1);
            Ok(self.pages.get(idx).cloned().unwrap_or(Page {
                items: vec![],
                next_page: None,
            }))
        }
        async fn list_issues(&self, _: u32) -> Result<Page<Issue>> {
            unreachable!("poll never calls list_issues")
        }
        async fn import_issue(&self, _: u64) -> Result<Issue> {
            unreachable!("poll never calls import_issue")
        }
        async fn create_pr(&self, _: &CreatePrRequest) -> Result<PullRequest> {
            unreachable!("poll never writes")
        }
        async fn comment(&self, _: u64, _: &str) -> Result<Comment> {
            unreachable!("poll never writes")
        }
        async fn review_pr(&self, _: u64, _: ReviewVerdict, _: &str) -> Result<Comment> {
            unreachable!("poll never writes")
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        got: Mutex<Vec<FleetTrigger>>,
    }
    impl TriggerSink for RecordingSink {
        fn enqueue(&self, t: FleetTrigger) -> TriggerOutcome {
            self.got.lock().unwrap().push(t);
            TriggerOutcome::Accepted
        }
    }

    // ---- builders --------------------------------------------------------

    fn pr(number: u64, draft: bool) -> PullRequest {
        PullRequest {
            number,
            title: String::new(),
            body: String::new(),
            state: "open".into(),
            author: String::new(),
            url: String::new(),
            source_branch: String::new(),
            target_branch: String::new(),
            draft,
            head_sha: String::new(),
        }
    }

    fn page(items: Vec<PullRequest>, next: Option<u32>) -> Page<PullRequest> {
        Page {
            items,
            next_page: next,
        }
    }

    /// `n` pages, each with one non-draft PR (number == page) and each claiming there is
    /// *always* another page — even the last one. A well-behaved poll must still stop at
    /// [`MAX_POLL_PAGES`].
    fn always_more_pages(n: u32) -> Vec<Page<PullRequest>> {
        (1..=n)
            .map(|i| page(vec![pr(i as u64, false)], Some(i + 1)))
            .collect()
    }

    /// One page carrying `n` non-draft PRs numbered `1..=n` — a flood.
    fn flood_page(n: u64) -> Vec<Page<PullRequest>> {
        vec![page((1..=n).map(|i| pr(i, false)).collect(), None)]
    }

    // ---- table -----------------------------------------------------------

    enum Expect {
        /// Poll succeeded: exactly these PR numbers emitted (in order), this many pages
        /// fetched, and this `capped` flag.
        Ok {
            emitted: Vec<u64>,
            pages: u32,
            capped: bool,
        },
        /// Poll returned an error (a forge failure propagates, not swallowed).
        Err,
    }

    struct Case {
        desc: &'static str,
        pages: Vec<Page<PullRequest>>,
        err_on: Option<u32>,
        max_triggers: usize,
        expect: Expect,
    }

    #[rstest]
    #[case::positive_emits_trigger_for_nondraft_pr(Case {
        desc: "two open non-draft PRs each emit a trigger",
        pages: vec![page(vec![pr(1, false), pr(2, false)], None)],
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok { emitted: vec![1, 2], pages: 1, capped: false },
    })]
    #[case::positive_draft_pr_is_filtered_out(Case {
        desc: "a draft PR between two non-drafts is skipped",
        pages: vec![page(vec![pr(1, false), pr(2, true), pr(3, false)], None)],
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok { emitted: vec![1, 3], pages: 1, capped: false },
    })]
    #[case::positive_multi_page_follows_next_page(Case {
        desc: "an advancing next_page is followed across two pages",
        pages: vec![
            page(vec![pr(1, false)], Some(2)),
            page(vec![pr(2, false)], None),
        ],
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok { emitted: vec![1, 2], pages: 2, capped: false },
    })]
    #[case::negative_forge_error_propagates(Case {
        desc: "a forge failure on the first page surfaces as Err (not a silent empty poll)",
        pages: vec![page(vec![pr(1, false)], None)],
        err_on: Some(1),
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Err,
    })]
    #[case::boundary_empty_pr_list_emits_nothing(Case {
        desc: "an empty first page emits nothing and stops",
        pages: vec![page(vec![], None)],
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok { emitted: vec![], pages: 1, capped: false },
    })]
    #[case::boundary_pr_number_max_u64(Case {
        desc: "a u64::MAX PR number is carried through verbatim",
        pages: vec![page(vec![pr(u64::MAX, false)], None)],
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok { emitted: vec![u64::MAX], pages: 1, capped: false },
    })]
    #[case::corner_all_draft_emits_nothing(Case {
        desc: "a page of only drafts scans but emits nothing",
        pages: vec![page(vec![pr(1, true), pr(2, true)], None)],
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok { emitted: vec![], pages: 1, capped: false },
    })]
    #[case::corner_next_page_self_loop_stops(Case {
        desc: "next_page pointing back at the current page ends the walk (no infinite loop)",
        pages: vec![page(vec![pr(1, false)], Some(1))],
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok { emitted: vec![1], pages: 1, capped: false },
    })]
    #[case::adversarial_hostile_page_count_clamped(Case {
        desc: "a forge that always claims another page is clamped to MAX_POLL_PAGES fetches",
        pages: always_more_pages(MAX_POLL_PAGES + 2),
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok {
            emitted: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            pages: MAX_POLL_PAGES,
            capped: false,
        },
    })]
    #[case::adversarial_pr_flood_capped_per_tick(Case {
        desc: "100 non-draft PRs on one page emit only MAX_TRIGGERS_PER_TICK, capped",
        pages: flood_page(100),
        err_on: None,
        max_triggers: MAX_TRIGGERS_PER_TICK,
        expect: Expect::Ok {
            emitted: (1..=MAX_TRIGGERS_PER_TICK as u64).collect(),
            pages: 1,
            capped: true,
        },
    })]
    #[tokio::test]
    async fn poll_session_cases(#[case] case: Case) {
        let forge = FakeForge::new(case.pages, case.err_on);
        let sink = RecordingSink::default();
        let got = poll_session(&forge, "acme", "sess", &sink, case.max_triggers).await;

        match case.expect {
            Expect::Err => {
                assert!(got.is_err(), "{}: expected Err", case.desc);
                assert!(
                    sink.got.lock().unwrap().is_empty(),
                    "{}: no triggers emitted on error",
                    case.desc
                );
            }
            Expect::Ok {
                emitted,
                pages,
                capped,
            } => {
                let report = got.unwrap_or_else(|e| panic!("{}: unexpected Err {e}", case.desc));
                assert_eq!(report.pages, pages, "{}: pages fetched", case.desc);
                assert_eq!(report.capped, capped, "{}: capped flag", case.desc);
                assert_eq!(
                    report.emitted,
                    emitted.len(),
                    "{}: emitted count",
                    case.desc
                );
                let recorded = sink.got.lock().unwrap();
                assert!(
                    recorded.iter().all(|t| t.tenant == "acme"),
                    "{}: every trigger carries the polled tenant",
                    case.desc
                );
                let nums: Vec<u64> = recorded.iter().map(|t| t.pr_number).collect();
                assert_eq!(nums, emitted, "{}: emitted PR numbers", case.desc);
                assert!(
                    recorded.iter().all(|t| t.session_id == "sess"),
                    "{}: every trigger carries the session id",
                    case.desc
                );
            }
        }
    }

    /// desc: each trigger carries the head its PR reported, so the orchestrator can
    /// skip an unchanged PR without fetching. expect: the listed head, or empty when the
    /// forge reported none.
    #[rstest]
    #[case::positive_head_is_handed_on("the listed head rides on the trigger", &"a".repeat(40))]
    #[case::boundary_no_head_is_empty("no listed head leaves the hint empty", "")]
    #[tokio::test]
    async fn trigger_carries_the_listed_head(#[case] desc: &str, #[case] head: &str) {
        let mut listed = pr(5, false);
        listed.head_sha = head.to_string();
        let forge = FakeForge::new(vec![page(vec![listed], None)], None);
        let sink = RecordingSink::default();
        poll_session(&forge, "acme", "sess", &sink, MAX_TRIGGERS_PER_TICK)
            .await
            .unwrap();
        let got = sink.got.lock().unwrap();
        assert_eq!(got[0].head_sha, head, "{desc}");
    }

    // ---- PollBackoff -----------------------------------------------------

    fn mins(n: u64) -> Duration {
        Duration::from_secs(n * 60)
    }

    /// The wait after each of `outcomes` (true = a clean poll), with the jitter draw
    /// pinned at its top (`rand01` → 1, i.e. the full ceiling).
    #[rstest]
    #[case::positive_first_failure_waits_a_minute("one failure waits a minute", &[false], &[Some(mins(1))])]
    #[case::positive_doubles_per_failure(
        "consecutive failures double the wait",
        &[false, false, false],
        &[Some(mins(1)), Some(mins(2)), Some(mins(4))]
    )]
    #[case::positive_success_clears("a clean poll clears the backoff", &[false, false, true, false], &[Some(mins(1)), Some(mins(2)), None, Some(mins(1))])]
    #[case::negative_success_sets_nothing("a clean poll sets no wait", &[true], &[None])]
    #[case::boundary_capped_at_half_an_hour(
        "the wait never passes half an hour",
        &[false; 8],
        &[Some(mins(1)), Some(mins(2)), Some(mins(4)), Some(mins(8)), Some(mins(16)), Some(mins(30)), Some(mins(30)), Some(mins(30))]
    )]
    fn poll_backoff_schedule_cases(
        #[case] desc: &str,
        #[case] outcomes: &[bool],
        #[case] want: &[Option<Duration>],
    ) {
        let b = PollBackoff::new();
        let t0 = Instant::now();
        let got: Vec<_> = outcomes
            .iter()
            .map(|ok| {
                b.record_with("job", *ok, t0, 0.999_999)
                    .map(|d| Duration::from_secs((d.as_millis() as u64).div_ceil(1000)))
            })
            .collect();
        assert_eq!(got, want, "{desc}");
    }

    /// Equal jitter keeps at least half the ceiling, so a failing row never comes back
    /// sooner than half its wait.
    #[rstest]
    #[case::boundary_lowest_draw(
        "the lowest draw waits half the ceiling",
        0.0,
        Duration::from_secs(30)
    )]
    #[case::corner_middle_draw("a middle draw waits three quarters", 0.5, Duration::from_secs(45))]
    fn poll_backoff_jitter_floor(#[case] desc: &str, #[case] rand01: f64, #[case] want: Duration) {
        let b = PollBackoff::new();
        assert_eq!(
            b.record_with("job", false, Instant::now(), rand01),
            Some(want),
            "{desc}"
        );
    }

    /// A failing job is held until its wait has passed; other jobs are untouched.
    #[test]
    fn positive_wait_holds_then_releases_and_is_per_job() {
        let b = PollBackoff::new();
        let t0 = Instant::now();
        let wait = b.record_with("acme/web", false, t0, 0.999_999).unwrap();
        assert_eq!(
            b.wait("acme/web", t0),
            Some(wait),
            "held right after the failure"
        );
        assert_eq!(b.wait("acme/api", t0), None, "another job is not held");
        assert_eq!(
            b.wait("acme/web", t0 + wait),
            None,
            "released once the wait has passed"
        );
    }

    /// The map of failing jobs is bounded: past the cap the soonest-ending is forgotten.
    #[test]
    fn adversarial_failing_jobs_are_bounded() {
        let b = PollBackoff::new();
        let t0 = Instant::now();
        for i in 0..MAX_BACKOFF_JOBS {
            b.record_with(
                &format!("j{i}"),
                false,
                t0 + Duration::from_millis(i as u64),
                0.0,
            );
        }
        b.record_with("newcomer", false, t0 + mins(1), 0.0);
        assert_eq!(b.failing.lock().unwrap().len(), MAX_BACKOFF_JOBS);
        assert!(
            b.wait("newcomer", t0 + mins(1)).is_some(),
            "the new job is tracked"
        );
        assert_eq!(b.wait("j0", t0), None, "the soonest-ending job made room");
    }

    /// Thousands of failures never overflow the counter or the wait.
    #[test]
    fn adversarial_endless_failures_stay_capped() {
        let b = PollBackoff::new();
        let t0 = Instant::now();
        let mut last = None;
        for _ in 0..10_000 {
            last = b.record_with("job", false, t0, 0.999_999);
        }
        assert!(last.unwrap() <= POLL_BACKOFF_MAX);
    }
}
