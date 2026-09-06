//! The `FleetRegistry` seam as a service (review-fleet C3): the review-fleet
//! roster — CRUD over `FleetSession` rows (`agent --serve-fleet` control plane).
//!
//! Fails **hard** (`Err` → `Status`), like the provider-registry seam: a
//! control-plane mutation that fails should surface to the operator, not
//! silently degrade. Every id/row is untrusted and validated inside the store
//! (`safe_segment`, number clamps, count caps); a rejection maps to
//! `InvalidArgument` — and a store's `not found` to `NotFound` — via
//! `status_from_error`. Rows carry `token_ref` *references*; the server never
//! resolves one (there is no token to leak), so `Get`/`List` return the
//! reference verbatim.

use std::sync::Arc;

use agent_core::FleetRegistry;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ReviewFleetSvc {
    inner: Arc<dyn FleetRegistry>,
}

impl ReviewFleetSvc {
    pub fn new(inner: Arc<dyn FleetRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::review_fleet_service_server::ReviewFleetServiceServer<Self> {
        pb::review_fleet_service_server::ReviewFleetServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::review_fleet_service_server::ReviewFleetService for ReviewFleetSvc {
    async fn list(
        &self,
        request: Request<pb::FleetListRequest>,
    ) -> Result<Response<pb::FleetSessionList>, Status> {
        let sp = span("fleet.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let rows = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::FleetSessionList {
                sessions: rows.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(
        &self,
        request: Request<pb::FleetSessionRef>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        let sp = span("fleet.get", request.metadata());
        let inner = self.inner.clone();
        async move {
            let row = inner
                .get(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(row.into()))
        }
        .instrument(sp)
        .await
    }

    async fn put(
        &self,
        request: Request<pb::FleetSession>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        let sp = span("fleet.put", request.metadata());
        let inner = self.inner.clone();
        async move {
            // Wire → core clamps numbers; the store validates fail-closed.
            let session = agent_core::FleetSession::from(request.into_inner());
            let stored = inner
                .put(session)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn delete(
        &self,
        request: Request<pb::FleetSessionRef>,
    ) -> Result<Response<pb::FleetDeleteReply>, Status> {
        let sp = span("fleet.delete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::FleetDeleteReply { deleted }))
        }
        .instrument(sp)
        .await
    }

    async fn set_enabled(
        &self,
        request: Request<pb::FleetSetEnabledRequest>,
    ) -> Result<Response<pb::FleetSession>, Status> {
        let sp = span("fleet.set_enabled", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let row = inner
                .set_enabled(&req.id, req.enabled)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(row.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn review_fleet_router(inner: Arc<dyn FleetRegistry>) -> Router {
    Server::builder().add_service(ReviewFleetSvc::new(inner).into_server())
}
