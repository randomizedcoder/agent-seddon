//! The `[campaign]` driver wiring (docs/design/campaigns/04-executor.md, CP-05 /
//! CP-06a): `[campaign]` keys → [`DriverConfig`], the tenant selection rule, and
//! [`build_driver`], which assembles an [`agent_campaign::Driver`] over the
//! backend `crate::campaign::open_campaign_backend` opened.
//!
//! The poller is the process's `[forge]` backend (`Agent::forge`) behind a
//! [`ForgePoller`]; with no forge configured the driver runs the [`NoopPoller`]
//! and warns once, because `in_review` leaves are then never resolved. The
//! shipped driver still has **no worker exec** (`with_exec(None)`): it reaps,
//! polls and plans, and its claim phase is off, so `agent campaign run` reports
//! `claimed 0  dispatched 0` rather than burning attempts on leaves nothing can
//! execute. The worker body and the subprocess dispatch under `[campaign]
//! sandbox` land in CP-06b.

use std::sync::Arc;
use std::time::Duration;

use agent_campaign::{
    Driver, DriverConfig, ForgePoller, NoopPoller, PrPoller, Tenants, TickPlanner,
};
use agent_core::campaign::{CampaignBackend, Policy, DECOMPOSING_MAX_SECS};
use agent_core::{Forge, UserId};

use crate::config::{CampaignCfg, Config};

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

/// The shipped driver: config mapped by [`driver_config`], the poller from
/// `forge` ([`poller_for`]), no exec. `planner` is the CLI's `FactoryPlanner`
/// over the built agent's planner provider; `forge` is `Agent::forge()`.
pub fn build_driver(
    cfg: &CampaignCfg,
    backend: Arc<dyn CampaignBackend>,
    tenants: Tenants,
    planner: Arc<dyn TickPlanner>,
    forge: Option<Arc<dyn Forge>>,
) -> Driver {
    Driver::new(backend, tenants, driver_config(cfg), planner)
        .with_poller(poller_for(forge))
        .with_exec(None)
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
            ..CampaignCfg::default()
        }
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
    // config does not carry are the seam's defaults; no exec is wired.
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
        );
        assert_eq!(*driver.config(), driver_config(&cfg));
        assert_eq!(driver.tenants(), &Tenants::Fixed(vec!["acme".into()]));
        assert!(!driver.has_exec(), "CP-06a ships no worker: claims are off");
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
        );
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
        );
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
        );
        let report = driver.tick().await;
        assert_eq!(report.poll().polled, 1);
        assert_eq!(report.poll().merged, 1);
        assert_eq!(report.errors, 0);
        let s = mem.with_tenant("acme").unwrap();
        assert_eq!(state(&s, r.task_id).await, TaskState::Done);
        assert_eq!(*forge.0.lock().unwrap(), vec![7]);
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
        );
        let report = driver.tick().await;
        assert_eq!(report.poll(), agent_campaign::PollReport::default());
        let s = mem.with_tenant("acme").unwrap();
        assert_eq!(s.get(r.task_id).await.unwrap(), r);
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
