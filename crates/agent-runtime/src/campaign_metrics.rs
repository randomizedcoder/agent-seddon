//! The campaign observability bridge (docs/design/campaigns/04-executor.md
//! §Observability, CP-08): [`MetricsObserver`] is the [`TickObserver`] the shipped
//! driver runs with, turning each tick's report into the `agent_campaign_*`
//! families of [`Metrics`] — the way the scheduler's `RunObserver` bridges
//! `agent-scheduler` to `agent-metrics` without a dependency between them.
//!
//! Everything here is a pure walk over the report the driver already returns:
//!
//! * per tenant — the reapers' counts (`leases_lost`, `plans_released`), the
//!   claims, the poll outcomes, and the plan phase: one `attempts{kind="decompose"}`
//!   per node with the planner's outcome label and model, its tokens, and the
//!   node(s) it left behind (`nodes{kind,state}`; a split's children as
//!   `kind="task", state="created"` — their real states are the policy's, not
//!   read back here). A store failure on a node (`Err`) is `outcome="failure"`,
//!   distinct from the planner's own `error` close;
//! * per harvested worker (and per worker a drain settles) — one
//!   `attempts{kind="work"}` with the driver's outcome word and the attempt's
//!   model, plus the tokens the attempt row recorded (clamped: the store's `i64`s
//!   are untrusted);
//! * the tick itself — `tick_seconds` and `tick_errors_total`.
//!
//! A worker that ran as an `agent --run-task` child is counted here, not in the
//! child (whose registry dies with it): the driver reads the attempt row back when
//! it settles the leaf ([`agent_campaign::Settled`]).

use std::time::Duration;

use agent_campaign::{
    DrainReport, PlanOutcome, Settled, TenantReport, TickObserver, TickReport, WorkerOutcome,
};
use agent_metrics::Metrics;

/// The `attempts{kind}` word for the plan phase.
const KIND_DECOMPOSE: &str = "decompose";
/// The `attempts{kind}` word for a dispatched worker.
const KIND_WORK: &str = "work";
/// The `attempts{outcome}` for a node the store failed on (no planner outcome).
const OUTCOME_FAILURE: &str = "failure";

/// Bridges the driver's tick and drain reports to the campaign metric families.
pub struct MetricsObserver(pub Metrics);

/// A report count as a metric add (a `usize` never exceeds `u64` on the targets
/// this builds for; saturate rather than wrap on one that would).
fn n(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// The driver's outcome word for a settled worker.
fn outcome_word(outcome: WorkerOutcome) -> &'static str {
    match outcome {
        WorkerOutcome::Ok => "ok",
        WorkerOutcome::Error => "error",
        WorkerOutcome::Timeout => "timeout",
        WorkerOutcome::Panic => "panic",
    }
}

impl MetricsObserver {
    fn record_tenant(&self, t: &TenantReport) {
        let m = &self.0;
        let tenant = t.tenant.as_str();
        m.on_campaign_reaped(tenant, n(t.reaped), n(t.released));
        m.on_campaign_claims(tenant, n(t.claimed));
        m.on_campaign_polls(
            tenant,
            n(t.poll.merged),
            n(t.poll.closed),
            n(t.poll.awaiting),
            n(t.poll.errors),
        );
        let Some(plan) = &t.plan else {
            return;
        };
        for (_, planned) in &plan.nodes {
            match planned {
                Ok(p) => {
                    m.on_campaign_attempt(tenant, KIND_DECOMPOSE, p.outcome.label(), &plan.model);
                    let (tokens_in, tokens_out) = p.tokens.clamped();
                    m.add_campaign_tokens(tenant, KIND_DECOMPOSE, tokens_in, tokens_out);
                    match &p.outcome {
                        PlanOutcome::Executed { task, .. }
                        | PlanOutcome::NeedsInfo { task }
                        | PlanOutcome::Rejected { task }
                        | PlanOutcome::Blocked { task, .. }
                        | PlanOutcome::Errored { task, .. } => {
                            m.on_campaign_node(tenant, task.kind.as_str(), task.state.as_str(), 1);
                        }
                        PlanOutcome::Split {
                            parent, children, ..
                        } => {
                            m.on_campaign_node(
                                tenant,
                                parent.kind.as_str(),
                                parent.state.as_str(),
                                1,
                            );
                            m.on_campaign_node(tenant, "task", "created", n(*children));
                        }
                        PlanOutcome::Skipped(_) | PlanOutcome::Conflict => {}
                    }
                }
                Err(_) => {
                    m.on_campaign_attempt(tenant, KIND_DECOMPOSE, OUTCOME_FAILURE, &plan.model);
                }
            }
        }
    }

