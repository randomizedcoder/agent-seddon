//! The `[campaign]` driver wiring (docs/design/campaigns/04-executor.md, CP-05):
//! `[campaign]` keys → [`DriverConfig`], the tenant selection rule, and
//! [`build_driver`], which assembles an [`agent_campaign::Driver`] over the
//! backend `crate::campaign::open_campaign_backend` opened.
//!
//! The shipped CP-05 driver has **no worker exec** (`with_exec(None)`): it reaps,
//! polls (a [`NoopPoller`] until CP-06's forge poller) and plans, and its claim
//! phase is off, so `agent campaign run` reports `claimed 0  dispatched 0` rather
//! than burning attempts on leaves nothing can execute. The worker body, the
//! subprocess dispatch under `[campaign] sandbox` and the poller land in CP-06.

use std::sync::Arc;
use std::time::Duration;

use agent_campaign::{Driver, DriverConfig, NoopPoller, Tenants, TickPlanner};
use agent_core::campaign::{CampaignBackend, Policy, DECOMPOSING_MAX_SECS};
use agent_core::UserId;

use crate::config::{CampaignCfg, Config};

/// The driver knobs from the validated `[campaign]` block. The lease is the
/// policy default (a claim spans campaigns; the per-campaign lease is the CP-06
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

/// The shipped driver: config mapped by [`driver_config`], the [`NoopPoller`], no
/// exec. `planner` is the CLI's `FactoryPlanner` over the built agent's planner
/// provider.
pub fn build_driver(
    cfg: &CampaignCfg,
    backend: Arc<dyn CampaignBackend>,
    tenants: Tenants,
    planner: Arc<dyn TickPlanner>,
) -> Driver {
    Driver::new(backend, tenants, driver_config(cfg), planner)
        .with_poller(Arc::new(NoopPoller))
        .with_exec(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_campaign::PlanReport;
    use agent_core::campaign::CampaignStore;
    use agent_testkit::campaign::MemCampaigns;
    use async_trait::async_trait;
    use rstest::rstest;

    /// A planner that plans nothing.
    struct IdlePlanner;

    #[async_trait]
    impl TickPlanner for IdlePlanner {
        async fn tick(&self, _store: Arc<dyn CampaignStore>, _limit: usize) -> PlanReport {
            PlanReport::default()
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
            ..CampaignCfg::default()
        }
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
            }
        );
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let driver = build_driver(
            &cfg,
            backend,
            Tenants::Fixed(vec!["acme".into()]),
            Arc::new(IdlePlanner),
        );
        assert_eq!(*driver.config(), driver_config(&cfg));
        assert_eq!(driver.tenants(), &Tenants::Fixed(vec!["acme".into()]));
        assert!(!driver.has_exec(), "CP-05 ships no worker: claims are off");
        assert_eq!(driver.global_available(), 9);
        assert_eq!(driver.owner().as_str().len(), 32);
    }

    // corner: `enabled = false` builds a driver whose tick is a no-op.
    #[tokio::test]
    async fn corner_build_driver_disabled() {
        let cfg = CampaignCfg {
            enabled: false,
            ..campaign_cfg()
        };
        let backend: Arc<dyn CampaignBackend> = Arc::new(MemCampaigns::new());
        let driver = build_driver(&cfg, backend, Tenants::Discover, Arc::new(IdlePlanner));
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
        );
        let report = driver.tick().await;
        assert!(!report.disabled);
        assert_eq!(report.tenants, ["acme"]);
        assert_eq!(report.claimed(), 0);
        assert_eq!(report.errors, 0);
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
