//! The `LspBackend` seam as a service — language intelligence behind gRPC.
//!
//! A language server is a long-lived process with a warm index: `rust-analyzer`
//! can take minutes to reach steady state and hold gigabytes of it. Hosting the
//! seam separately means that warm process is shared and survives the agent
//! restarting, instead of every agent paying the cold-start cost.

use std::sync::Arc;

use agent_core::LspBackend;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct LspServiceSvc {
    inner: Arc<dyn LspBackend>,
}

impl LspServiceSvc {
    pub fn new(inner: Arc<dyn LspBackend>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::lsp_service_server::LspServiceServer<Self> {
        pb::lsp_service_server::LspServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::lsp_service_server::LspService for LspServiceSvc {
    async fn open(
        &self,
        request: Request<pb::LspOpenRequest>,
    ) -> Result<Response<pb::LspOpenResponse>, Status> {
        let sp = span("lsp.open", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            inner
                .open(&req.uri, &req.text)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::LspOpenResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn request(
        &self,
        request: Request<pb::LspRequestMsg>,
    ) -> Result<Response<pb::LspResultMsg>, Status> {
        let sp = span("lsp.request", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req: agent_core::LspRequest = request.into_inner().into();
            let out = inner
                .request(&req)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(out.into()))
        }
        .instrument(sp)
        .await
    }

    async fn capabilities(
        &self,
        request: Request<pb::LspCapabilitiesRequest>,
    ) -> Result<Response<pb::LspCapabilities>, Status> {
        let sp = span("lsp.capabilities", request.metadata());
        let caps = self.inner.capabilities(&request.into_inner().language);
        async move { Ok(Response::new(caps.into())) }
            .instrument(sp)
            .await
    }

    async fn shutdown(
        &self,
        request: Request<pb::LspShutdownRequest>,
    ) -> Result<Response<pb::LspShutdownResponse>, Status> {
        let sp = span("lsp.shutdown", request.metadata());
        let inner = self.inner.clone();
        // NOTE: this shuts down the SERVER's language servers, not the client's
        // connection. A shared host serving several agents should not expose
        // this to all of them — see docs/components/lsp.md.
        async move {
            inner.shutdown().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::LspShutdownResponse {}))
        }
        .instrument(sp)
        .await
    }
}

pub fn lsp_router(inner: Arc<dyn LspBackend>) -> Router {
    Server::builder().add_service(LspServiceSvc::new(inner).into_server())
}