    fn record_settled(&self, s: &Settled) {
        let m = &self.0;
        m.on_campaign_attempt(&s.tenant, KIND_WORK, outcome_word(s.outcome), &s.model);
        let (tokens_in, tokens_out) = s.tokens.clamped();
        m.add_campaign_tokens(&s.tenant, KIND_WORK, tokens_in, tokens_out);
    }
}

impl TickObserver for MetricsObserver {
    fn on_tick(&self, report: &TickReport, elapsed: Duration) {
        for t in &report.per_tenant {
            self.record_tenant(t);
        }
        for s in &report.harvested {
            self.record_settled(s);
        }
        self.0
            .on_campaign_tick(elapsed.as_secs_f64(), n(report.errors));
    }

    fn on_drain(&self, report: &DrainReport) {
        for s in &report.settled {
            self.record_settled(s);
        }
    }
}

/// T17, the metrics half (docs/design/campaigns/06-test-matrix.md): the bridge
/// over hand-built reports, and over the real driver for the tick / disabled rows.
#[cfg(test)]
mod tests {
    use super::*;
    use agent_campaign::{
        ClosureExec, Driver, DriverConfig, PlanReport, Planned, PollReport, Tenants, TickPlanner,
    };
    use agent_core::campaign::{CampaignBackend, CampaignStore, TaskId, TokenUsage};
    use agent_testkit::campaign::conformance::{campaign, leaf, ready_leaves, split};
    use agent_testkit::campaign::MemCampaigns;
    use agent_testkit::observe::MetricsProbe;
    use async_trait::async_trait;
    use rstest::rstest;
    use std::sync::Arc;

    /// The value of the one exposition line for `family` whose labels include
    /// every `(k, v)` in `wants` (label order in the text is the crate's, not ours).
    fn sample(m: &Metrics, family: &str, wants: &[(&str, &str)]) -> Option<f64> {
        m.encode_text().lines().find_map(|line| {
            let rest = line.strip_prefix(family)?;
            let (labels, value) = match rest.strip_prefix('{') {
                Some(r) => r.split_once('}')?,
                None if wants.is_empty() => ("", rest),
                None => return None,
            };
            if !wants
                .iter()
                .all(|(k, v)| labels.contains(&format!("{k}=\"{v}\"")))
            {
                return None;
            }
            value.trim().parse::<f64>().ok()
        })
    }

    fn observer() -> (Metrics, MetricsObserver) {
        let m = Metrics::new();
        (m.clone(), MetricsObserver(m))
    }

    fn tenant_report(tenant: &str) -> TenantReport {
        TenantReport {
            tenant: tenant.to_string(),
            ..TenantReport::default()
        }
    }

    fn planned(outcome: PlanOutcome, tokens_in: i64, tokens_out: i64) -> Planned {
        Planned {
            task: TaskId(1),
            outcome,
            calls: 1,
            repairs: 0,
            tokens: TokenUsage::new(tokens_in, tokens_out),
            prompt_hash: None,
        }
    }

    fn settled(tenant: &str, outcome: WorkerOutcome, tokens: TokenUsage, model: &str) -> Settled {
        Settled {
            tenant: tenant.to_string(),
            task: TaskId(1),
            outcome,
            tokens,
            model: model.to_string(),
        }
    }

    /// A planner that plans nothing (the real tick rows only need the claim phase).
    struct IdlePlanner;

    #[async_trait]
    impl TickPlanner for IdlePlanner {
        async fn tick(&self, _store: Arc<dyn CampaignStore>, _limit: usize) -> PlanReport {
            PlanReport::default()
        }
    }

