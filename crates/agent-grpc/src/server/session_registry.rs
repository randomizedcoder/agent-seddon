//! The session-lifecycle seam as a service (docs/design/multi-session/05-lifecycle.md):
//! `Open` server-mints an id, `Close` frees state, `Heartbeat` keeps a session warm.

use std::sync::Arc;

use agent_core::{SessionKey, SessionRegistry};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct SessionRegistrySvc {
    inner: Arc<dyn SessionRegistry>,
}

impl SessionRegistrySvc {
    pub fn new(inner: Arc<dyn SessionRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::session_registry_service_server::SessionRegistryServiceServer<Self> {
        pb::session_registry_service_server::SessionRegistryServiceServer::new(self)
    }
}

/// Build the caller-named `(user, session_id)` key, validating both segments — the
/// values are attacker-controlled, so a malformed one is rejected as `INVALID_ARGUMENT`
/// rather than sanitized.
#[allow(clippy::result_large_err)]
fn key_of(user: &str, session_id: &str) -> Result<SessionKey, Status> {
    SessionKey::parse(user, session_id)
        .map_err(|e| Status::invalid_argument(format!("invalid identity: {e}")))
}

#[tonic::async_trait]
impl pb::session_registry_service_server::SessionRegistryService for SessionRegistrySvc {
    async fn open(
        &self,
        request: Request<pb::OpenRequest>,
    ) -> Result<Response<pb::OpenResponse>, Status> {
        let sp = span("session_registry.open", request.metadata());
        let inner = self.inner.clone();
        async move {
            let user = request.into_inner().user;
            let session_id = inner.open(&user).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::OpenResponse {
                session_id: session_id.as_str().to_string(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn close(
        &self,
        request: Request<pb::CloseRequest>,
    ) -> Result<Response<pb::CloseResponse>, Status> {
        let sp = span("session_registry.close", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let key = key_of(&req.user, &req.session_id)?;
            inner.close(&key).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::CloseResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn heartbeat(
        &self,
        request: Request<pb::HeartbeatRequest>,
    ) -> Result<Response<pb::HeartbeatResponse>, Status> {
        let sp = span("session_registry.heartbeat", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let key = key_of(&req.user, &req.session_id)?;
            inner
                .heartbeat(&key)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::HeartbeatResponse {}))
        }
        .instrument(sp)
        .await
    }
}

pub fn session_registry_router(inner: Arc<dyn SessionRegistry>) -> Router {
    Server::builder().add_service(SessionRegistrySvc::new(inner).into_server())
}
