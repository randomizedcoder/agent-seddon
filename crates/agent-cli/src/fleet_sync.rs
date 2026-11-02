//! The `--serve-fleet` background drivers, tenant-aware and live (forge-auth F6).
//!
//! The fleet's background work — reconcile, the forge poll, the trigger drain — has no
//! ambient caller, so against a per-tenant roster (`[tenancy] per_tenant`, a `PerTenant`
//! store) an unscoped call routes to the default `local` tenant and every other tenant's
//! rows are invisible. These drivers instead discover the tenants holding rows
//! ([`agent_core::FleetTenants`]) and run each tenant's work **under that tenant's
//! scope** ([`agent_core::in_tenant`]), the pattern `agent-runtime`'s scheduler driver set.
//!
//! They are also **live**: a [`FleetSync`] pass (at boot, then every
//! [`FLEET_RESYNC_SECS`]) re-reads each tenant's roster, admits new rows (reconcile is
//! idempotent), drops the owner session of a row that went away or was disabled, and
//! diffs the forge-poll jobs — so a row created, edited or deleted from the portal is
//! picked up within a resync interval rather than at the next restart.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use agent_core::{
    FleetHost, FleetRegistry, FleetSession, FleetTenants, ForgeCard, ForgeRegistry, JobId,
    Scheduler, SessionKey, TriggerSink,
};

/// How often a [`FleetSync`] pass re-reads the roster.
pub(crate) const FLEET_RESYNC_SECS: u64 = 60;

/// The forge-poll job cap across every tenant: each enabled row with a forge is one job.
/// Bounded so a hostile or runaway roster can't grow the scheduler without limit.
pub(crate) const MAX_FLEET_POLL_JOBS: usize = 4096;

pub(crate) use agent_core::in_tenant;

/// The tenants to drive: every tenant holding rows when the roster is per-tenant, else
/// the one unscoped roster (`""`). `Err` when discovery failed — the caller must then act
/// on nothing (never treat a failed listing as "no tenants" and tear their work down).
pub(crate) async fn driven_tenants(
    tenants: Option<&Arc<dyn FleetTenants>>,
) -> agent_core::Result<Vec<String>> {
    match tenants {
        None => Ok(vec![String::new()]),
        Some(t) => t.tenants().await,
    }
}

/// A poll job's goal: `<tenant>/<row id>`. Both halves are path-safe segments (no `/`),
/// and an unscoped row has an empty tenant (`/<id>`).
pub(crate) fn job_goal(tenant: &str, id: &str) -> String {
    format!("{tenant}/{id}")
}

/// The `(tenant, row id)` a [`job_goal`] names; `None` for anything else.
pub(crate) fn parse_job_goal(goal: &str) -> Option<(&str, &str)> {
    let (tenant, id) = goal.split_once('/')?;
    let ok_tenant = tenant.is_empty() || agent_core::safe_segment(tenant);
    (ok_tenant && agent_core::safe_segment(id)).then_some((tenant, id))
}

/// Whether a row is polled: enabled, with a forge either inline or by card.
fn polls(row: &FleetSession) -> bool {
    row.enabled && (!row.backend.is_empty() || !row.forge_id.is_empty())
}

/// What a [`FleetSync`] pass must change in the poll jobs: `cancel` the goals no longer
/// wanted (gone, disabled, or a changed interval), `add` the wanted goals with no job.
/// Only goals of tenants in `synced` are touched — a tenant whose roster read failed this
/// pass keeps its jobs as they were.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct JobPlan {
    pub cancel: Vec<String>,
    pub add: Vec<(String, u64)>,
}

