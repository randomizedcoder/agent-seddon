//! The `MemoryStore` seam over the wire.

use agent_core::{MemoryEvent, MemoryItem, MemoryStore, RecallQuery, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcMemory {
    client: pb::memory_client::MemoryClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcMemory {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Memory(e.to_string()))?;
        Ok(Self {
            client: pb::memory_client::MemoryClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl MemoryStore for GrpcMemory {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        let pbreq = pb::RecallQuery::from(query.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.recall(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(resp
            .into_inner()
            .items
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn append(&self, event: MemoryEvent) -> Result<()> {
        let pbreq = pb::MemoryEvent::from(event);
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.append(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(())
    }

    async fn distill(&self) -> Result<usize> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.distill(outbound(pb::DistillRequest {})).await }
        })
        .await
        .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(resp.into_inner().count as usize)
    }
}
