//! The agent-session observation surface as a service (docs/design/portal): a live
//! structured feed of the running loop + a status snapshot.
//!
//! Read-only. `Subscribe` sends the current `StatusSnapshot` first (so a late
//! joiner is consistent), then the live tail — a slow consumer lags and drops
//! rather than stalling the loop (bounded broadcast on the runtime side).

use std::pin::Pin;
use std::sync::Arc;

use agent_core::SessionSource;
use agent_proto::{pb, snapshot_event};
use futures_util::{Stream, StreamExt};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct AgentSessionSvc {
    source: Arc<dyn SessionSource>,
}

impl AgentSessionSvc {
    pub fn new(source: Arc<dyn SessionSource>) -> Self {
        Self { source }
    }
    pub fn into_server(self) -> pb::agent_session_service_server::AgentSessionServiceServer<Self> {
        pb::agent_session_service_server::AgentSessionServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::agent_session_service_server::AgentSessionService for AgentSessionSvc {
    type SubscribeStream = Pin<Box<dyn Stream<Item = Result<pb::SessionEvent, Status>> + Send>>;

    #[allow(clippy::result_large_err)]
    async fn subscribe(
        &self,
        request: Request<pb::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let sp = span("agent_session.subscribe", request.metadata());
        // The snapshot is taken *before* subscribing so no event is missed between
        // the two (the leading snapshot may double a just-published event — the
        // client renders idempotently).
        let snapshot = snapshot_event(self.source.snapshot());
        let tail = self
            .source
            .subscribe()
            .map(|ev| Ok(pb::SessionEvent::from(ev)));
        let stream = futures_util::stream::once(async move { Ok(snapshot) }).chain(tail);
        let _ = sp;
        Ok(Response::new(Box::pin(stream) as Self::SubscribeStream))
    }

    async fn snapshot(
        &self,
        request: Request<pb::SnapshotRequest>,
    ) -> Result<Response<pb::StatusSnapshot>, Status> {
        let sp = span("agent_session.snapshot", request.metadata());
        let source = self.source.clone();
        async move { Ok(Response::new(source.snapshot().into())) }
            .instrument(sp)
            .await
    }
}

pub fn agent_session_router(source: Arc<dyn SessionSource>) -> Router {
    Server::builder().add_service(AgentSessionSvc::new(source).into_server())
}
