//! The fleet orchestrator (review-fleet C1/C8, increment 3c — **skeleton**): the two
//! loops that turn a durable roster into running reviews.
//!
//! - [`reconcile`] rebuilds the live session set from the roster (the source of
//!   truth): for each **enabled** row it fail-closed-checks the row's forge
//!   credential (C5) and then admits a capacity-checked placeholder **owner** session.
//!   It is idempotent, so booting, re-running it, or reacting to a control-plane edit
//!   all converge to the same set — the crash-safe rebuild the design calls for.
//! - [`FleetOrchestrator`] drives one PR through the state machine
//!   `triggered → cloning → reviewing`: fetch the PR head (C9), materialize a
//!   read-only worktree, mint the PR-scoped `SessionKey`, and start a review run on it.
//!   A [`TriggerQueue`] feeds it — **bounded** and **coalescing** (an over-capacity or
//!   duplicate trigger folds into the pending one and is logged, never silently
//!   dropped).
//!
//! **What is deliberately *not* here yet** (later increments, per the plan): the real
//! triggers (forge poll C6 / Slack watch C7, inc 4), the review skill + collectors
//! (inc 5), and the draft → approve → post tail + head-oid dedup (C14, inc 6). Dedup
//! here is by `(session_id, pr_number)`: one in-flight review per PR.
//!
//! **Untrusted throughout.** Row fields come from a gRPC peer / hand-edited file, and a
//! `pr_number` from a trigger source; ids are re-validated (`SessionKey::parse`) before
//! becoming a path segment, and an unresolvable forge credential keeps the session
//! **disabled** (fail closed) rather than admitting a broken session.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use agent_core::{
    encode_review_session_id, FleetHost, FleetRegistry, FleetSession, FleetTrigger, RepoBackend,
    RunHandle, SessionKey, TriggerOutcome, TriggerSink, UserId, WorktreeSpec,
};

/// A forge-credential check for one row (C5): `Ok(())` when the row's `token_ref`
/// resolves to a usable secret and its backend forge can be built; `Err(reason)` when
/// it cannot, so reconcile keeps the session **disabled** (fail closed). Injected as a
/// closure so this crate stays free of the concrete `agent-forge`/token machinery
/// (which lives in `agent-runtime`) and the reconcile path is testable with a double.
pub type ForgeCheck = dyn Fn(&FleetSession) -> Result<(), String> + Send + Sync;

/// The outcome of a single [`reconcile`] pass: which owner sessions are live and which
/// enabled rows were skipped (with a reason) — a broken credential, a bad id, or a
/// capacity cap. Skipped rows are **not** admitted; they are logged and left for the
/// next reconcile (a fixed credential / freed capacity readmits them).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// The owner `SessionKey`s admitted (or already live) after this pass.
    pub admitted: Vec<SessionKey>,
    /// `(row id, reason)` for each enabled row that could not be admitted.
    pub skipped: Vec<(String, String)>,
}

/// The owner `SessionKey` for a roster row: `user = <org>`, `session = <row id>`. Both
/// are re-validated (`SessionKey::parse`) — a row that fails is skipped, never
/// sanitized. (The PR-scoped review key is a *different* key, minted per trigger.)
fn owner_key(row: &FleetSession) -> Result<SessionKey, String> {
    SessionKey::parse(&row.user, &row.id).map_err(|e| format!("bad owner key: {e}"))
}

