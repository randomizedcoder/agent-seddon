//! The `SessionStore` seam as a service — content-addressed conversation
//! history (checkpoint / branch / undo / fork) behind gRPC.

use std::sync::Arc;

use agent_core::SessionStore;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{missing, span};

pub struct SessionServiceSvc {
    inner: Arc<dyn SessionStore>,
}

impl SessionServiceSvc {
    pub fn new(inner: Arc<dyn SessionStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::session_service_server::SessionServiceServer<Self> {
        pb::session_service_server::SessionServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::session_service_server::SessionService for SessionServiceSvc {
    async fn checkpoint(
        &self,
        request: Request<pb::SessionCheckpointRequest>,
    ) -> Result<Response<pb::SessionCheckpointRef>, Status> {
        let sp = span("session.checkpoint", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let working = req.working.ok_or_else(|| missing("working"))?.try_into()?;
            let id = inner
                .checkpoint(&req.session, &working, &req.label)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SessionCheckpointRef { id }))
        }
        .instrument(sp)
        .await
    }

    async fn list(
        &self,
        request: Request<pb::SessionRef>,
    ) -> Result<Response<pb::SessionCheckpointList>, Status> {
        let sp = span("session.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let metas = inner
                .list(&request.into_inner().session)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SessionCheckpointList {
                checkpoints: metas.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn restore(
        &self,
        request: Request<pb::SessionCheckpointRef>,
    ) -> Result<Response<pb::WorkingSet>, Status> {
        let sp = span("session.restore", request.metadata());
        let inner = self.inner.clone();
        async move {
            let ws = inner
                .restore(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(ws.into()))
        }
        .instrument(sp)
        .await
    }

    async fn branch(
        &self,
        request: Request<pb::SessionBranchRequest>,
    ) -> Result<Response<pb::SessionBranchResponse>, Status> {
        let sp = span("session.branch", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            inner
                .branch(&req.session, &req.from, &req.name)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SessionBranchResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn undo(
        &self,
        request: Request<pb::SessionUndoRequest>,
    ) -> Result<Response<pb::SessionCheckpointRef>, Status> {
        let sp = span("session.undo", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let id = inner
                .undo(&req.session, req.n)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SessionCheckpointRef { id }))
        }
        .instrument(sp)
        .await
    }

    async fn fork(
        &self,
        request: Request<pb::SessionRef>,
    ) -> Result<Response<pb::SessionRef>, Status> {
        let sp = span("session.fork", request.metadata());
        let inner = self.inner.clone();
        async move {
            let session = inner
                .fork(&request.into_inner().session)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SessionRef { session }))
        }
        .instrument(sp)
        .await
    }

    async fn diff(
        &self,
        request: Request<pb::SessionDiffRequest>,
    ) -> Result<Response<pb::SessionCheckpointDiff>, Status> {
        let sp = span("session.diff", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let d = inner
                .diff(&req.a, &req.b)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(d.into()))
        }
        .instrument(sp)
        .await
    }

    async fn prune(
        &self,
        request: Request<pb::SessionRef>,
    ) -> Result<Response<pb::SessionPruneResponse>, Status> {
        let sp = span("session.prune", request.metadata());
        let inner = self.inner.clone();
        async move {
            let n = inner
                .prune(&request.into_inner().session)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SessionPruneResponse {
                reclaimed: n as u64,
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn session_router(inner: Arc<dyn SessionStore>) -> Router {
    Server::builder().add_service(SessionServiceSvc::new(inner).into_server())
}