    fn driver(mem: &MemCampaigns, enabled: bool, m: &Metrics) -> Driver {
        let backend: Arc<dyn CampaignBackend> = Arc::new(mem.clone());
        Driver::new(
            backend,
            Tenants::Fixed(vec!["ta".into()]),
            DriverConfig {
                enabled,
                ..DriverConfig::default()
            },
            Arc::new(IdlePlanner),
        )
        .with_exec(Some(Arc::new(ClosureExec(|_, _, _, _| async { Ok(()) }))))
        .with_observer(Arc::new(MetricsObserver(m.clone())))
    }

    // desc: one real tick over the memory tier with a claimable leaf moves the
    // claims counter and times the tick; the drain then records the settled worker.
    #[tokio::test]
    async fn positive_tick_claims_and_seconds() {
        let mem = MemCampaigns::new();
        let s = mem.with_tenant("ta").unwrap();
        ready_leaves(&s, 1).await;
        let m = Metrics::new();
        let probe = MetricsProbe::new(&m);
        let d = driver(&mem, true, &m);
        let report = d.tick().await;
        assert_eq!(report.claimed(), 1);
        assert_eq!(
            probe.delta(&m, "agent_campaign_claims_total", Some("tenant=\"ta\"")),
            1.0
        );
        assert_eq!(
            probe.delta(&m, "agent_campaign_tick_seconds_count", None),
            1.0
        );
        d.drain(Duration::from_secs(5)).await;
        assert_eq!(
            sample(
                &m,
                "agent_campaign_attempts_total",
                &[("tenant", "ta"), ("kind", "work"), ("outcome", "ok")]
            ),
            Some(1.0)
        );
    }

    // desc (corner): a disabled driver runs nothing, so nothing is recorded.
    #[tokio::test]
    async fn corner_disabled_tick_records_nothing() {
        let mem = MemCampaigns::new();
        let s = mem.with_tenant("ta").unwrap();
        ready_leaves(&s, 1).await;
        let m = Metrics::new();
        let probe = MetricsProbe::new(&m);
        let d = driver(&mem, false, &m);
        assert!(d.tick().await.disabled);
        assert_eq!(
            probe.delta(&m, "agent_campaign_tick_seconds_count", None),
            0.0
        );
        assert!(!m.encode_text().contains("tenant=\"ta\""));
    }

    // desc: every planner outcome becomes one decompose attempt under the
    // planner's model, its tokens, and the node(s) it left behind; a store
    // failure on a node is `outcome="failure"`.
    #[tokio::test]
    async fn positive_plan_outcomes_nodes_and_attempts() {
        let mem = MemCampaigns::new();
        let s = mem.with_tenant("ta").unwrap();
        let root = campaign(&s, "c").await;
        let d = split(&s, root.task_id, 2).await;
        let l = leaf(&s, d.children[0].task_id).await;
        let parent = s.get(root.task_id).await.unwrap();
        let (m, obs) = observer();
        let mut t = tenant_report("ta");
        t.plan = Some(PlanReport {
            nodes: vec![
                (
                    parent.clone(),
                    Ok(planned(
                        PlanOutcome::Split {
                            parent: parent.clone(),
                            children: 2,
                            low_confidence: false,
                        },
                        100,
                        20,
                    )),
                ),
                (
                    l.clone(),
                    Ok(planned(
                        PlanOutcome::Executed {
                            task: l.clone(),
                            low_confidence: false,
                        },
                        50,
                        10,
                    )),
                ),
                (
                    l.clone(),
                    Err(agent_core::campaign::CampaignError::Backend("db".into())),
                ),
            ],
            model: "kimi-k3".into(),
            ..PlanReport::default()
        });
        obs.on_tick(
            &TickReport {
                per_tenant: vec![t],
                ..TickReport::default()
            },
            Duration::from_millis(5),
        );
        let ta = ("tenant", "ta");
        let dec = ("kind", "decompose");
        assert_eq!(
            sample(
                &m,
                "agent_campaign_nodes_total",
                &[ta, ("kind", "objective"), ("state", "decomposed")]
            ),
            Some(1.0)
        );
        assert_eq!(
            sample(
                &m,
                "agent_campaign_nodes_total",
                &[ta, ("kind", "task"), ("state", "created")]
            ),
            Some(2.0)
        );
        assert_eq!(
            sample(
                &m,
                "agent_campaign_nodes_total",
                &[ta, ("kind", "leaf"), ("state", "ready")]
            ),
            Some(1.0)
        );
        for outcome in ["split", "execute", "failure"] {
            assert_eq!(
                sample(
                    &m,
                    "agent_campaign_attempts_total",
                    &[ta, dec, ("outcome", outcome), ("model", "kimi-k3")]
                ),
                Some(1.0),
                "{outcome}"
            );
        }
        assert_eq!(
            sample(
                &m,
                "agent_campaign_tokens_total",
                &[ta, dec, ("direction", "in")]
            ),
            Some(150.0)
        );
        assert_eq!(
            sample(
                &m,
                "agent_campaign_tokens_total",
                &[ta, dec, ("direction", "out")]
            ),
            Some(30.0)
        );
        assert_eq!(
            sample(&m, "agent_campaign_tick_errors_total", &[]),
            Some(0.0)
        );
    }

