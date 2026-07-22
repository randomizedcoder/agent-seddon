//! The `Scheduler` seam as a service — unattended recurring runs behind gRPC.

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
        let sp = span("scheduler.schedule", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let id = inner
                .schedule(&req.spec, &req.goal)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedJobRef { id }))
        }
        .instrument(sp)
        .await
    }

    async fn list(
        &self,
        request: Request<pb::SchedListRequest>,
    ) -> Result<Response<pb::SchedJobList>, Status> {
        let sp = span("scheduler.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let jobs = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedJobList {
                jobs: jobs.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn cancel(
        &self,
        request: Request<pb::SchedJobRef>,
    ) -> Result<Response<pb::SchedCancelResponse>, Status> {
        let sp = span("scheduler.cancel", request.metadata());
        let inner = self.inner.clone();
        async move {
            let cancelled = inner
                .cancel(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedCancelResponse { cancelled }))
        }
        .instrument(sp)
        .await
    }

    async fn history(
        &self,
        request: Request<pb::SchedJobRef>,
    ) -> Result<Response<pb::SchedRunList>, Status> {
        let sp = span("scheduler.history", request.metadata());
        let inner = self.inner.clone();
        async move {
            let runs = inner
                .history(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SchedRunList {
                runs: runs.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn scheduler_router(inner: Arc<dyn Scheduler>) -> Router {
    Server::builder().add_service(SchedulerServiceSvc::new(inner).into_server())
}