/// Diff the current poll jobs (`goal → poll_secs`) against the wanted ones, restricted to
/// the `synced` tenants. Pure, so the diff is tested without a scheduler or a clock.
pub(crate) fn plan_jobs(
    current: &HashMap<String, u64>,
    wanted: &HashMap<String, u64>,
    synced: &HashSet<String>,
) -> JobPlan {
    let in_synced =
        |goal: &str| parse_job_goal(goal).is_some_and(|(tenant, _)| synced.contains(tenant));
    let mut plan = JobPlan::default();
    for (goal, secs) in current {
        if in_synced(goal) && wanted.get(goal) != Some(secs) {
            plan.cancel.push(goal.clone());
        }
    }
    for (goal, secs) in wanted {
        let changed = current.get(goal).is_some_and(|cur| cur != secs);
        if in_synced(goal) && (changed || !current.contains_key(goal)) {
            plan.add.push((goal.clone(), *secs));
        }
    }
    plan.cancel.sort();
    plan.add.sort();
    plan
}

/// What one [`FleetSync::sync_once`] pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct SyncReport {
    pub tenants: usize,
    pub admitted: usize,
    pub skipped: usize,
    pub removed: usize,
    pub jobs: usize,
    pub jobs_added: usize,
    pub jobs_cancelled: usize,
}

/// The live, tenant-aware roster driver of `--serve-fleet`: owner-session reconcile and
/// the forge-poll job set, re-synced from the roster each pass.
pub(crate) struct FleetSync {
    roster: Arc<dyn FleetRegistry>,
    host: Arc<dyn FleetHost>,
    forge_registry: Option<Arc<dyn ForgeRegistry>>,
    tenants: Option<Arc<dyn FleetTenants>>,
    scheduler: Arc<agent_scheduler::LocalScheduler>,
    /// goal → (job id, poll_secs) for every registered poll job.
    jobs: Mutex<HashMap<String, (JobId, u64)>>,
    /// tenant → the owner sessions admitted for it on the last pass.
    admitted: Mutex<HashMap<String, HashSet<SessionKey>>>,
}

impl FleetSync {
    pub(crate) fn new(
        roster: Arc<dyn FleetRegistry>,
        host: Arc<dyn FleetHost>,
        forge_registry: Option<Arc<dyn ForgeRegistry>>,
        tenants: Option<Arc<dyn FleetTenants>>,
    ) -> Self {
        Self {
            roster,
            host,
            forge_registry,
            tenants,
            scheduler: Arc::new(
                agent_scheduler::LocalScheduler::new().with_max_jobs(MAX_FLEET_POLL_JOBS),
            ),
            jobs: Mutex::new(HashMap::new()),
            admitted: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn scheduler(&self) -> Arc<agent_scheduler::LocalScheduler> {
        self.scheduler.clone()
    }

    pub(crate) fn roster(&self) -> Arc<dyn FleetRegistry> {
        self.roster.clone()
    }

    pub(crate) fn forge_registry(&self) -> Option<Arc<dyn ForgeRegistry>> {
        self.forge_registry.clone()
    }

    /// One pass: per driven tenant (scoped), reconcile the owner sessions and collect
    /// the wanted poll jobs; then drop owner sessions that went away and apply the job
    /// diff. A failed tenant discovery changes nothing; a tenant whose roster read failed
    /// keeps its sessions and jobs as they were.
    pub(crate) async fn sync_once(&self) -> SyncReport {
        let mut report = SyncReport::default();
        let tenants = match driven_tenants(self.tenants.as_ref()).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "fleet sync: tenant discovery failed; nothing changed this pass");
                return report;
            }
        };
        report.tenants = tenants.len();

        let mut synced: HashSet<String> = HashSet::new();
        let mut wanted: HashMap<String, u64> = HashMap::new();
        let mut admitted_now: HashMap<String, HashSet<SessionKey>> = HashMap::new();
        for tenant in &tenants {
            let Some(pass) = in_tenant(tenant, self.tenant_pass()).await else {
                tracing::warn!("fleet sync: refusing to drive an invalid tenant");
                continue;
            };
            let Some(pass) = pass else {
                continue; // a failed roster read: leave this tenant untouched
            };
            synced.insert(tenant.clone());
            report.skipped += pass.skipped;
            for (id, secs) in pass.polls {
                wanted.insert(job_goal(tenant, &id), secs);
            }
            report.admitted += pass.admitted.len();
            admitted_now.insert(tenant.clone(), pass.admitted);
        }