    // desc: a harvested worker is one work attempt under its outcome and model,
    // with the tokens its attempt row recorded.
    #[test]
    fn positive_work_settled_tokens_and_model() {
        let (m, obs) = observer();
        obs.on_tick(
            &TickReport {
                harvested: vec![settled(
                    "ta",
                    WorkerOutcome::Ok,
                    TokenUsage::new(60, 40),
                    "kimi-k3",
                )],
                ..TickReport::default()
            },
            Duration::from_millis(1),
        );
        let ta = ("tenant", "ta");
        assert_eq!(
            sample(
                &m,
                "agent_campaign_attempts_total",
                &[
                    ta,
                    ("kind", "work"),
                    ("outcome", "ok"),
                    ("model", "kimi-k3")
                ]
            ),
            Some(1.0)
        );
        assert_eq!(
            sample(
                &m,
                "agent_campaign_tokens_total",
                &[ta, ("kind", "work"), ("direction", "in")]
            ),
            Some(60.0)
        );
        assert_eq!(
            sample(
                &m,
                "agent_campaign_tokens_total",
                &[ta, ("kind", "work"), ("direction", "out")]
            ),
            Some(40.0)
        );
    }

    // desc: the reapers' counts and the poll outcomes land in their families.
    #[test]
    fn positive_reap_and_polls() {
        let (m, obs) = observer();
        let mut t = tenant_report("ta");
        t.reaped = 2;
        t.released = 1;
        t.poll = PollReport {
            polled: 4,
            merged: 1,
            closed: 1,
            awaiting: 1,
            errors: 1,
        };
        obs.on_tick(
            &TickReport {
                per_tenant: vec![t],
                ..TickReport::default()
            },
            Duration::from_millis(1),
        );
        let ta = ("tenant", "ta");
        assert_eq!(
            sample(&m, "agent_campaign_leases_lost_total", &[ta]),
            Some(2.0)
        );
        assert_eq!(
            sample(&m, "agent_campaign_plans_released_total", &[ta]),
            Some(1.0)
        );
        for outcome in ["merged", "closed", "awaiting", "error"] {
            assert_eq!(
                sample(
                    &m,
                    "agent_campaign_polls_total",
                    &[ta, ("outcome", outcome)]
                ),
                Some(1.0),
                "{outcome}"
            );
        }
    }

    // desc: a worker the drain settles (after the last tick) is still recorded.
    #[test]
    fn positive_drain_settled_recorded() {
        let (m, obs) = observer();
        obs.on_drain(&DrainReport {
            settled: vec![settled(
                "ta",
                WorkerOutcome::Timeout,
                TokenUsage::default(),
                "",
            )],
            aborted: 0,
        });
        assert_eq!(
            sample(
                &m,
                "agent_campaign_attempts_total",
                &[
                    ("tenant", "ta"),
                    ("kind", "work"),
                    ("outcome", "timeout"),
                    ("model", "unknown")
                ]
            ),
            Some(1.0)
        );
        assert!(
            sample(&m, "agent_campaign_tokens_total", &[("tenant", "ta")]).is_none(),
            "zero tokens mint no series"
        );
    }

