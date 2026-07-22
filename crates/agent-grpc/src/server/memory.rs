//! The `MemoryStore` seam as a service, plus the `Episodic` and `Semantic`
//! layer adapters that expose the same store at a finer grain.

use std::sync::Arc;

use agent_core::{EpisodicStore, MemoryStore, SemanticStore};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct MemoryService {
    inner: Arc<dyn MemoryStore>,
}

impl MemoryService {
    pub fn new(inner: Arc<dyn MemoryStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::memory_server::MemoryServer<Self> {
        pb::memory_server::MemoryServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::memory_server::Memory for MemoryService {
    async fn recall(
        &self,
        request: Request<pb::RecallQuery>,
    ) -> Result<Response<pb::RecallResponse>, Status> {
        let sp = span("memory.recall", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q = request.into_inner().into();
            let items = inner.recall(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::RecallResponse {
                items: items.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn append(
        &self,
        request: Request<pb::MemoryEvent>,
    ) -> Result<Response<pb::AppendResponse>, Status> {
        let sp = span("memory.append", request.metadata());
        let inner = self.inner.clone();
        async move {
            let event = request.into_inner().try_into()?;
            inner
                .append(event)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AppendResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn distill(
        &self,
        request: Request<pb::DistillRequest>,
    ) -> Result<Response<pb::DistillResponse>, Status> {
        let sp = span("memory.distill", request.metadata());
        let inner = self.inner.clone();
        async move {
            let count = inner.distill().await.map_err(|e| status_from_error(&e))? as u64;
            Ok(Response::new(pb::DistillResponse { count }))
        }
        .instrument(sp)
        .await
    }
}

pub struct EpisodicService {
    inner: Arc<dyn EpisodicStore>,
}

impl EpisodicService {
    pub fn new(inner: Arc<dyn EpisodicStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::episodic_server::EpisodicServer<Self> {
        pb::episodic_server::EpisodicServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::episodic_server::Episodic for EpisodicService {
    async fn append(
        &self,
        request: Request<pb::MemoryEvent>,
    ) -> Result<Response<pb::AppendResponse>, Status> {
        let sp = span("episodic.append", request.metadata());
        let inner = self.inner.clone();
        async move {
            let event = request.into_inner().try_into()?;
            inner
                .append(event)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AppendResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn recent(
        &self,
        request: Request<pb::RecentRequest>,
    ) -> Result<Response<pb::RecentResponse>, Status> {
        let sp = span("episodic.recent", request.metadata());
        let inner = self.inner.clone();
        async move {
            let limit = request.into_inner().limit as usize;
            let events = inner
                .recent(limit)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::RecentResponse {
                events: events.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub struct SemanticService {
    inner: Arc<dyn SemanticStore>,
}

impl SemanticService {
    pub fn new(inner: Arc<dyn SemanticStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::semantic_server::SemanticServer<Self> {
        pb::semantic_server::SemanticServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::semantic_server::Semantic for SemanticService {
    async fn recall(
        &self,
        request: Request<pb::RecallQuery>,
    ) -> Result<Response<pb::RecallResponse>, Status> {
        let sp = span("semantic.recall", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q = request.into_inner().into();
            let items = inner.recall(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::RecallResponse {
                items: items.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn distill(
        &self,
        request: Request<pb::SemanticDistillRequest>,
    ) -> Result<Response<pb::DistillResponse>, Status> {
        let sp = span("semantic.distill", request.metadata());
        let inner = self.inner.clone();
        async move {
            let episodic = request
                .into_inner()
                .episodic
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?;
            let count = inner
                .distill(&episodic)
                .await
                .map_err(|e| status_from_error(&e))? as u64;
            Ok(Response::new(pb::DistillResponse { count }))
        }
        .instrument(sp)
        .await
    }
}

pub fn memory_router(inner: Arc<dyn MemoryStore>) -> Router {
    Server::builder().add_service(MemoryService::new(inner).into_server())
}
