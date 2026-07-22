//! The `Sandbox` and `Pty` seams as services — running things somewhere else.
//!
//! # Security
//!
//! These two are a materially larger grant than any other seam here. Both accept
//! a command string and run it, so **anyone who can reach the socket can execute
//! code on this host as this user**. The transport is unauthenticated by design
//! (a unix socket 0o600 in a 0o700 dir, or loopback TCP); exposing either beyond
//! that is exactly as dangerous as it sounds.
//!
//! Note also what the server does *not* do: the `Policy` gate lives on the agent
//! side, in front of the tool. A seam server hosts the raw capability.

use std::sync::Arc;

use agent_core::{Pty, Sandbox};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct SandboxServiceSvc {
    inner: Arc<dyn Sandbox>,
}

impl SandboxServiceSvc {
    pub fn new(inner: Arc<dyn Sandbox>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::sandbox_service_server::SandboxServiceServer<Self> {
        pb::sandbox_service_server::SandboxServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::sandbox_service_server::SandboxService for SandboxServiceSvc {
    async fn exec(
        &self,
        request: Request<pb::ExecRequest>,
    ) -> Result<Response<pb::ExecResult>, Status> {
        let sp = span("sandbox.exec", request.metadata());
        let inner = self.inner.clone();
        async move {
            let spec: agent_core::ExecSpec = request.into_inner().into();
            let out = inner.exec(&spec).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(out.into()))
        }
        .instrument(sp)
        .await
    }

    async fn capabilities(
        &self,
        request: Request<pb::ExecCapabilitiesRequest>,
    ) -> Result<Response<pb::ExecCapabilities>, Status> {
        let sp = span("sandbox.capabilities", request.metadata());
        let caps = self.inner.capabilities();
        async move { Ok(Response::new(caps.into())) }
            .instrument(sp)
            .await
    }
}

pub fn sandbox_router(inner: Arc<dyn Sandbox>) -> Router {
    Server::builder().add_service(SandboxServiceSvc::new(inner).into_server())
}

pub struct PtyServiceSvc {
    inner: Arc<dyn Pty>,
}

impl PtyServiceSvc {
    pub fn new(inner: Arc<dyn Pty>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::pty_service_server::PtyServiceServer<Self> {
        pb::pty_service_server::PtyServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::pty_service_server::PtyService for PtyServiceSvc {
    async fn open(
        &self,
        request: Request<pb::PtyOpenRequest>,
    ) -> Result<Response<pb::PtySessionRef>, Status> {
        let sp = span("pty.open", request.metadata());
        let inner = self.inner.clone();
        async move {
            let spec: agent_core::PtySpec = request.into_inner().into();
            let id = inner.open(&spec).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PtySessionRef { id }))
        }
        .instrument(sp)
        .await
    }

    async fn write(
        &self,
        request: Request<pb::PtyWriteRequest>,
    ) -> Result<Response<pb::PtyWriteResponse>, Status> {
        let sp = span("pty.write", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            inner
                .write(&req.id, &req.input)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PtyWriteResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn read(
        &self,
        request: Request<pb::PtyReadRequest>,
    ) -> Result<Response<pb::PtyReadResponse>, Status> {
        let sp = span("pty.read", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let out = inner
                .read(&req.id, req.cursor)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PtyReadResponse {
                data: out.data,
                next_cursor: out.next_cursor,
                dropped: out.dropped,
                state: Some(out.state.into()),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn resize(
        &self,
        request: Request<pb::PtyResizeRequest>,
    ) -> Result<Response<pb::PtyResizeResponse>, Status> {
        let sp = span("pty.resize", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            // Clamped in the conversion layer; a 0- or 60000-column ioctl is
            // nonsense and 0 rows can wedge a child.
            let (cols, rows) = (
                req.cols.clamp(1, 1000) as u16,
                req.rows.clamp(1, 1000) as u16,
            );
            inner
                .resize(&req.id, cols, rows)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PtyResizeResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn close(
        &self,
        request: Request<pb::PtySessionRef>,
    ) -> Result<Response<pb::PtyCloseResponse>, Status> {
        let sp = span("pty.close", request.metadata());
        let inner = self.inner.clone();
        async move {
            let closed = inner
                .close(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PtyCloseResponse { closed }))
        }
        .instrument(sp)
        .await
    }

    async fn list(
        &self,
        request: Request<pb::PtyListRequest>,
    ) -> Result<Response<pb::PtySessionList>, Status> {
        let sp = span("pty.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let sessions = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PtySessionList {
                sessions: sessions.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(
        &self,
        request: Request<pb::PtySessionRef>,
    ) -> Result<Response<pb::PtySessionInfo>, Status> {
        let sp = span("pty.get", request.metadata());
        let inner = self.inner.clone();
        async move {
            let info = inner
                .get(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(info.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn pty_router(inner: Arc<dyn Pty>) -> Router {
    Server::builder().add_service(PtyServiceSvc::new(inner).into_server())
}