/// Rebuild the live owner-session set from the roster (the source of truth). For each
/// **enabled** row: run `forge_check` (fail closed → skip on `Err`), then admit a
/// capacity-checked owner session via `host`. Disabled rows are never admitted. Callers
/// that also need to *drop* sessions for rows that flipped disabled/removed do so via
/// [`FleetHost::remove_session`] (the serve loop's mutation path); a from-boot reconcile
/// only needs to admit, since a fresh manager starts empty.
pub async fn reconcile(
    roster: &dyn FleetRegistry,
    host: &dyn FleetHost,
    forge_check: &ForgeCheck,
) -> ReconcileReport {
    let mut report = ReconcileReport::default();
    let rows = match roster.list().await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "fleet reconcile: roster list failed; no sessions admitted");
            return report;
        }
    };
    for row in rows.iter().filter(|r| r.enabled) {
        // C5 fail-closed credential gate: a session whose forge cannot be built stays
        // disabled (never admit a broken session). Progress-channel surfacing is inc 7.
        if let Err(reason) = forge_check(row) {
            tracing::warn!(id = %row.id, %reason, "fleet reconcile: row kept disabled (forge)");
            report.skipped.push((row.id.clone(), reason));
            continue;
        }
        let key = match owner_key(row) {
            Ok(k) => k,
            Err(reason) => {
                tracing::warn!(id = %row.id, %reason, "fleet reconcile: row skipped (bad id)");
                report.skipped.push((row.id.clone(), reason));
                continue;
            }
        };
        match host.admit_owner(key.clone()) {
            Ok(()) => report.admitted.push(key),
            Err(cap) => {
                tracing::warn!(id = %row.id, cap = %cap, "fleet reconcile: row shed (capacity)");
                report
                    .skipped
                    .push((row.id.clone(), format!("capacity: {cap}")));
            }
        }
    }
    report
}

/// Shared state of a [`TriggerQueue`]/[`TriggerReceiver`] pair: the set of
/// `(session_id, pr_number)` keys currently *pending* (queued but not yet popped), used
/// to coalesce duplicates before they reach the orchestrator.
#[derive(Default)]
struct QueueShared {
    pending: Mutex<HashSet<(String, u64)>>,
}

/// The producer half of the orchestrator's **bounded, coalescing** trigger queue,
/// exposed to trigger sources as a [`TriggerSink`]. `enqueue` never blocks and never
/// rejects: a duplicate (already pending) or an over-capacity trigger is *coalesced*
/// (folded into the pending one and logged), reported as [`TriggerOutcome::Coalesced`].
pub struct TriggerQueue {
    tx: tokio::sync::mpsc::Sender<FleetTrigger>,
    shared: Arc<QueueShared>,
}

/// The consumer half: the serve loop pops triggers here and drives each through
/// [`FleetOrchestrator::handle`]. Popping clears the trigger's *pending* mark, so a
/// fresh trigger for the same PR that arrives while a review is in flight is caught by
/// the orchestrator's in-flight guard rather than the queue.
pub struct TriggerReceiver {
    rx: tokio::sync::mpsc::Receiver<FleetTrigger>,
    shared: Arc<QueueShared>,
}

impl TriggerQueue {
    /// Build a bounded queue of `capacity` triggers, returning the sink half (to hand to
    /// trigger sources) and the receiver half (to drive in the serve loop). `capacity`
    /// is clamped to at least 1.
    pub fn channel(capacity: usize) -> (Arc<TriggerQueue>, TriggerReceiver) {
        let (tx, rx) = tokio::sync::mpsc::channel(capacity.max(1));
        let shared = Arc::new(QueueShared::default());
        (
            Arc::new(TriggerQueue {
                tx,
                shared: shared.clone(),
            }),
            TriggerReceiver { rx, shared },
        )
    }
}

impl TriggerSink for TriggerQueue {
    fn enqueue(&self, trigger: FleetTrigger) -> TriggerOutcome {
        let kt = (trigger.session_id.clone(), trigger.pr_number);
        let mut pending = self.shared.pending.lock().expect("queue pending poisoned");
        if pending.contains(&kt) {
            tracing::debug!(session_id = %trigger.session_id, pr = trigger.pr_number,
                "fleet trigger coalesced (already pending)");
            return TriggerOutcome::Coalesced;
        }
        match self.tx.try_send(trigger.clone()) {
            Ok(()) => {
                pending.insert(kt);
                TriggerOutcome::Accepted
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                // Bounded, but never a silent drop: fold it into the backlog and log.
                tracing::warn!(session_id = %trigger.session_id, pr = trigger.pr_number,
                    "fleet trigger coalesced (queue full)");
                TriggerOutcome::Coalesced
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(session_id = %trigger.session_id, pr = trigger.pr_number,
                    "fleet trigger dropped (drain loop gone)");
                TriggerOutcome::Coalesced
            }
        }
    }
}

