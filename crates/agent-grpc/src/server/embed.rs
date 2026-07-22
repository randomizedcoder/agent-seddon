//! The `Embedder` seam as a service — text embedding behind gRPC.

use std::sync::Arc;

use agent_core::Embedder;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct EmbedServiceSvc {
    inner: Arc<dyn Embedder>,
}

impl EmbedServiceSvc {
    pub fn new(inner: Arc<dyn Embedder>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::embed_service_server::EmbedServiceServer<Self> {
        pb::embed_service_server::EmbedServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::embed_service_server::EmbedService for EmbedServiceSvc {
    async fn capabilities(
        &self,
        request: Request<pb::EmbCapabilitiesRequest>,
    ) -> Result<Response<pb::EmbCapabilities>, Status> {
        let sp = span("embed.capabilities", request.metadata());
        let (dimensions, max_batch) = (self.inner.dimensions(), self.inner.max_batch());
        async move {
            Ok(Response::new(pb::EmbCapabilities {
                dimensions: dimensions as u32,
                max_batch: max_batch as u32,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn embed_query(
        &self,
        request: Request<pb::EmbQueryRequest>,
    ) -> Result<Response<pb::EmbVector>, Status> {
        let sp = span("embed.query", request.metadata());
        let inner = self.inner.clone();
        async move {
            let v = inner
                .embed_query(&request.into_inner().text)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::EmbVector { values: v }))
        }
        .instrument(sp)
        .await
    }

    async fn embed_docs(
        &self,
        request: Request<pb::EmbDocsRequest>,
    ) -> Result<Response<pb::EmbVectors>, Status> {
        let sp = span("embed.docs", request.metadata());
        let inner = self.inner.clone();
        async move {
            let vs = inner
                .embed_docs(&request.into_inner().texts)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::EmbVectors {
                vectors: vs
                    .into_iter()
                    .map(|values| pb::EmbVector { values })
                    .collect(),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn embed_router(inner: Arc<dyn Embedder>) -> Router {
    Server::builder().add_service(EmbedServiceSvc::new(inner).into_server())
}
