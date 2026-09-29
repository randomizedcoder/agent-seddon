//! The `[campaign]` driver wiring (docs/design/campaigns/04-executor.md, CP-05 /
//! CP-06a): `[campaign]` keys → [`DriverConfig`], the tenant selection rule, and
//! [`build_driver`], which assembles an [`agent_campaign::Driver`] over the
//! backend `crate::campaign::open_campaign_backend` opened.
//!
//! The poller is the process's `[forge]` backend (`Agent::forge`) behind a
//! [`ForgePoller`]; with no forge configured the driver runs the [`NoopPoller`]
//! and warns once, because `in_review` leaves are then never resolved. The worker
//! exec follows `[campaign] sandbox` (CP-06b): `"subprocess"` dispatches each leaf
//! as an `agent --run-task` child under the process `[sandbox]` backend
//! ([`SubprocessExec`]) — with no sandbox wired the driver **refuses to build**
//! naming the key rather than fall back to running the leaf in this process;
//! `"in_process"` runs [`run_leaf`](crate::campaign_worker::run_leaf) here
//! ([`InProcessExec`]). `run --once` follows the same rule (CP-05 decision 7: one
//! code path).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agent_campaign::{
    Driver, DriverConfig, ForgePoller, NoopPoller, PrPoller, Tenants, TickPlanner, WorkerExec,
};
use agent_core::campaign::{CampaignBackend, Policy, DECOMPOSING_MAX_SECS};
use agent_core::{Forge, UserId};

use crate::agent::Agent;
use crate::campaign_metrics::MetricsObserver;
use crate::campaign_worker::{InProcessExec, SubprocessExec, WorkerCfg};
use crate::config::{CampaignCfg, Config};

/// What the worker exec needs from the process: the built agent (its sandbox,
/// repo, forge, policy and worker provider), this binary and the config path the
/// subprocess re-reads, and the worker knobs.
pub struct WorkerDeps {
    pub agent: Arc<Agent>,
    /// `std::env::current_exe()`; `None` refuses the subprocess sandbox.
    pub agent_bin: Option<PathBuf>,
    /// `Config::source_path` (the `--config` given); `None` refuses the subprocess
    /// sandbox.
    pub config_path: Option<PathBuf>,
    pub worker: WorkerCfg,
}

/// The driver knobs from the validated `[campaign]` block. The lease is the
/// policy default (a claim spans campaigns; the per-campaign lease is the CP-06b
/// heartbeat's), the `decomposing` bound the seam constant.
pub fn driver_config(cfg: &CampaignCfg) -> DriverConfig {
    DriverConfig {
        enabled: cfg.enabled,
        per_tenant_workers: cfg.per_tenant_workers,
        global_workers: cfg.global_workers,
        plan_per_tick: cfg.plan_per_tick,
        worker_timeout: Duration::from_secs(cfg.worker_timeout_secs),
        lease_secs: Policy::default().lease_secs,
        decomposing_max_secs: DECOMPOSING_MAX_SECS,
        poll_batch: cfg.poll_batch,
    }
}

/// Which tenants a driver serves: `--tenant T` names one; otherwise every tenant
/// with live work under `[tenancy] per_tenant` (mirroring the scheduler driver),
/// else the `local` tenant a single-tenant install writes to.
pub fn tenants_for(cfg: &Config, cli_tenant: Option<&str>) -> Tenants {
    match cli_tenant {
        Some(t) => Tenants::Fixed(vec![t.to_string()]),
        None if cfg.tenancy.per_tenant => Tenants::Discover,
        None => Tenants::Fixed(vec![UserId::LOCAL.to_string()]),
    }
}

/// The poller for `forge`: the [`ForgePoller`] when a `[forge]` backend is
/// wired, else the [`NoopPoller`] with one warning.
fn poller_for(forge: Option<Arc<dyn Forge>>) -> Arc<dyn PrPoller> {
    match forge {
        Some(f) => Arc::new(ForgePoller::new(f)),
        None => {
            tracing::warn!(
                "campaign: no [forge] backend configured; leaves in review are never resolved"
            );
            Arc::new(NoopPoller)
        }
    }
}