    // desc (boundary): the tick's error count adds to the health counter.
    #[test]
    fn boundary_tick_errors_counter() {
        let (m, obs) = observer();
        let probe = MetricsProbe::new(&m);
        obs.on_tick(
            &TickReport {
                errors: 3,
                ..TickReport::default()
            },
            Duration::from_millis(1),
        );
        assert_eq!(
            probe.delta(&m, "agent_campaign_tick_errors_total", None),
            3.0
        );
    }

    // desc (adversarial): negative token counts from the store never reach
    // `inc_by` — clamped to zero, no panic, no series.
    #[test]
    fn adversarial_tokens_negative_clamped() {
        let (m, obs) = observer();
        obs.on_tick(
            &TickReport {
                harvested: vec![settled(
                    "ta",
                    WorkerOutcome::Error,
                    TokenUsage::new(-5, -7),
                    "m",
                )],
                ..TickReport::default()
            },
            Duration::from_millis(1),
        );
        assert!(sample(&m, "agent_campaign_tokens_total", &[("tenant", "ta")]).is_none());
        assert_eq!(
            sample(
                &m,
                "agent_campaign_attempts_total",
                &[("tenant", "ta"), ("outcome", "error")]
            ),
            Some(1.0)
        );
    }

    // desc (adversarial): a tenant string the recorder refuses mints no series
    // anywhere, whichever phase reported it.
    #[rstest]
    #[case::adversarial_traversal("../x")]
    #[case::adversarial_separator("a/b")]
    #[case::boundary_empty("")]
    fn adversarial_tenant_unsafe_no_series(#[case] tenant: &str) {
        let (m, obs) = observer();
        let mut t = tenant_report(tenant);
        t.reaped = 1;
        t.claimed = 1;
        t.poll.merged = 1;
        obs.on_tick(
            &TickReport {
                per_tenant: vec![t],
                harvested: vec![settled(
                    tenant,
                    WorkerOutcome::Ok,
                    TokenUsage::new(1, 1),
                    "m",
                )],
                ..TickReport::default()
            },
            Duration::from_millis(1),
        );
        let text = m.encode_text();
        assert!(
            !text
                .lines()
                .any(|l| l.starts_with("agent_campaign_") && l.contains("tenant=")),
            "{text}"
        );
        assert_eq!(
            sample(&m, "agent_campaign_tick_seconds_count", &[]),
            Some(1.0)
        );
    }

    // desc (adversarial): a model label with control characters or a runaway
    // length folds to `other` (the planner's and the worker's alike).
    #[test]
    fn adversarial_model_label_folded() {
        let (m, obs) = observer();
        let hostile = format!("x\u{1b}[31m{}", "m".repeat(300));
        let mut t = tenant_report("ta");
        t.plan = Some(PlanReport {
            nodes: vec![(
                agent_core::campaign::Task { ..dummy_task() },
                Err(agent_core::campaign::CampaignError::Backend("db".into())),
            )],
            model: hostile.clone(),
            ..PlanReport::default()
        });
        obs.on_tick(
            &TickReport {
                per_tenant: vec![t],
                harvested: vec![settled(
                    "ta",
                    WorkerOutcome::Ok,
                    TokenUsage::default(),
                    &hostile,
                )],
                ..TickReport::default()
            },
            Duration::from_millis(1),
        );
        let text = m.encode_text();
        assert!(!text.contains('\u{1b}'));
        for kind in ["decompose", "work"] {
            assert_eq!(
                sample(
                    &m,
                    "agent_campaign_attempts_total",
                    &[("tenant", "ta"), ("kind", kind), ("model", "other")]
                ),
                Some(1.0),
                "{kind}"
            );
        }
    }

    /// A task value for a report row whose fields the bridge never reads.
    fn dummy_task() -> agent_core::campaign::Task {
        serde_json::from_str(
            r#"{"task_id":1,"campaign_id":1,"repo_id":1,"parent_id":null,"path":"1","depth":0,"ordinal":1,"kind":"objective","state":"ready","title":"t","goal":"g","acceptance":[],"touches":[],"depends_on":[],"est_size":null,"source_ref":null,"policy":null,"version":1,"attempts":0,"claimed_by":null,"lease_until_ms":null,"pr_number":null,"pr_url":null,"branch":null,"superseded_by":null,"created_by":"u","created_at_ms":0,"updated_at_ms":0}"#,
        )
        .expect("a valid task")
    }
}
