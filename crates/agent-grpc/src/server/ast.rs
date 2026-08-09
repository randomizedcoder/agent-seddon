//! The `AstBackend` seam as a service — the type-aware code-graph verbs
//! (callers/callees/callchain, implementations/interface-of, blast radius, package
//! dependency path), plus the server-streaming `Reindex` that bridges the core
//! callback-style progress fn to a stream. Fail-soft like the seam: an engine that
//! can't build a graph surfaces `Error::Ast` → `tonic::Status`, never a panic.

use std::pin::Pin;
use std::sync::Arc;

use agent_core::AstBackend;
use agent_proto::{pb, status_from_error};
use futures_util::Stream;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{missing, span};

pub struct AstServiceSvc {
    inner: Arc<dyn AstBackend>,
}

impl AstServiceSvc {
    pub fn new(inner: Arc<dyn AstBackend>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::ast_service_server::AstServiceServer<Self> {
        pb::ast_service_server::AstServiceServer::new(self)
    }
    /// The served backend's name — the label echoed on responses + progress.
    fn label(&self) -> String {
        self.inner.capabilities().backend
    }
}

#[tonic::async_trait]
impl pb::ast_service_server::AstService for AstServiceSvc {
    async fn status(
        &self,
        request: Request<pb::AstStatusRequest>,
    ) -> Result<Response<pb::AstStatusResponse>, Status> {
        let sp = span("ast.status", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let status = inner.status().await.map_err(|e| status_from_error(&e))?;
            let mut pb_status = pb::IndexStatus::from(status);
            pb_status.backend = label;
            Ok(Response::new(pb::AstStatusResponse {
                backends: vec![pb_status],
            }))
        }
        .instrument(sp)
        .await
    }

    async fn capabilities(
        &self,
        request: Request<pb::AstCapabilitiesRequest>,
    ) -> Result<Response<pb::AstCapabilitiesResponse>, Status> {
        let _sp = span("ast.capabilities", request.metadata()).entered();
        let caps = pb::AstCapabilities::from(self.inner.capabilities());
        Ok(Response::new(pb::AstCapabilitiesResponse {
            backends: vec![caps],
        }))
    }

    type ReindexStream = Pin<Box<dyn Stream<Item = Result<pb::ReindexProgress, Status>> + Send>>;

    #[allow(clippy::result_large_err)]
    async fn reindex(
        &self,
        request: Request<pb::AstReindexRequest>,
    ) -> Result<Response<Self::ReindexStream>, Status> {
        let sp = span("ast.reindex", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let tx_progress = tx.clone();
                let progress = move |p: agent_core::ReindexProgress| {
                    let mut pp = pb::ReindexProgress::from(p);
                    pp.backend.clone_from(&label);
                    let _ = tx_progress.send(Ok(pp));
                };
                if let Err(e) = inner.reindex(&progress).await {
                    let _ = tx.send(Err(status_from_error(&e)));
                }
            });
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            Ok(Response::new(Box::pin(stream) as Self::ReindexStream))
        }
        .instrument(sp)
        .await
    }

    async fn find_symbol(
        &self,
        request: Request<pb::FindSymbolRequest>,
    ) -> Result<Response<pb::SymbolList>, Status> {
        let sp = span("ast.find_symbol", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let q = agent_core::SymbolQuery::from(request.into_inner());
            let syms = inner
                .find_symbol(&q)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(symbol_list(syms, label)))
        }
        .instrument(sp)
        .await
    }

    async fn implementations(
        &self,
        request: Request<pb::ImplementationsRequest>,
    ) -> Result<Response<pb::SymbolList>, Status> {
        let sp = span("ast.implementations", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let iface = req
                .iface
                .ok_or_else(|| missing("ImplementationsRequest.iface"))?
                .into();
            let syms = inner
                .implementations(&iface)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(symbol_list(syms, label)))
        }
        .instrument(sp)
        .await
    }

    async fn interface_of(
        &self,
        request: Request<pb::InterfaceOfRequest>,
    ) -> Result<Response<pb::SymbolList>, Status> {
        let sp = span("ast.interface_of", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let ty = req
                .concrete_type
                .ok_or_else(|| missing("InterfaceOfRequest.concrete_type"))?
                .into();
            let syms = inner
                .interface_of(&ty)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(symbol_list(syms, label)))
        }
        .instrument(sp)
        .await
    }

    async fn callers(
        &self,
        request: Request<pb::CallersRequest>,
    ) -> Result<Response<pb::AstCallGraph>, Status> {
        let sp = span("ast.callers", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let target = req
                .target
                .ok_or_else(|| missing("CallersRequest.target"))?
                .into();
            let g = inner
                .callers(&target, req.hops)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(call_graph(g, label)))
        }
        .instrument(sp)
        .await
    }

    async fn callees(
        &self,
        request: Request<pb::CalleesRequest>,
    ) -> Result<Response<pb::AstCallGraph>, Status> {
        let sp = span("ast.callees", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let target = req
                .target
                .ok_or_else(|| missing("CalleesRequest.target"))?
                .into();
            let g = inner
                .callees(&target, req.hops)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(call_graph(g, label)))
        }
        .instrument(sp)
        .await
    }

    async fn callchain(
        &self,
        request: Request<pb::CallchainRequest>,
    ) -> Result<Response<pb::CallchainResponse>, Status> {
        let sp = span("ast.callchain", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let from = req
                .from
                .ok_or_else(|| missing("CallchainRequest.from"))?
                .into();
            let to = req.to.ok_or_else(|| missing("CallchainRequest.to"))?.into();
            let paths = inner
                .callchain(&from, &to, req.max_paths)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::CallchainResponse {
                paths: paths.into_iter().map(Into::into).collect(),
                backend: label,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn blast_radius(
        &self,
        request: Request<pb::BlastRadiusRequest>,
    ) -> Result<Response<pb::AstCallGraph>, Status> {
        let sp = span("ast.blast_radius", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let g = inner
                .blast_radius(&req.changed, req.hops)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(call_graph(g, label)))
        }
        .instrument(sp)
        .await
    }

    async fn dependency_path(
        &self,
        request: Request<pb::DependencyPathRequest>,
    ) -> Result<Response<pb::DependencyPathResponse>, Status> {
        let sp = span("ast.dependency_path", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let packages = inner
                .dependency_path(&req.from_package, &req.to_package)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::DependencyPathResponse {
                packages,
                backend: label,
            }))
        }
        .instrument(sp)
        .await
    }
}

fn symbol_list(syms: Vec<agent_core::Symbol>, backend: String) -> pb::SymbolList {
    pb::SymbolList {
        symbols: syms.into_iter().map(Into::into).collect(),
        backend,
    }
}

fn call_graph(g: agent_core::AstCallGraph, backend: String) -> pb::AstCallGraph {
    let mut pg = pb::AstCallGraph::from(g);
    pg.backend = backend;
    pg
}

pub fn ast_router(inner: Arc<dyn AstBackend>) -> Router {
    Server::builder().add_service(AstServiceSvc::new(inner).into_server())
}