/// The worker exec for `[campaign] sandbox`. Fails closed: `"subprocess"` with
/// no `[sandbox] backend`, no binary path or no config path is an error naming
/// what is missing — never a silent in-process fallback.
fn exec_for(cfg: &CampaignCfg, deps: WorkerDeps) -> anyhow::Result<Arc<dyn WorkerExec>> {
    if cfg.sandbox == "in_process" {
        return Ok(Arc::new(InProcessExec::new(deps.agent, deps.worker)));
    }
    let sandbox = deps.agent.sandbox().ok_or_else(|| {
        anyhow::anyhow!(
            "[campaign] sandbox = \"subprocess\" needs a `[sandbox] backend`, and none is \
             configured (set one, or `[campaign] sandbox = \"in_process\"`)"
        )
    })?;
    let agent_bin = deps.agent_bin.ok_or_else(|| {
        anyhow::anyhow!(
            "[campaign] sandbox = \"subprocess\": this binary's path is unknown, so no \
             worker child can be spawned"
        )
    })?;
    let config_path = deps.config_path.ok_or_else(|| {
        anyhow::anyhow!(
            "[campaign] sandbox = \"subprocess\": the config path is unknown (`--config`), \
             so no worker child can re-read it"
        )
    })?;
    Ok(Arc::new(SubprocessExec::new(
        sandbox,
        agent_bin,
        config_path,
        deps.worker.worker_timeout,
    )))
}

