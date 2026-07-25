//! The `MetricsProxy` seam as a service (generic PromQL over gRPC).
//!
//! Fails **soft**: the backing proxy folds every upstream failure into a
//! `PromResult` with a class-only `error`, so the RPC returns `Ok` with that
//! shape rather than a `Status` — the client always gets something to render.

use std::sync::Arc;

use agent_core::MetricsProxy;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct MetricsProxySvc {
    inner: Arc<dyn MetricsProxy>,
}

impl MetricsProxySvc {
    pub fn new(inner: Arc<dyn MetricsProxy>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::metrics_proxy_service_server::MetricsProxyServiceServer<Self> {
        pb::metrics_proxy_service_server::MetricsProxyServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::metrics_proxy_service_server::MetricsProxyService for MetricsProxySvc {
    async fn query(
        &self,
        request: Request<pb::PromQuery>,
    ) -> Result<Response<pb::PromResult>, Status> {
        let sp = span("metrics.query", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q = request.into_inner().into();
            let res = inner.query(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(res.into()))
        }
        .instrument(sp)
        .await
    }

    async fn query_range(
        &self,
        request: Request<pb::PromRangeQuery>,
    ) -> Result<Response<pb::PromResult>, Status> {
        let sp = span("metrics.query_range", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q = request.into_inner().into();
            let res = inner
                .query_range(&q)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(res.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn metrics_proxy_router(inner: Arc<dyn MetricsProxy>) -> Router {
    Server::builder().add_service(MetricsProxySvc::new(inner).into_server())
}