        // Owner sessions admitted last pass whose row is gone/disabled now. A tenant that
        // vanished from a successful discovery has no rows left, so it counts as synced.
        {
            let mut admitted = self.admitted.lock().expect("fleet sync admitted poisoned");
            for (tenant, before) in admitted.iter() {
                let now_synced = synced.contains(tenant) || !tenants.contains(tenant);
                if !now_synced {
                    continue;
                }
                let empty = HashSet::new();
                let now = admitted_now.get(tenant).unwrap_or(&empty);
                for key in before.difference(now) {
                    self.host.remove_session(key);
                    report.removed += 1;
                }
            }
            admitted.retain(|t, _| !synced.contains(t) && tenants.contains(t));
            admitted.extend(admitted_now);
        }

        // Tenants gone from a successful discovery are synced too (their jobs go).
        let mut diff_scope = synced;
        let current: HashMap<String, u64> = {
            let jobs = self.jobs.lock().expect("fleet sync jobs poisoned");
            for goal in jobs.keys() {
                if let Some((t, _)) = parse_job_goal(goal) {
                    if !tenants.iter().any(|x| x == t) {
                        diff_scope.insert(t.to_string());
                    }
                }
            }
            jobs.iter().map(|(g, (_, s))| (g.clone(), *s)).collect()
        };
        let plan = plan_jobs(&current, &wanted, &diff_scope);
        for goal in &plan.cancel {
            let id = self
                .jobs
                .lock()
                .expect("fleet sync jobs poisoned")
                .remove(goal)
                .map(|(id, _)| id);
            if let Some(id) = id {
                if let Err(e) = self.scheduler.cancel(&id).await {
                    tracing::warn!(job = %goal, error = %e, "fleet sync: could not cancel a poll job");
                }
                report.jobs_cancelled += 1;
            }
        }
        for (goal, secs) in &plan.add {
            let spec = format!("every {secs}s");
            match self.scheduler.schedule(&spec, goal).await {
                Ok(id) => {
                    self.jobs
                        .lock()
                        .expect("fleet sync jobs poisoned")
                        .insert(goal.clone(), (id, *secs));
                    report.jobs_added += 1;
                }
                Err(e) => tracing::warn!(job = %goal, spec = %spec, error = %e,
                    "fleet sync: could not register a forge-poll job"),
            }
        }
        report.jobs = self.jobs.lock().expect("fleet sync jobs poisoned").len();
        report
    }

    /// The scoped half of a pass for the tenant in scope: snapshot its forge cards,
    /// reconcile its rows, and list the rows it polls. `None` if its roster read failed.
    async fn tenant_pass(&self) -> Option<TenantPass> {
        let rows = match self.roster.list().await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %e, "fleet sync: roster list failed; tenant left as it was");
                return None;
            }
        };
        // `check_session_forge` is sync, so a row's card is checked against a snapshot of
        // this tenant's cards; a `forge_id` absent from it fails closed.
        let cards: HashMap<String, ForgeCard> = match &self.forge_registry {
            Some(reg) => reg
                .list()
                .await
                .map(|cards| cards.into_iter().map(|c| (c.id.clone(), c)).collect())
                .unwrap_or_default(),
            None => HashMap::new(),
        };
        let check = move |row: &FleetSession| {
            let card = if row.forge_id.is_empty() {
                None
            } else {
                cards.get(&row.forge_id)
            };
            agent_runtime::check_session_forge(row, card)
        };
        let report =
            agent_review_fleet::reconcile(self.roster.as_ref(), self.host.as_ref(), &check).await;
        Some(TenantPass {
            admitted: report.admitted.into_iter().collect(),
            skipped: report.skipped.len(),
            polls: rows
                .iter()
                .filter(|r| polls(r))
                .map(|r| (r.id.clone(), r.poll_secs))
                .collect(),
        })
    }
}

struct TenantPass {
    admitted: HashSet<SessionKey>,
    skipped: usize,
    polls: Vec<(String, u64)>,
}