impl TriggerReceiver {
    /// Await the next trigger, clearing its pending mark. `None` once every
    /// [`TriggerQueue`] is dropped and the backlog is drained.
    pub async fn recv(&mut self) -> Option<FleetTrigger> {
        let trigger = self.rx.recv().await?;
        self.shared
            .pending
            .lock()
            .expect("queue pending poisoned")
            .remove(&(trigger.session_id.clone(), trigger.pr_number));
        Some(trigger)
    }
}

/// How [`FleetOrchestrator::handle`] resolved one trigger.
#[derive(Debug)]
pub enum Handled {
    /// The FSM ran `triggered → cloning → reviewing`: the PR head was fetched, a
    /// worktree materialized, and a review run started on `key`.
    Reviewing { key: SessionKey },
    /// A review for this `(session_id, pr_number)` was already in flight — a no-op
    /// (no fetch, no new run). Head-oid–aware re-review is inc 6 (C14).
    Duplicate,
}

/// Drives one PR through the review state machine and keeps its run alive. **Single
/// consumer:** [`Self::handle`] is meant to be called serially by one drain loop (the
/// bounded [`TriggerQueue`] serializes triggers), so its check-then-insert of the
/// in-flight guard needs no cross-task locking beyond the guard itself.
pub struct FleetOrchestrator {
    roster: Arc<dyn FleetRegistry>,
    repo: Arc<dyn RepoBackend>,
    host: Arc<dyn FleetHost>,
    /// Live review runs, keyed by `(session_id, pr_number)`. Holding the [`RunHandle`]
    /// keeps the run from cancelling (drop = cancel); membership is the in-flight guard.
    /// Completion-driven removal is inc 6 — until then a run is cleared only by
    /// [`Self::cancel`] or dropping the orchestrator.
    in_flight: Mutex<HashMap<(String, u64), RunHandle>>,
}

