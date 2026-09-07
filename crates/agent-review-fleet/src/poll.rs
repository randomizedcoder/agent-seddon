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
//! orchestrator's per-`(session, pr)` in-flight guard. Precise head-oid re-review dedup
//! (a PR whose head moved) is inc 6 (C14): `PullRequest` carries no head SHA, so the
//! oid is only known post-checkout.

use agent_core::{FleetTrigger, Forge, Result, TriggerSink};

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
/// onto `sink`. `max_triggers` is clamped to [`MAX_TRIGGERS_PER_TICK`]. See the module
/// docs for the untrusted-response contract.
pub async fn poll_session(
    forge: &dyn Forge,
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
                session_id: session_id.to_string(),
                pr_number: pr.number,
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
        let got = poll_session(&forge, "sess", &sink, case.max_triggers).await;

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
}