/// Poll one job's row: under its tenant's scope, re-read the row, build its forge (inline
/// or by card) and emit a trigger per open non-draft PR, each stamped with the tenant.
/// A row whose last polls failed is skipped until its `backoff` wait has passed, so a
/// failing or rate-limiting forge is not asked again at the row's full cadence.
pub(crate) async fn poll_job(
    goal: String,
    roster: Arc<dyn FleetRegistry>,
    forge_registry: Option<Arc<dyn ForgeRegistry>>,
    sink: Arc<dyn TriggerSink>,
    backoff: Arc<agent_review_fleet::PollBackoff>,
) -> agent_core::Result<String> {
    let Some((tenant, id)) = parse_job_goal(&goal) else {
        return Err(agent_core::Error::Scheduler(
            "malformed fleet poll job".into(),
        ));
    };
    if let Some(left) = backoff.wait(&goal, std::time::Instant::now()) {
        return Ok(format!(
            "skipped {goal}: backing off another {}s after failed polls",
            left.as_secs().max(1)
        ));
    }
    let work = async {
        let row = roster
            .get(id)
            .await
            .map_err(|e| agent_core::Error::Scheduler(e.to_string()))?;
        match agent_runtime::resolve_session_forge(&row, forge_registry.as_ref())
            .await
            .map_err(|e| agent_core::Error::Scheduler(e.to_string()))?
        {
            Some(forge) => {
                let report = agent_review_fleet::poll_session(
                    forge.as_ref(),
                    tenant,
                    id,
                    sink.as_ref(),
                    agent_review_fleet::MAX_TRIGGERS_PER_TICK,
                )
                .await?;
                Ok(format!(
                    "polled {goal}: {} emitted / {} scanned{}",
                    report.emitted,
                    report.scanned,
                    if report.capped { " (capped)" } else { "" }
                ))
            }
            None => Ok(format!("polled {goal}: no forge configured")),
        }
    };
    let result = in_tenant(tenant, work)
        .await
        .unwrap_or_else(|| Err(agent_core::Error::Scheduler("invalid fleet tenant".into())));
    if let Some(wait) = backoff.record(&goal, result.is_ok(), std::time::Instant::now()) {
        tracing::warn!(job = %goal, wait_secs = wait.as_secs(),
            "fleet: forge poll failed; backing off before the next poll of this row");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{DriverError, UserId};
    use rstest::rstest;

    fn map(items: &[(&str, u64)]) -> HashMap<String, u64> {
        items.iter().map(|(g, s)| (g.to_string(), *s)).collect()
    }

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(std::string::ToString::to_string).collect()
    }

    #[rstest]
    #[case::positive_tenant_row("acme/web", Some(("acme", "web")))]
    #[case::positive_unscoped_row("/web", Some(("", "web")))]
    #[case::negative_no_separator("web", None)]
    #[case::negative_empty_id("acme/", None)]
    #[case::corner_empty("", None)]
    #[case::boundary_extra_separator("acme/web/x", None)]
    #[case::adversarial_traversal_tenant("../acme/web", None)]
    #[case::adversarial_traversal_id("acme/..", None)]
    #[case::adversarial_leading_dash_id("acme/-rf", None)]
    fn job_goal_round_trip(#[case] goal: &str, #[case] want: Option<(&str, &str)>) {
        assert_eq!(parse_job_goal(goal), want);
        if let Some((t, id)) = want {
            assert_eq!(job_goal(t, id), goal);
        }
    }

    // `(current, wanted, synced) → (cancel, add)`: only synced tenants' jobs move; a
    // changed interval is a cancel + an add; an unchanged job is left alone.
    #[rstest]
    #[case::positive_new_row_added(&[], &[("acme/a", 60)], &["acme"], &[], &[("acme/a", 60)])]
    #[case::positive_removed_row_cancelled(&[("acme/a", 60)], &[], &["acme"], &["acme/a"], &[])]
    #[case::positive_unchanged_left(&[("acme/a", 60)], &[("acme/a", 60)], &["acme"], &[], &[])]
    #[case::positive_interval_change_replaced(&[("acme/a", 60)], &[("acme/a", 300)], &["acme"], &["acme/a"], &[("acme/a", 300)])]
    #[case::positive_unscoped_roster(&[("/a", 60)], &[("/b", 60)], &[""], &["/a"], &[("/b", 60)])]
    #[case::negative_unsynced_tenant_untouched(&[("globex/a", 60)], &[], &["acme"], &[], &[])]
    #[case::corner_two_tenants_same_id(&[("acme/a", 60)], &[("acme/a", 60), ("globex/a", 60)], &["acme", "globex"], &[], &[("globex/a", 60)])]
    #[case::corner_nothing_synced(&[("acme/a", 60)], &[("acme/b", 60)], &[], &[], &[])]
    #[case::adversarial_malformed_goal_never_touched(&[("../x", 60)], &[], &["acme", ""], &[], &[])]
    fn plan_jobs_cases(
        #[case] current: &[(&str, u64)],
        #[case] wanted: &[(&str, u64)],
        #[case] synced: &[&str],
        #[case] cancel: &[&str],
        #[case] add: &[(&str, u64)],
    ) {
        let plan = plan_jobs(&map(current), &map(wanted), &set(synced));
        let want = JobPlan {
            cancel: cancel
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            add: add.iter().map(|(g, s)| (g.to_string(), *s)).collect(),
        };
        assert_eq!(plan, want);
    }

    /// A per-tenant roster double: routes every call by the ambient tenant, like the
    /// `PerTenant` store does, so a test sees exactly what each scope reads.
    #[derive(Default)]
    struct TenantRoster {
        by_tenant: Mutex<HashMap<String, Vec<FleetSession>>>,
        fail: Mutex<HashSet<String>>,
    }

    impl TenantRoster {
        fn set(&self, tenant: &str, rows: Vec<FleetSession>) {
            self.by_tenant.lock().unwrap().insert(tenant.into(), rows);
        }
        fn rows(&self) -> agent_core::Result<Vec<FleetSession>> {
            let t = agent_core::current_tenant();
            if self.fail.lock().unwrap().contains(&t) {
                return Err(agent_core::Error::Fleet("down".into()));
            }
            Ok(self
                .by_tenant
                .lock()
                .unwrap()
                .get(&t)
                .cloned()
                .unwrap_or_default())
        }
    }

    #[async_trait::async_trait]
    impl FleetRegistry for TenantRoster {
        async fn list(&self) -> agent_core::Result<Vec<FleetSession>> {
            self.rows()
        }
        async fn get(&self, id: &str) -> agent_core::Result<FleetSession> {
            self.rows()?
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| agent_core::Error::Fleet(format!("not found: {id}")))
        }
        async fn put(&self, s: FleetSession) -> agent_core::Result<FleetSession> {
            Ok(s)
        }
        async fn delete(&self, _id: &str) -> agent_core::Result<bool> {
            Ok(false)
        }
        async fn set_enabled(&self, id: &str, _enabled: bool) -> agent_core::Result<FleetSession> {
            self.get(id).await
        }
    }

    struct Tenants(Mutex<agent_core::Result<Vec<String>>>);

    #[async_trait::async_trait]
    impl FleetTenants for Tenants {
        async fn tenants(&self) -> agent_core::Result<Vec<String>> {
            match &*self.0.lock().unwrap() {
                Ok(t) => Ok(t.clone()),
                Err(_) => Err(agent_core::Error::Fleet("discovery down".into())),
            }
        }
    }

    #[derive(Default)]
    struct Host {
        live: Mutex<HashSet<SessionKey>>,
    }

    #[async_trait::async_trait]
    impl FleetHost for Host {
        fn admit_owner(&self, key: SessionKey) -> std::result::Result<(), DriverError> {
            self.live.lock().unwrap().insert(key);
            Ok(())
        }
        fn remove_session(&self, key: &SessionKey) {
            self.live.lock().unwrap().remove(key);
        }
        async fn run_review(
            &self,
            _key: SessionKey,
            _goal: String,
            _skill: Option<String>,
            _workspace: agent_core::ReviewWorkspace,
        ) -> std::result::Result<String, DriverError> {
            unreachable!("sync never runs a review")
        }
    }

    fn row(user: &str, id: &str, enabled: bool) -> FleetSession {
        FleetSession {
            id: id.into(),
            user: user.into(),
            repo: "acme__web".into(),
            backend: "github".into(),
            token_ref: "env:FLEET_SYNC_TEST_TOKEN".into(),
            poll_secs: 60,
            enabled,
            ..Default::default()
        }
    }

    fn owner(user: &str, id: &str) -> SessionKey {
        SessionKey::parse(user, id).unwrap()
    }

    struct Rig {
        roster: Arc<TenantRoster>,
        tenants: Arc<Tenants>,
        host: Arc<Host>,
        sync: FleetSync,
    }

    fn rig(tenants: &[&str]) -> Rig {
        std::env::set_var("FLEET_SYNC_TEST_TOKEN", "t0ken");
        let roster = Arc::new(TenantRoster::default());
        let tenants = Arc::new(Tenants(Mutex::new(Ok(tenants
            .iter()
            .map(std::string::ToString::to_string)
            .collect()))));
        let host = Arc::new(Host::default());
        let sync = FleetSync::new(
            roster.clone(),
            host.clone(),
            None,
            Some(tenants.clone() as Arc<dyn FleetTenants>),
        );
        Rig {
            roster,
            tenants,
            host,
            sync,
        }
    }

    fn goals(sync: &FleetSync) -> Vec<String> {
        let mut g: Vec<String> = sync.jobs.lock().unwrap().keys().cloned().collect();
        g.sort();
        g
    }

    #[tokio::test]
    async fn positive_each_tenant_reconciled_and_polled_under_its_scope() {
        let r = rig(&["acme", "globex"]);
        r.roster.set("acme", vec![row("acme", "web", true)]);
        r.roster.set(
            "globex",
            vec![row("globex", "web", true), row("globex", "off", false)],
        );
        let rep = r.sync.sync_once().await;
        assert_eq!((rep.tenants, rep.admitted, rep.jobs), (2, 2, 2));
        assert_eq!(goals(&r.sync), ["acme/web", "globex/web"]);
        let live = r.host.live.lock().unwrap().clone();
        assert_eq!(
            live,
            HashSet::from([owner("acme", "web"), owner("globex", "web")])
        );
    }

    #[tokio::test]
    async fn positive_live_resync_picks_up_edits_without_restart() {
        let r = rig(&["acme"]);
        r.roster.set("acme", vec![row("acme", "web", true)]);
        r.sync.sync_once().await;
        // Added row, disabled row, changed interval — all on the next pass.
        let mut web = row("acme", "web", false);
        web.poll_secs = 60;
        let mut api = row("acme", "api", true);
        api.poll_secs = 300;
        r.roster.set("acme", vec![web, api]);
        let rep = r.sync.sync_once().await;
        assert_eq!(goals(&r.sync), ["acme/api"]);
        assert_eq!((rep.jobs_added, rep.jobs_cancelled, rep.removed), (1, 1, 1));
        assert_eq!(
            r.host.live.lock().unwrap().clone(),
            HashSet::from([owner("acme", "api")])
        );
    }

    #[tokio::test]
    async fn negative_failed_discovery_changes_nothing() {
        let r = rig(&["acme"]);
        r.roster.set("acme", vec![row("acme", "web", true)]);
        r.sync.sync_once().await;
        *r.tenants.0.lock().unwrap() = Err(agent_core::Error::Fleet("x".into()));
        let rep = r.sync.sync_once().await;
        assert_eq!(rep, SyncReport::default());
        assert_eq!(
            goals(&r.sync),
            ["acme/web"],
            "jobs kept on a failed discovery"
        );
        assert_eq!(r.host.live.lock().unwrap().len(), 1, "sessions kept");
    }

    #[tokio::test]
    async fn negative_failed_tenant_read_keeps_that_tenant_only() {
        let r = rig(&["acme", "globex"]);
        r.roster.set("acme", vec![row("acme", "web", true)]);
        r.roster.set("globex", vec![row("globex", "web", true)]);
        r.sync.sync_once().await;
        r.roster.fail.lock().unwrap().insert("acme".into());
        r.roster.set("globex", vec![]);
        r.sync.sync_once().await;
        assert_eq!(
            goals(&r.sync),
            ["acme/web"],
            "acme untouched; globex's row went"
        );
        assert_eq!(
            r.host.live.lock().unwrap().clone(),
            HashSet::from([owner("acme", "web")])
        );
    }

    #[tokio::test]
    async fn corner_tenant_gone_from_discovery_is_torn_down() {
        let r = rig(&["acme", "globex"]);
        r.roster.set("acme", vec![row("acme", "web", true)]);
        r.roster.set("globex", vec![row("globex", "web", true)]);
        r.sync.sync_once().await;
        *r.tenants.0.lock().unwrap() = Ok(vec!["acme".into()]);
        r.sync.sync_once().await;
        assert_eq!(goals(&r.sync), ["acme/web"]);
        assert!(!r
            .host
            .live
            .lock()
            .unwrap()
            .contains(&owner("globex", "web")));
    }

    #[tokio::test]
    async fn corner_unscoped_roster_without_discovery() {
        std::env::set_var("FLEET_SYNC_TEST_TOKEN", "t0ken");
        let roster = Arc::new(TenantRoster::default());
        roster.set(UserId::LOCAL, vec![row("acme", "web", true)]);
        let host = Arc::new(Host::default());
        let sync = FleetSync::new(roster, host.clone(), None, None);
        let rep = sync.sync_once().await;
        assert_eq!((rep.tenants, rep.jobs), (1, 1));
        assert_eq!(goals(&sync), ["/web"]);
    }

    #[tokio::test]
    async fn adversarial_hostile_tenant_from_discovery_is_never_driven() {
        let r = rig(&["../etc", "acme"]);
        r.roster.set("acme", vec![row("acme", "web", true)]);
        r.roster
            .set(UserId::LOCAL, vec![row("local", "leak", true)]);
        let rep = r.sync.sync_once().await;
        assert_eq!(
            goals(&r.sync),
            ["acme/web"],
            "no fallback to the local roster"
        );
        assert_eq!(rep.admitted, 1);
    }

    #[tokio::test]
    async fn adversarial_in_tenant_refuses_invalid_tenant() {
        assert_eq!(in_tenant("../x", async { 1 }).await, None);
        assert_eq!(
            in_tenant("", async { agent_core::current_tenant() })
                .await
                .as_deref(),
            Some("local")
        );
        assert_eq!(
            in_tenant("acme", async { agent_core::current_tenant() })
                .await
                .as_deref(),
            Some("acme")
        );
    }

    #[tokio::test]
    async fn negative_malformed_poll_job_refused() {
        let roster: Arc<dyn FleetRegistry> = Arc::new(TenantRoster::default());
        let (queue, _rx) = agent_review_fleet::TriggerQueue::channel_for(4, true);
        let backoff = Arc::new(agent_review_fleet::PollBackoff::new());
        let err = poll_job("../x".into(), roster, None, queue, backoff.clone())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("malformed"));
        assert_eq!(
            backoff.wait("../x", std::time::Instant::now()),
            None,
            "a refused job name is not tracked"
        );
    }

    /// A row whose poll fails is backed off: the next fire is skipped without reading
    /// the roster or the forge, and says why.
    #[tokio::test]
    async fn positive_failed_poll_backs_off_the_row() {
        let roster: Arc<dyn FleetRegistry> = Arc::new(TenantRoster::default());
        let (queue, _rx) = agent_review_fleet::TriggerQueue::channel_for(4, true);
        let backoff = Arc::new(agent_review_fleet::PollBackoff::new());
        let goal = job_goal("acme", "missing-row");
        let first = poll_job(
            goal.clone(),
            roster.clone(),
            None,
            queue.clone(),
            backoff.clone(),
        )
        .await;
        assert!(first.is_err(), "an unknown row fails the poll: {first:?}");
        let second = poll_job(goal.clone(), roster, None, queue, backoff.clone())
            .await
            .expect("a backed-off row is skipped, not failed");
        assert!(second.starts_with("skipped"), "{second}");
        assert!(backoff.wait(&goal, std::time::Instant::now()).is_some());
    }
}
