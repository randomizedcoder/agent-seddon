//! The `ProviderRegistry` seam over the wire (model-router 03). Fails **hard**:
//! a transport or validation error surfaces as an `Err` rather than degrading —
//! a control-plane read/write the operator asked for should not silently no-op.
//! Everything a remote registry returns is untrusted: the shared wire→core
//! decode clamps every number before a card reaches selection math.

use agent_core::{
    Error, ProviderRegistry, Result, RouteDecision, RouteHint, RoutePolicySpec, Upstream,
    UpstreamHealth,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcRegistry {
    client: pb::provider_registry_service_client::ProviderRegistryServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcRegistry {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| Error::Registry(e.to_string()))?;
        Ok(Self {
            client: pb::provider_registry_service_client::ProviderRegistryServiceClient::new(
                channel,
            ),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl ProviderRegistry for GrpcRegistry {
    async fn list(&self) -> Result<Vec<Upstream>> {
        let resp = unary!(self, list, pb::UpstreamListRequest {}).map_err(status_to_err)?;
        Ok(resp
            .into_inner()
            .upstreams
            .into_iter()
            .map(Upstream::from) // decode clamps hostile numbers
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Upstream> {
        let req = pb::UpstreamRef { id: id.to_string() };
        let resp = unary!(self, get, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn put(&self, card: Upstream) -> Result<Upstream> {
        let req = pb::Upstream::from(card);
        let resp = unary!(self, put, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let req = pb::UpstreamRef { id: id.to_string() };
        let resp = unary!(self, delete, req).map_err(status_to_err)?;
        Ok(resp.into_inner().deleted)
    }

    async fn enable(&self, id: &str, enabled: bool) -> Result<Upstream> {
        let req = pb::UpstreamEnableRequest {
            id: id.to_string(),
            enabled,
        };
        let resp = unary!(self, enable, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn get_policy(&self) -> Result<RoutePolicySpec> {
        let resp = unary!(self, get_policy, pb::RoutePolicyRef {}).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn put_policy(&self, policy: RoutePolicySpec) -> Result<RoutePolicySpec> {
        let req = pb::RoutePolicy::from(policy);
        let resp = unary!(self, put_policy, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn route(&self, hint: &RouteHint) -> Result<RouteDecision> {
        let req = pb::RouteRequest {
            hint: Some(pb::RouteHint::from(hint.clone())),
        };
        let resp = unary!(self, route, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn health(&self) -> Result<Vec<UpstreamHealth>> {
        let resp = unary!(self, health, pb::UpstreamHealthRequest {}).map_err(status_to_err)?;
        Ok(resp
            .into_inner()
            .entries
            .into_iter()
            .map(UpstreamHealth::from)
            .collect())
    }
}

fn status_to_err(s: tonic::Status) -> Error {
    // Preserve the seam's `not found` contract across the wire, so a chained
    // registry (grpc → grpc) still maps to NotFound at the outer hop.
    let m = s.message();
    if s.code() == tonic::Code::NotFound {
        Error::Registry(format!(
            "not found: {}",
            m.strip_prefix("registry: not found: ").unwrap_or(m)
        ))
    } else {
        Error::Registry(m.to_string())
    }
}