impl FleetOrchestrator {
    pub fn new(
        roster: Arc<dyn FleetRegistry>,
        repo: Arc<dyn RepoBackend>,
        host: Arc<dyn FleetHost>,
    ) -> Self {
        Self {
            roster,
            repo,
            host,
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// The skeleton review goal for a PR. The real, skill-selected prompt is inc 5; this
    /// is a grounded, side-effect-free instruction so the driven run is meaningful and
    /// never posts. `repo`/`skill` come from the (validated) roster row.
    fn review_goal(row: &FleetSession, pr: u64) -> String {
        let skill = if row.skill.is_empty() {
            "code review"
        } else {
            row.skill.as_str()
        };
        format!(
            "Perform a {skill} of pull request #{pr} in repository `{}`. \
             Summarize findings only; do not post or push anything.",
            row.repo
        )
    }

    /// Whether a review for `(session_id, pr_number)` is currently in flight.
    pub fn is_in_flight(&self, session_id: &str, pr_number: u64) -> bool {
        self.in_flight
            .lock()
            .expect("in_flight poisoned")
            .contains_key(&(session_id.to_string(), pr_number))
    }

    /// Cancel and clear an in-flight review (dropping its [`RunHandle`] cancels the run).
    /// Returns whether one was present.
    pub fn cancel(&self, session_id: &str, pr_number: u64) -> bool {
        self.in_flight
            .lock()
            .expect("in_flight poisoned")
            .remove(&(session_id.to_string(), pr_number))
            .is_some()
    }

    /// Drive one trigger through `triggered → cloning → reviewing`.
    ///
    /// Fails closed with `Err` on an unknown row, a fetch/worktree failure, or a
    /// capacity cap (the caller logs it). A duplicate `(session_id, pr_number)` returns
    /// [`Handled::Duplicate`] without touching the forge or the session.
    pub async fn handle(&self, trigger: FleetTrigger) -> agent_core::Result<Handled> {
        let kt = (trigger.session_id.clone(), trigger.pr_number);
        // C14-lite dedup: one in-flight review per (session, PR). Checked up front so a
        // duplicate never re-fetches.
        if self
            .in_flight
            .lock()
            .expect("in_flight poisoned")
            .contains_key(&kt)
        {
            tracing::debug!(session_id = %trigger.session_id, pr = trigger.pr_number,
                "fleet: duplicate trigger ignored (already in flight)");
            return Ok(Handled::Duplicate);
        }

        // Look up the (validated) row so we key the review by its org + repo.
        let row = self.roster.get(&trigger.session_id).await?;
        let pr = trigger.pr_number;

        // triggered → cloning: fetch the PR head (C9) and materialize a read-only
        // worktree at it. Both are fail-hard — a review must run against the real head.
        let head = self.repo.fetch_pr(pr).await?;
        let _worktree = self
            .repo
            .worktree_add(&WorktreeSpec {
                revision: head,
                writable: false,
                id: Some(format!("pr-{pr}")),
            })
            .await?;

        // cloning → reviewing: mint the PR-scoped key (user = org, session =
        // encode_review_session_id(repo, pr)) and start a review run on it.
        let key = SessionKey {
            user: UserId::new(row.user.as_str()),
            session: encode_review_session_id(&row.repo, pr),
        };
        let run = self
            .host
            .start_review(
                key.clone(),
                Self::review_goal(&row, pr),
                Some(row.skill.clone()),
            )
            .map_err(|e| agent_core::Error::Fleet(format!("admit review session: {e}")))?;
        // Keep the run alive (drop = cancel) and mark it in flight.
        self.in_flight
            .lock()
            .expect("in_flight poisoned")
            .insert(kt, run);
        tracing::info!(session_id = %trigger.session_id, pr, session = %key.session.as_str(),
            "fleet: review started");
        Ok(Handled::Reviewing { key })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryFleet;
    use agent_core::DriverError;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // ---- doubles ----------------------------------------------------------

    /// A `RunHandle` guard whose `Drop` records cancellation, so a test can assert that
    /// dropping the orchestrator's handle actually cancels the run.
    struct CancelFlag(Arc<AtomicBool>);
    impl Drop for CancelFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Default)]
    struct HostState {
        admitted: Vec<SessionKey>,
        removed: Vec<SessionKey>,
        reviews: Vec<(SessionKey, String, Option<String>)>,
        /// One cancel flag per started review (index-aligned with `reviews`).
        cancels: Vec<Arc<AtomicBool>>,
    }

    /// A [`FleetHost`] double with a configurable global cap. Admits are idempotent
    /// (re-admitting a live key is a no-op success), matching the real manager, so
    /// reconcile is idempotent. `start_review` hands back a cancel-on-drop handle whose
    /// flag the test keeps a clone of.
    struct FakeHost {
        max_total: usize,
        state: Mutex<HostState>,
    }
    impl FakeHost {
        fn new(max_total: usize) -> Self {
            Self {
                max_total,
                state: Mutex::new(HostState::default()),
            }
        }
        fn admitted(&self) -> Vec<SessionKey> {
            self.state.lock().unwrap().admitted.clone()
        }
    }
    impl FleetHost for FakeHost {
        fn admit_owner(&self, key: SessionKey) -> Result<(), DriverError> {
            let mut s = self.state.lock().unwrap();
            if s.admitted.contains(&key) {
                return Ok(()); // idempotent
            }
            if self.max_total > 0 && s.admitted.len() >= self.max_total {
                return Err(DriverError::TotalLimit(self.max_total));
            }
            s.admitted.push(key);
            Ok(())
        }
        fn remove_session(&self, key: &SessionKey) {
            let mut s = self.state.lock().unwrap();
            s.removed.push(key.clone());
            s.admitted.retain(|k| k != key);
        }
        fn start_review(
            &self,
            key: SessionKey,
            goal: String,
            skill: Option<String>,
        ) -> Result<RunHandle, DriverError> {
            let mut s = self.state.lock().unwrap();
            if self.max_total > 0
                && !s.admitted.contains(&key)
                && s.admitted.len() >= self.max_total
            {
                return Err(DriverError::TotalLimit(self.max_total));
            }
            let flag = Arc::new(AtomicBool::new(false));
            s.reviews.push((key.clone(), goal, skill));
            s.cancels.push(flag.clone());
            if !s.admitted.contains(&key) {
                s.admitted.push(key);
            }
            Ok(RunHandle::new(CancelFlag(flag)))
        }
    }

    /// A roster row that passes `validate()`: `repo` is the `owner__name` safe-segment
    /// convention the forge builder decodes.
    fn row(id: &str, enabled: bool) -> FleetSession {
        FleetSession {
            id: id.into(),
            user: "acme".into(),
            repo: "acme__web".into(),
            backend: "github".into(),
            token_ref: "env:ACME_GH_TOKEN".into(),
            poll_secs: 300,
            enabled,
            ..Default::default()
        }
    }

    async fn seeded(rows: &[FleetSession]) -> Arc<MemoryFleet> {
        let roster = Arc::new(MemoryFleet::new());
        for r in rows {
            roster.put(r.clone()).await.expect("seed row");
        }
        roster
    }

    /// A forge check that always passes (the credential resolves) — the fail-closed
    /// path is exercised by `always_broken`.
    fn always_ok() -> Box<ForgeCheck> {
        Box::new(|_row: &FleetSession| Ok(()))
    }
    fn always_broken() -> Box<ForgeCheck> {
        Box::new(|_row: &FleetSession| Err("token_ref file missing".into()))
    }

    // ---- reconcile --------------------------------------------------------

    #[tokio::test]
    async fn positive_reconcile_admits_enabled_and_skips_disabled() {
        // desc: two enabled + one disabled row → only the enabled owners are admitted.
        // expect: admitted = the two enabled keys; disabled row never admitted.
        let roster = seeded(&[row("web", true), row("api", true), row("cli", false)]).await;
        let host = FakeHost::new(0);
        let report = reconcile(roster.as_ref(), &host, always_ok().as_ref()).await;
        assert_eq!(report.admitted.len(), 2, "both enabled rows admitted");
        assert!(report.skipped.is_empty(), "nothing skipped");
        let admitted = host.admitted();
        assert!(admitted.iter().any(|k| k.session.as_str() == "web"));
        assert!(admitted.iter().any(|k| k.session.as_str() == "api"));
        assert!(
            !admitted.iter().any(|k| k.session.as_str() == "cli"),
            "disabled row must not be admitted"
        );
        // The owner user is the org (row.user).
        assert!(admitted.iter().all(|k| k.user.as_str() == "acme"));
    }

    #[tokio::test]
    async fn boundary_with_limits_rejects_over_capacity() {
        // desc: cap = 1 but two enabled rows. expect: one admitted, one skipped with a
        // capacity reason (→ the manager would map this to RESOURCE_EXHAUSTED).
        let roster = seeded(&[row("web", true), row("api", true)]).await;
        let host = FakeHost::new(1);
        let report = reconcile(roster.as_ref(), &host, always_ok().as_ref()).await;
        assert_eq!(report.admitted.len(), 1, "cap admits exactly one");
        assert_eq!(report.skipped.len(), 1, "the other is shed");
        assert!(
            report.skipped[0].1.contains("capacity"),
            "skip reason names capacity: {:?}",
            report.skipped[0]
        );
    }

    #[tokio::test]
    async fn corner_reconcile_is_idempotent() {
        // desc: reconcile twice over the same roster. expect: the same live set (a
        // crash-safe rebuild-from-roster), not duplicates.
        let roster = seeded(&[row("web", true), row("api", true)]).await;
        let host = FakeHost::new(0);
        let first = reconcile(roster.as_ref(), &host, always_ok().as_ref()).await;
        let second = reconcile(roster.as_ref(), &host, always_ok().as_ref()).await;
        assert_eq!(first.admitted.len(), 2);
        assert_eq!(second.admitted.len(), 2);
        assert_eq!(host.admitted().len(), 2, "no duplicate admits on re-run");
    }

    #[tokio::test]
    async fn adversarial_unresolvable_token_ref_keeps_session_disabled() {
        // desc: an enabled row whose forge credential can't be resolved. expect: it is
        // skipped (fail closed), never admitted — a broken session must not run.
        let roster = seeded(&[row("web", true)]).await;
        let host = FakeHost::new(0);
        let report = reconcile(roster.as_ref(), &host, always_broken().as_ref()).await;
        assert!(report.admitted.is_empty(), "no session admitted");
        assert_eq!(report.skipped.len(), 1);
        assert!(host.admitted().is_empty(), "fail closed: nothing admitted");
    }

    // ---- FSM (FleetOrchestrator::handle) ----------------------------------

    fn orch(
        roster: Arc<MemoryFleet>,
        repo: Arc<agent_testkit::FixtureRepo>,
        host: Arc<FakeHost>,
    ) -> FleetOrchestrator {
        FleetOrchestrator::new(roster, repo, host)
    }

    #[tokio::test]
    async fn positive_trigger_drives_cloning_then_reviewing() {
        // desc: a trigger for a known row + PR. expect: FSM reaches Reviewing, the PR
        // head is fetched once, a worktree is added, and the review runs on the
        // PR-scoped key (user = org, session = encode_review_session_id(repo, pr)).
        let mut r = row("web", true);
        r.skill = "code-review".into();
        let roster = seeded(&[r]).await;
        let repo = Arc::new(agent_testkit::FixtureRepo::new());
        let host = Arc::new(FakeHost::new(0));
        let o = orch(roster, repo.clone(), host.clone());

        let got = o
            .handle(FleetTrigger {
                session_id: "web".into(),
                pr_number: 42,
            })
            .await
            .expect("handle ok");
        assert!(matches!(got, Handled::Reviewing { .. }));
        assert_eq!(
            repo.fetch_pr_calls(),
            vec![42],
            "PR head fetched exactly once"
        );

        let s = host.state.lock().unwrap();
        assert_eq!(s.reviews.len(), 1, "one review started");
        let (key, goal, skill) = &s.reviews[0];
        assert_eq!(key.user.as_str(), "acme");
        assert_eq!(
            key.session.as_str(),
            encode_review_session_id("acme__web", 42).as_str()
        );
        assert!(goal.contains("#42"), "goal names the PR: {goal}");
        // The roster row's skill is threaded to the host (C11) so the review session
        // is seeded with its checklist; `row("web", …)` carries the default skill.
        assert_eq!(
            skill.as_deref(),
            Some("code-review"),
            "the review skill reaches the host"
        );
    }

    #[tokio::test]
    async fn corner_duplicate_trigger_same_pr_is_noop() {
        // desc: the same (session, PR) triggered twice. expect: the second is a no-op
        // (Duplicate) and does NOT re-fetch — one in-flight review per PR.
        let roster = seeded(&[row("web", true)]).await;
        let repo = Arc::new(agent_testkit::FixtureRepo::new());
        let host = Arc::new(FakeHost::new(0));
        let o = orch(roster, repo.clone(), host.clone());

        let t = || FleetTrigger {
            session_id: "web".into(),
            pr_number: 7,
        };
        let first = o.handle(t()).await.expect("first ok");
        let second = o.handle(t()).await.expect("second ok");
        assert!(matches!(first, Handled::Reviewing { .. }));
        assert!(matches!(second, Handled::Duplicate));
        assert_eq!(
            repo.fetch_pr_calls(),
            vec![7],
            "duplicate must not re-fetch"
        );
        assert_eq!(host.state.lock().unwrap().reviews.len(), 1);
    }

    #[tokio::test]
    async fn negative_unknown_session_id_is_err() {
        // desc: a trigger naming a roster row that does not exist. expect: Err (the
        // seam's `not found`), and nothing fetched or driven — fail closed.
        let roster = seeded(&[row("web", true)]).await;
        let repo = Arc::new(agent_testkit::FixtureRepo::new());
        let host = Arc::new(FakeHost::new(0));
        let o = orch(roster, repo.clone(), host.clone());

        let err = o
            .handle(FleetTrigger {
                session_id: "ghost".into(),
                pr_number: 1,
            })
            .await;
        assert!(err.is_err(), "unknown row must be an error");
        assert!(repo.fetch_pr_calls().is_empty(), "no fetch on unknown row");
        assert!(host.state.lock().unwrap().reviews.is_empty());
    }

    #[tokio::test]
    async fn corner_runhandle_drop_cancels_the_run() {
        // desc: after a review starts, cancelling it drops the RunHandle. expect: the
        // run's cancel flag fires (drop = cancel), and the guard is cleared.
        let roster = seeded(&[row("web", true)]).await;
        let repo = Arc::new(agent_testkit::FixtureRepo::new());
        let host = Arc::new(FakeHost::new(0));
        let o = orch(roster, repo, host.clone());

        o.handle(FleetTrigger {
            session_id: "web".into(),
            pr_number: 9,
        })
        .await
        .expect("handle ok");
        let flag = host.state.lock().unwrap().cancels[0].clone();
        assert!(
            !flag.load(Ordering::SeqCst),
            "not cancelled while in flight"
        );
        assert!(o.is_in_flight("web", 9));

        assert!(o.cancel("web", 9), "cancel removes the in-flight run");
        assert!(
            flag.load(Ordering::SeqCst),
            "dropping the handle cancels the run"
        );
        assert!(!o.is_in_flight("web", 9));
    }

    // ---- bounded, coalescing trigger queue --------------------------------

    #[tokio::test]
    async fn positive_enqueue_then_recv_roundtrips() {
        let (q, mut rx) = TriggerQueue::channel(8);
        let t = FleetTrigger {
            session_id: "web".into(),
            pr_number: 3,
        };
        assert_eq!(q.enqueue(t.clone()), TriggerOutcome::Accepted);
        assert_eq!(rx.recv().await, Some(t));
    }

    #[tokio::test]
    async fn corner_duplicate_pending_coalesces() {
        // desc: the same (session, PR) enqueued twice before it is popped. expect: the
        // second coalesces (folded into the pending one), not a second queue entry.
        let (q, mut rx) = TriggerQueue::channel(8);
        let t = FleetTrigger {
            session_id: "web".into(),
            pr_number: 5,
        };
        assert_eq!(q.enqueue(t.clone()), TriggerOutcome::Accepted);
        assert_eq!(q.enqueue(t.clone()), TriggerOutcome::Coalesced);
        assert_eq!(rx.recv().await, Some(t));
    }

    #[tokio::test]
    async fn boundary_queue_full_coalesces_not_drops() {
        // desc: capacity 1, two *distinct* triggers. expect: the first is Accepted, the
        // second coalesces on overflow (bounded) — never a silent drop or a panic.
        let (q, mut rx) = TriggerQueue::channel(1);
        let a = FleetTrigger {
            session_id: "web".into(),
            pr_number: 1,
        };
        let b = FleetTrigger {
            session_id: "web".into(),
            pr_number: 2,
        };
        assert_eq!(q.enqueue(a.clone()), TriggerOutcome::Accepted);
        assert_eq!(
            q.enqueue(b),
            TriggerOutcome::Coalesced,
            "full queue coalesces"
        );
        assert_eq!(rx.recv().await, Some(a));
    }

    #[tokio::test]
    async fn positive_recv_clears_pending_allowing_requeue() {
        // desc: after a trigger is popped, the same PR can be queued again (its pending
        // mark cleared) — a genuine second round, not a coalesce.
        let (q, mut rx) = TriggerQueue::channel(8);
        let t = FleetTrigger {
            session_id: "web".into(),
            pr_number: 4,
        };
        assert_eq!(q.enqueue(t.clone()), TriggerOutcome::Accepted);
        assert_eq!(rx.recv().await, Some(t.clone()));
        assert_eq!(
            q.enqueue(t.clone()),
            TriggerOutcome::Accepted,
            "requeue after pop is a fresh accept"
        );
        assert_eq!(rx.recv().await, Some(t));
    }
}