/// The shipped driver: config mapped by [`driver_config`], the poller from
/// `forge` ([`poller_for`]), the exec from `[campaign] sandbox` over `deps`.
/// `planner` is the CLI's `FactoryPlanner` over the built agent's planner
/// provider; `forge` is `Agent::forge()`.
pub fn build_driver(
    cfg: &CampaignCfg,
    backend: Arc<dyn CampaignBackend>,
    tenants: Tenants,
    planner: Arc<dyn TickPlanner>,
    forge: Option<Arc<dyn Forge>>,
    deps: WorkerDeps,
) -> anyhow::Result<Driver> {
    let metrics = deps.agent.metrics();
    let exec = exec_for(cfg, deps)?;
    Ok(Driver::new(backend, tenants, driver_config(cfg), planner)
        .with_poller(poller_for(forge))
        .with_exec(Some(exec))
        .with_observer(Arc::new(MetricsObserver(metrics))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_campaign::PlanReport;
    use agent_core::campaign::{CampaignStore, TaskState};
    use agent_core::{Comment, CreatePrRequest, Issue, Page, PullRequest, ReviewVerdict};
    use agent_testkit::campaign::conformance::{
        campaign_with, children, in_review, leaf, owner, split_with, state,
    };
    use agent_testkit::campaign::MemCampaigns;
    use async_trait::async_trait;
    use rstest::rstest;
    use std::sync::Mutex;

    /// A planner that plans nothing.
    struct IdlePlanner;

    #[async_trait]
    impl TickPlanner for IdlePlanner {
        async fn tick(&self, _store: Arc<dyn CampaignStore>, _limit: usize) -> PlanReport {
            PlanReport::default()
        }
    }

    /// A forge whose every PR is merged; records the numbers asked for.
    #[derive(Default)]
    struct MergedForge(Mutex<Vec<u64>>);

    #[async_trait]
    impl Forge for MergedForge {
        fn name(&self) -> &str {
            "merged"
        }
        async fn get_pr(&self, number: u64) -> agent_core::Result<PullRequest> {
            self.0.lock().unwrap().push(number);
            Ok(PullRequest {
                number,
                title: String::new(),
                body: String::new(),
                state: "merged".into(),
                author: String::new(),
                url: format!("https://github.com/org/repo/pull/{number}"),
                source_branch: String::new(),
                target_branch: String::new(),
                draft: false,
            })
        }
        async fn list_prs(&self, _page: u32) -> agent_core::Result<Page<PullRequest>> {
            unimplemented!()
        }
        async fn list_issues(&self, _page: u32) -> agent_core::Result<Page<Issue>> {
            unimplemented!()
        }
        async fn import_issue(&self, _number: u64) -> agent_core::Result<Issue> {
            unimplemented!()
        }
        async fn create_pr(&self, _req: &CreatePrRequest) -> agent_core::Result<PullRequest> {
            unimplemented!()
        }
        async fn comment(&self, _number: u64, _body: &str) -> agent_core::Result<Comment> {
            unimplemented!()
        }
        async fn review_pr(
            &self,
            _number: u64,
            _verdict: ReviewVerdict,
            _body: &str,
        ) -> agent_core::Result<Comment> {
            unimplemented!()
        }
    }

    fn campaign_cfg() -> CampaignCfg {
        CampaignCfg {
            store: "postgres".into(),
            enabled: true,
            tick_secs: 7,
            per_tenant_workers: 3,
            global_workers: 9,
            plan_per_tick: 2,
            worker_timeout_secs: 120,
            poll_batch: 5,
            sandbox: "in_process".into(),
            ..CampaignCfg::default()
        }
    }

    /// A bare agent (no sandbox, no repo, no forge) over a one-turn provider.
    fn bare_agent() -> Arc<Agent> {
        Arc::new(crate::campaign_worker::testing::bare_agent(
            Arc::new(agent_testkit::ScriptedProvider::new(vec![
                agent_testkit::final_turn("ok"),
            ])),
            Arc::new(crate::policy::AutoApprove),
        ))
    }

    /// Worker deps over `agent` with a stub binary and config path.
    fn deps_for(agent: Arc<Agent>) -> WorkerDeps {
        WorkerDeps {
            agent,
            agent_bin: Some(PathBuf::from("/nonexistent/agent")),
            config_path: Some(PathBuf::from("/nonexistent/agent.toml")),
            worker: WorkerCfg {
                worker_timeout: Duration::from_secs(120),
                forge_dry_run: false,
                push_policy: "branch".into(),
                target_branch: "main".into(),
            },
        }
    }

    fn deps() -> WorkerDeps {
        deps_for(bare_agent())
    }

    /// One `in_review` leaf on PR 7 under tenant `acme`, no approval needed.
    async fn seed_in_review(mem: &MemCampaigns) -> agent_core::campaign::Task {
        let s = mem.with_tenant("acme").unwrap();
        let root = campaign_with(
            &s,
            Policy {
                approve_levels: vec![],
                require_pr_approval: false,
                ..Policy::default()
            },
        )
        .await;
        let d = split_with(&s, root.task_id, children(1), 9_001).await;
        let l = leaf(&s, d.children[0].task_id).await;
        in_review(&s, l.task_id, &owner("w1"), 7).await
    }

    // positive: every driver knob comes from its `[campaign]` key; the two the
    // config does not carry are the seam's defaults; the exec is wired.
    #[test]
    fn positive_build_driver_from_cfg() {
        let cfg = campaign_cfg();
        assert_eq!(
            driver_config(&cfg),
            DriverConfig {
                enabled: true,
                per_tenant_workers: 3,
                global_workers: 9,
                plan_per_tick: 2,
                worker_timeout: Duration::from_secs(120),
                lease_secs: Policy::default().lease_secs,
                decomposing_max_secs: DECOMPOSING_MAX_SECS,
                poll_batch: 5,
            }
        );
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let driver = build_driver(
            &cfg,
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            None,
            deps(),
        )
        .unwrap();
        assert_eq!(*driver.config(), driver_config(&cfg));
        assert_eq!(driver.tenants(), &Tenants::Fixed(vec!["acme".into()]));
        assert!(driver.has_exec(), "CP-06b wires the worker: claims are on");
        assert_eq!(driver.global_available(), 9);
        assert_eq!(driver.owner().as_str().len(), 32);
    }

    // positive: the config default for `poll_batch` is the driver's constant.
    #[test]
    fn positive_poll_batch_default_matches_driver() {
        assert_eq!(
            CampaignCfg::default().poll_batch,
            agent_campaign::POLL_BATCH
        );
        assert_eq!(
            driver_config(&CampaignCfg::default()).poll_batch,
            DriverConfig::default().poll_batch
        );
    }

    // corner: `enabled = false` builds a driver whose tick is a no-op.
    #[tokio::test]
    async fn corner_build_driver_disabled() {
        let cfg = CampaignCfg {
            enabled: false,
            ..campaign_cfg()
        };
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let driver = build_driver(
            &cfg,
            backend,
            Tenants::Discover,
            Arc::new(IdlePlanner),
            None,
            deps(),
        )
        .unwrap();
        let report = driver.tick().await;
        assert!(report.disabled);
        assert!(report.tenants.is_empty());
    }

    // positive: a driver over the memory tier ticks the fixed tenant and plans
    // nothing with the idle planner (the runtime wiring reaches the seam).
    #[tokio::test]
    async fn positive_built_driver_ticks() {
        let cfg = campaign_cfg();
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let driver = build_driver(
            &cfg,
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            None,
            deps(),
        )
        .unwrap();
        let report = driver.tick().await;
        assert!(!report.disabled);
        assert_eq!(report.tenants, ["acme"]);
        assert_eq!(report.claimed(), 0);
        assert_eq!(report.errors, 0);
    }

    // positive: with a forge the tick runs the `ForgePoller` — an `in_review`
    // leaf whose PR is merged is `done` after one tick, under `poll_batch`.
    #[tokio::test]
    async fn positive_build_driver_with_forge_polls() {
        let mem = MemCampaigns::new();
        let r = seed_in_review(&mem).await;
        let forge = Arc::new(MergedForge::default());
        let backend: Arc<dyn CampaignBackend> = Arc::new(mem.clone());
        let driver = build_driver(
            &campaign_cfg(),
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            Some(Arc::clone(&forge) as Arc<dyn Forge>),
            deps(),
        )
        .unwrap();
        let report = driver.tick().await;
        assert_eq!(report.poll().polled, 1);
        assert_eq!(report.poll().merged, 1);
        assert_eq!(report.errors, 0);
        let s = mem.with_tenant("acme").unwrap();
        assert_eq!(state(&s, r.task_id).await, TaskState::Done);
        assert_eq!(*forge.0.lock().unwrap(), vec![7]);
    }

    // positive (T17): the shipped driver reports to the agent's metrics — one
    // tick over a claimable leaf moves the claims counter and times the tick.
    #[tokio::test]
    async fn positive_build_driver_wires_metrics_observer() {
        let mem = MemCampaigns::new();
        let s = mem.with_tenant("acme").unwrap();
        agent_testkit::campaign::conformance::ready_leaves(&s, 1).await;
        let agent = bare_agent();
        let metrics = agent.metrics();
        let probe = agent_testkit::observe::MetricsProbe::new(&metrics);
        let backend: Arc<dyn CampaignBackend> = Arc::new(mem.clone());
        let driver = build_driver(
            &campaign_cfg(),
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            None,
            deps_for(agent),
        )
        .unwrap();
        let report = driver.tick().await;
        assert_eq!(report.claimed(), 1);
        assert_eq!(
            probe.delta(
                &metrics,
                "agent_campaign_claims_total",
                Some("tenant=\"acme\"")
            ),
            1.0
        );
        assert_eq!(
            probe.delta(&metrics, "agent_campaign_tick_seconds_count", None),
            1.0
        );
        driver.drain(Duration::from_secs(5)).await;
        assert!(
            probe.delta(
                &metrics,
                "agent_campaign_attempts_total",
                Some("kind=\"work\"")
            ) >= 1.0
        );
    }

    // corner: without a forge the poller is the noop — the leaf stays
    // `in_review` and the poll counts are zero (the warning is logged once).
    #[tokio::test]
    async fn corner_build_driver_without_forge_noop() {
        let mem = MemCampaigns::new();
        let r = seed_in_review(&mem).await;
        let backend: Arc<dyn CampaignBackend> = Arc::new(mem.clone());
        let driver = build_driver(
            &campaign_cfg(),
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            None,
            deps(),
        )
        .unwrap();
        let report = driver.tick().await;
        assert_eq!(report.poll(), agent_campaign::PollReport::default());
        let s = mem.with_tenant("acme").unwrap();
        assert_eq!(s.get(r.task_id).await.unwrap(), r);
    }

    // positive: `sandbox = "subprocess"` over an agent with a sandbox builds the
    // subprocess exec; the claim phase is on.
    #[test]
    fn positive_build_driver_subprocess_has_exec() {
        let cfg = CampaignCfg {
            sandbox: "subprocess".into(),
            ..campaign_cfg()
        };
        let agent = Arc::new(
            crate::campaign_worker::testing::bare_agent(
                Arc::new(agent_testkit::ScriptedProvider::new(vec![
                    agent_testkit::final_turn("ok"),
                ])),
                Arc::new(crate::policy::AutoApprove),
            )
            .with_sandbox(Some(Arc::new(agent_sandbox::LocalSandbox))),
        );
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let driver = build_driver(
            &cfg,
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            None,
            deps_for(agent),
        )
        .unwrap();
        assert!(driver.has_exec());
    }

    // positive: `sandbox = "in_process"` needs no sandbox backend.
    #[test]
    fn positive_build_driver_in_process_has_exec() {
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let driver = build_driver(
            &campaign_cfg(),
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            None,
            deps(),
        )
        .unwrap();
        assert!(driver.has_exec());
    }

    // negative: `subprocess` without a `[sandbox] backend` / binary / config path
    // refuses to build, naming what is missing — never an in-process fallback.
    #[rstest]
    #[case::negative_subprocess_without_sandbox(false, true, true, "[sandbox] backend")]
    #[case::negative_subprocess_without_binary(true, false, true, "binary's path is unknown")]
    #[case::negative_subprocess_without_config_path(true, true, false, "config path is unknown")]
    fn build_driver_subprocess_refusal_rows(
        #[case] sandbox: bool,
        #[case] bin: bool,
        #[case] config_path: bool,
        #[case] needle: &str,
    ) {
        let cfg = CampaignCfg {
            sandbox: "subprocess".into(),
            ..campaign_cfg()
        };
        let mut agent = crate::campaign_worker::testing::bare_agent(
            Arc::new(agent_testkit::ScriptedProvider::new(vec![
                agent_testkit::final_turn("ok"),
            ])),
            Arc::new(crate::policy::AutoApprove),
        );
        if sandbox {
            agent = agent.with_sandbox(Some(Arc::new(agent_sandbox::LocalSandbox)));
        }
        let mut d = deps_for(Arc::new(agent));
        if !bin {
            d.agent_bin = None;
        }
        if !config_path {
            d.config_path = None;
        }
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let err = build_driver(
            &cfg,
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
            None,
            d,
        )
        .expect_err("refused");
        let msg = err.to_string();
        assert!(msg.contains(needle), "{msg}");
        assert!(msg.contains("[campaign] sandbox"), "{msg}");
    }

    #[rstest]
    #[case::positive_cli_tenant_wins(true, Some("acme"), Tenants::Fixed(vec!["acme".into()]))]
    #[case::positive_cli_tenant_single(false, Some("acme"), Tenants::Fixed(vec!["acme".into()]))]
    #[case::positive_per_tenant_discovers(true, None, Tenants::Discover)]
    #[case::corner_single_tenant_local(false, None, Tenants::Fixed(vec!["local".into()]))]
    fn tenants_for_rows(
        #[case] per_tenant: bool,
        #[case] cli: Option<&str>,
        #[case] want: Tenants,
    ) {
        let mut cfg = Config::minimal_for_test();
        cfg.tenancy.per_tenant = per_tenant;
        assert_eq!(tenants_for(&cfg, cli), want);
    }
}
