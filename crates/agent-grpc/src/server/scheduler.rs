//! The `Scheduler` seam as a service — unattended recurring runs behind gRPC.
//!
//! Every RPC scopes the caller's ambient identity (`identity_key` + `run_scoped`,
//! mirroring `prompt.rs`/`memory.rs`) before touching the store, so a
//! `PerTenant<dyn Scheduler>`-wrapped backend (`agent-runtime/src/tenant.rs`)
//! routes a standalone `--serve-scheduler` caller to *their* tenant's jobs rather
//! than the default `local` store. Identity is attacker-controlled and fails
//! **closed** to `None` → the `local` tenant (see `server::identity_key`).

use std::sync::Arc;

use agent_core::Scheduler;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct SchedulerServiceSvc {
    inner: Arc<dyn Scheduler>,
}

impl SchedulerServiceSvc {
    pub fn new(inner: Arc<dyn Scheduler>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::scheduler_service_server::SchedulerServiceServer<Self> {
        pb::scheduler_service_server::SchedulerServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::scheduler_service_server::SchedulerService for SchedulerServiceSvc {
    async fn schedule(
        &self,
        request: Request<pb::SchedScheduleRequest>,
    ) -> Result<Response<pb::SchedJobRef>, Status> {
        super::authz::require(
            agent_core::Action::Schedule,
            agent_core::ResourceType::Scheduler,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("scheduler.schedule", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let req = request.into_inner();
            let id = inner
                .schedule(&req.spec, &req.goal)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedJobRef { id }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn list(
        &self,
        request: Request<pb::SchedListRequest>,
    ) -> Result<Response<pb::SchedJobList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("scheduler.list", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let jobs = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedJobList {
                jobs: jobs.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn cancel(
        &self,
        request: Request<pb::SchedJobRef>,
    ) -> Result<Response<pb::SchedCancelResponse>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::Scheduler,
        )?;
        let key = super::identity_key(request.metadata());
        let sp = span("scheduler.cancel", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let cancelled = inner
                .cancel(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedCancelResponse { cancelled }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn history(
        &self,
        request: Request<pb::SchedJobRef>,
    ) -> Result<Response<pb::SchedRunList>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("scheduler.history", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let runs = inner
                .history(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedRunList {
                runs: runs.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn scheduler_router(inner: Arc<dyn Scheduler>) -> Router {
    Server::builder().add_service(SchedulerServiceSvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Job, Run};
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use pb::scheduler_service_server::SchedulerService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) at each store call — proves the
    // served handler scopes the caller into a `PerTenant`-wrapped scheduler (C31 Principle 3).
    #[derive(Default)]
    struct TenantProbeScheduler {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeScheduler {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl Scheduler for TenantProbeScheduler {
        fn name(&self) -> &str {
            "tenant-probe"
        }
        async fn schedule(
            &self,
            _spec: &str,
            _goal: &str,
        ) -> agent_core::Result<agent_core::JobId> {
            self.record();
            Ok("job-1".to_string())
        }
        async fn list(&self) -> agent_core::Result<Vec<Job>> {
            self.record();
            Ok(vec![])
        }
        async fn cancel(&self, _id: &str) -> agent_core::Result<bool> {
            self.record();
            Ok(false)
        }
        async fn history(&self, _id: &str) -> agent_core::Result<Vec<Run>> {
            self.record();
            Ok(vec![])
        }
    }

    fn req_with<T>(payload: T, user: Option<&str>, session: Option<&str>) -> Request<T> {
        let mut req = Request::new(payload);
        if let Some(u) = user {
            req.metadata_mut()
                .insert(USER_ID_KEY, MetadataValue::try_from(u).unwrap());
        }
        if let Some(s) = session {
            req.metadata_mut()
                .insert(SESSION_ID_KEY, MetadataValue::try_from(s).unwrap());
        }
        req
    }

    // Every caller-identity class → the tenant the store runs under. Present + path-safe
    // scopes; partial/hostile fails closed to the default tenant (`None`).
    #[rstest]
    #[case::positive_tenant_header_scopes_to_tenant(Some("acme".into()), Some("s1".into()), Some("acme".into()))]
    #[case::negative_no_identity_runs_as_local(None, None, None)]
    #[case::negative_second_tenant_is_isolated(Some("globex".into()), Some("s1".into()), Some("globex".into()))]
    #[case::boundary_max_len_tenant_segment(Some("a".repeat(128)), Some("s1".into()), Some("a".repeat(128)))]
    #[case::corner_user_without_session_runs_as_local(Some("acme".into()), None, None)]
    #[case::adversarial_traversal_header_fails_to_local(Some("../../heads/main".into()), Some("s1".into()), None)]
    #[case::adversarial_empty_header_fails_to_local(Some(String::new()), Some("s1".into()), None)]
    #[tokio::test]
    async fn scheduler_list_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeScheduler::default());
        let svc = SchedulerServiceSvc::new(store.clone());
        let req = req_with(
            pb::SchedListRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.list(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // Two tenants over the wire hit disjoint scopes — a cross-tenant read cannot see the other.
    #[tokio::test]
    async fn positive_two_tenants_are_isolated_across_rpcs() {
        let store = Arc::new(TenantProbeScheduler::default());
        let svc = SchedulerServiceSvc::new(store.clone());
        svc.schedule(req_with(
            pb::SchedScheduleRequest::default(),
            Some("acme"),
            Some("s1"),
        ))
        .await
        .unwrap();
        svc.list(req_with(
            pb::SchedListRequest::default(),
            Some("globex"),
            Some("s1"),
        ))
        .await
        .unwrap();
        assert_eq!(
            store.seen.lock().unwrap().as_slice(),
            &[Some("acme".to_string()), Some("globex".to_string())]
        );
    }
}
