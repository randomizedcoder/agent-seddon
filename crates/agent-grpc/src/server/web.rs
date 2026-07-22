//! The `WebBackend` and `WebSearch` seams as services — the agent's egress
//! behind gRPC.
//!
//! Hosting these puts every outbound request in one process: search API keys
//! live there rather than in every agent, and the SSRF screen becomes a property
//! of the network position rather than of each agent's config.

use std::sync::Arc;

use agent_core::{WebBackend, WebSearch};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct WebServiceSvc {
    inner: Arc<dyn WebBackend>,
}

impl WebServiceSvc {
    pub fn new(inner: Arc<dyn WebBackend>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::web_service_server::WebServiceServer<Self> {
        pb::web_service_server::WebServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::web_service_server::WebService for WebServiceSvc {
    async fn fetch(
        &self,
        request: Request<pb::WebFetchRequest>,
    ) -> Result<Response<pb::WebFetchResponse>, Status> {
        let sp = span("web.fetch", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req: agent_core::WebRequest = request.into_inner().into();
            let resp = inner.fetch(&req).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(resp.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn web_router(inner: Arc<dyn WebBackend>) -> Router {
    Server::builder().add_service(WebServiceSvc::new(inner).into_server())
}

pub struct WebSearchServiceSvc {
    inner: Arc<dyn WebSearch>,
}

impl WebSearchServiceSvc {
    pub fn new(inner: Arc<dyn WebSearch>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::web_search_service_server::WebSearchServiceServer<Self> {
        pb::web_search_service_server::WebSearchServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::web_search_service_server::WebSearchService for WebSearchServiceSvc {
    async fn search(
        &self,
        request: Request<pb::WebSearchRequest>,
    ) -> Result<Response<pb::WebSearchResponse>, Status> {
        let sp = span("web_search.search", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q: agent_core::WebQuery = request.into_inner().into();
            let results = inner.search(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::WebSearchResponse {
                results: results.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn status(
        &self,
        request: Request<pb::WebSearchRequest>,
    ) -> Result<Response<pb::WebCacheStatus>, Status> {
        let sp = span("web_search.status", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q: agent_core::WebQuery = request.into_inner().into();
            let state = inner.status(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::WebCacheStatus {
                state: pb::WebCacheState::from(state) as i32,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn capabilities(
        &self,
        request: Request<pb::WebSearchCapabilitiesRequest>,
    ) -> Result<Response<pb::WebSearchCapabilities>, Status> {
        let sp = span("web_search.capabilities", request.metadata());
        let caps = self.inner.capabilities();
        async move { Ok(Response::new(caps.into())) }
            .instrument(sp)
            .await
    }
}

pub fn web_search_router(inner: Arc<dyn WebSearch>) -> Router {
    Server::builder().add_service(WebSearchServiceSvc::new(inner).into_server())
}
