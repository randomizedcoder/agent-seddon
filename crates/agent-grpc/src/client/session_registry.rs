//! The `SessionRegistry` lifecycle seam over the wire
//! (docs/design/multi-session/05-lifecycle.md).
//!
//! **Failure semantic: hard** — a transport failure surfaces as `Err`. `open` is
//! **not retried**: it server-mints a fresh id on every call, so a retry after a lost
//! response would allocate a *second* orphaned session (the same reasoning as
//! `SessionStore::checkpoint`). `close`/`heartbeat` name an existing id and are
//! idempotent, so they use the shared retry.

use agent_core::{Error, Result, SessionId, SessionKey, SessionRegistry};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcSessionRegistry {
    client: pb::session_registry_service_client::SessionRegistryServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcSessionRegistry {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::session_registry_service_client::SessionRegistryServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

fn err(s: tonic::Status) -> Error {
    Error::Session(s.to_string())
}

#[async_trait]
impl SessionRegistry for GrpcSessionRegistry {
    async fn open(&self, user: &str) -> Result<SessionId> {
        let req = pb::OpenRequest {
            user: user.to_string(),
        };
        // NOT retried — each call mints a new id (a retry would orphan a second session).
        let mut client = self.client.clone();
        let resp = client.open(outbound(req)).await.map_err(err)?;
        Ok(SessionId::new(resp.into_inner().session_id))
    }

    async fn close(&self, key: &SessionKey) -> Result<()> {
        let req = pb::CloseRequest {
            user: key.user.as_str().to_string(),
            session_id: key.session.as_str().to_string(),
        };
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.close(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn heartbeat(&self, key: &SessionKey) -> Result<()> {
        let req = pb::HeartbeatRequest {
            user: key.user.as_str().to_string(),
            session_id: key.session.as_str().to_string(),
        };
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.heartbeat(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(())
    }
}
