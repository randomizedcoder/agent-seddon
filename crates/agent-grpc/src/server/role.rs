//! The `RoleService` seam (config design C34 / increment C1b): the RBAC
//! control plane — CRUD over the operator-defined **role cards** that the gate
//! (`super::authz`) authorizes against, on top of the three immutable built-ins.
//!
//! Fails **hard** (`Err` → `Status`), like the other control-plane seams: a
//! failed mutation surfaces to the operator, never silently degrades. Every id,
//! action string, and resource-type string is untrusted and validated inside the
//! store and the wire→core boundary (`safe_segment`, reserved-id rejection,
//! `Action`/`ResourceType` parse) — a rejection maps to `InvalidArgument`, and a
//! store's `not found` to `NotFound`, via `status_from_error`.
//!
//! **The catalog snapshot.** The gate reads an ambient
//! [`agent_core::current_catalog`] (built-ins ∪ persisted cards). After every
//! successful `Put`/`Delete` this seam rebuilds that snapshot from the store and
//! [`agent_core::install_catalog`]s it, so a role edit takes effect for the very
//! next request — within this process (cross-process propagation is a later lift).

use std::sync::Arc;

use agent_core::{install_catalog, load_catalog, RoleRegistry};
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct RoleSvc {
    inner: Arc<dyn RoleRegistry>,
}

impl RoleSvc {
    pub fn new(inner: Arc<dyn RoleRegistry>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::role_service_server::RoleServiceServer<Self> {
        pb::role_service_server::RoleServiceServer::new(self)
    }

    /// Rebuild the live catalog snapshot from the store and install it, so the gate
    /// sees this write on the next request. A rebuild failure surfaces to the caller
    /// — the write already landed, but leaving a stale snapshot silently would be a
    /// fail-open, so we report it.
    async fn refresh_catalog(inner: &Arc<dyn RoleRegistry>) -> Result<(), Status> {
        let catalog = load_catalog(inner.as_ref())
            .await
            .map_err(|e| status_from_error(&e))?;
        install_catalog(catalog);
        Ok(())
    }
}

#[tonic::async_trait]
impl pb::role_service_server::RoleService for RoleSvc {
    async fn list(
        &self,
        request: Request<pb::RoleListRequest>,
    ) -> Result<Response<pb::RoleList>, Status> {
        let sp = span("role.list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let cards = inner.list().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::RoleList {
                roles: cards.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn get(&self, request: Request<pb::RoleRef>) -> Result<Response<pb::RoleCard>, Status> {
        let sp = span("role.get", request.metadata());
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

    async fn put(&self, request: Request<pb::RoleCard>) -> Result<Response<pb::RoleCard>, Status> {
        super::authz::require(agent_core::Action::Write, agent_core::ResourceType::Role)?;
        let sp = span("role.put", request.metadata());
        let inner = self.inner.clone();
        async move {
            // Wire → core is the fail-closed boundary (unknown action/resource string,
            // reserved/hostile id); the store validates again before it persists.
            let card = agent_core::RoleCard::try_from(request.into_inner())
                .map_err(tonic::Status::from)?;
            let stored = inner.put(card).await.map_err(|e| status_from_error(&e))?;
            Self::refresh_catalog(&inner).await?;
            Ok(Response::new(stored.into()))
        }
        .instrument(sp)
        .await
    }

    async fn delete(
        &self,
        request: Request<pb::RoleRef>,
    ) -> Result<Response<pb::RoleDeleteReply>, Status> {
        super::authz::require(agent_core::Action::Delete, agent_core::ResourceType::Role)?;
        let sp = span("role.delete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let deleted = inner
                .delete(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Self::refresh_catalog(&inner).await?;
            Ok(Response::new(pb::RoleDeleteReply { deleted }))
        }
        .instrument(sp)
        .await
    }
}

pub fn role_router(inner: Arc<dyn RoleRegistry>) -> Router {
    Server::builder().add_service(RoleSvc::new(inner).into_server())
}
