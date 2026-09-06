//! The `FleetRegistry` seam over the wire (review-fleet C3). Fails **hard**: a
//! transport or validation error surfaces as an `Err` rather than degrading — a
//! control-plane read/write the operator asked for should not silently no-op.
//! Everything a remote roster returns is untrusted: the shared wire→core decode
//! clamps every number before a row is used. `token_ref` rides as a reference;
//! nothing here resolves it.

use agent_core::{Error, FleetRegistry, FleetSession, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcFleet {
    client: pb::review_fleet_service_client::ReviewFleetServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcFleet {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| Error::Fleet(e.to_string()))?;
        Ok(Self {
            client: pb::review_fleet_service_client::ReviewFleetServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl FleetRegistry for GrpcFleet {
    async fn list(&self) -> Result<Vec<FleetSession>> {
        let resp = unary!(self, list, pb::FleetListRequest {}).map_err(status_to_err)?;
        Ok(resp
            .into_inner()
            .sessions
            .into_iter()
            .map(FleetSession::from) // decode clamps hostile numbers
            .collect())
    }

    async fn get(&self, id: &str) -> Result<FleetSession> {
        let req = pb::FleetSessionRef { id: id.to_string() };
        let resp = unary!(self, get, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn put(&self, session: FleetSession) -> Result<FleetSession> {
        let req = pb::FleetSession::from(session);
        let resp = unary!(self, put, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let req = pb::FleetSessionRef { id: id.to_string() };
        let resp = unary!(self, delete, req).map_err(status_to_err)?;
        Ok(resp.into_inner().deleted)
    }

    async fn set_enabled(&self, id: &str, enabled: bool) -> Result<FleetSession> {
        let req = pb::FleetSetEnabledRequest {
            id: id.to_string(),
            enabled,
        };
        let resp = unary!(self, set_enabled, req).map_err(status_to_err)?;
        Ok(resp.into_inner().into())
    }
}

fn status_to_err(s: tonic::Status) -> Error {
    // Preserve the seam's `not found` contract across the wire, so a chained
    // roster (grpc → grpc) still maps to NotFound at the outer hop.
    let m = s.message();
    if s.code() == tonic::Code::NotFound {
        Error::Fleet(format!(
            "not found: {}",
            m.strip_prefix("fleet: not found: ").unwrap_or(m)
        ))
    } else {
        Error::Fleet(m.to_string())
    }
}
