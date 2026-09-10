//! The `ForgeRegistryService` seam (config design C36 / increment D1): the forge
//! control plane — CRUD over the persisted **forge cards** the fleet + in-loop
//! review paths build their git-host clients from, on top of the built-in host
//! impls. This is the *config-card registry* for forges; the git-host *capability*
//! seam is `agent.v1.Forge` (`server::forge`), the way `ProviderRegistryService`
//! (cards) relates to the `LlmProvider` seam.
//!
//! Fails **hard** (`Err` → `Status`), like the other control-plane seams: a failed
//! mutation surfaces to the operator, never silently degrades. Every id, kind,
//! `token_ref`, and `repo_encoding` is untrusted and validated inside the store and
//! the wire→core boundary (`safe_segment`, `ApiKeyRef::parse`, `RepoEncoding::parse`)
//! — a rejection maps to `InvalidArgument`, and a store's `not found` to `NotFound`,
//! via `status_from_error`. Mutations are gated by the RBAC enforcement core (C1):
//! a caller needs a role granting `(write|delete, forge_registry)`.

use std::sync::Arc;

use agent_core::ForgeRegistry;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ForgeRegistrySvc {
    inner: Arc<dyn ForgeRegistry>,
}

impl ForgeRegistrySvc {
    pub fn new(inner: Arc<dyn ForgeRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::forge_registry_service_server::ForgeRegistryServiceServer<Self> {
        pb::forge_registry_service_server::ForgeRegistryServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::forge_registry_service_server::ForgeRegistryService for ForgeRegistrySvc {
    async fn list(
        &self,
        request: Request<pb::ForgeListRequest>,
    ) -> Result<Response<pb::ForgeList>, Status> {
        let sp = span("forge_registry.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let cards = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ForgeList {
                forges: cards.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(&self, request: Request<pb::ForgeRef>) -> Result<Response<pb::ForgeCard>, Status> {
        let sp = span("forge_registry.get", request.metadata());
        let inner = self.inner.clone();
        async move {
            let card = inner
                .get(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(card.into()))
        }
        .instrument(sp)
        .await
    }

    async fn put(
        &self,
        request: Request<pb::ForgeCard>,
    ) -> Result<Response<pb::ForgeCard>, Status> {
        super::authz::require(
            agent_core::Action::Write,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let sp = span("forge_registry.put", request.metadata());
        let inner = self.inner.clone();
        async move {
            // Wire → core is the fail-closed boundary (unknown/absent repo_encoding);
            // the store validates again (id/kind/token_ref) before it persists.
            let card = agent_core::ForgeCard::try_from(request.into_inner())
                .map_err(tonic::Status::from)?;
            let stored = inner.put(card).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn delete(
        &self,
        request: Request<pb::ForgeRef>,
    ) -> Result<Response<pb::ForgeDeleteReply>, Status> {
        super::authz::require(
            agent_core::Action::Delete,
            agent_core::ResourceType::ForgeRegistry,
        )?;
        let sp = span("forge_registry.delete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ForgeDeleteReply { deleted }))
        }
        .instrument(sp)
        .await
    }
}

pub fn forge_registry_router(inner: Arc<dyn ForgeRegistry>) -> Router {
    Server::builder().add_service(ForgeRegistrySvc::new(inner).into_server())
}
