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

/// The **episodic layer** over the wire: the append-only "what happened" log.
///
/// Split from [`GrpcMemory`] so the durable log can live on a different host
/// from semantic recall — which is the reason the layers are separate traits in
/// the first place.
///
/// **Failure semantic: hard.** An append that quietly no-ops loses the turn from
/// the log; a `recent` that quietly returns nothing makes distillation promote
/// nothing and look like it worked.
pub struct GrpcEpisodic {
    client: pb::episodic_client::EpisodicClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcEpisodic {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::episodic_client::EpisodicClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl agent_core::EpisodicStore for GrpcEpisodic {
    async fn append(&self, event: MemoryEvent) -> Result<()> {
        let req = pb::MemoryEvent::from(event);
        // NOT retried: the log is append-only, so a retry after a lost response
        // writes the event twice. A duplicated turn in the history is not
        // catastrophic, but it is a silent corruption of the record, and
        // `recent` feeds distillation.
        let mut client = self.client.clone();
        client
            .append(outbound(req))
            .await
            .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(())
    }

    async fn recent(&self, limit: usize) -> Result<Vec<MemoryEvent>> {
        let req = pb::RecentRequest {
            limit: limit as u64,
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req;
            async move { client.recent(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        resp.into_inner()
            .events
            .into_iter()
            .map(TryInto::try_into)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Memory(e.to_string()))
    }
}

/// The **semantic layer** over the wire: relevance recall plus promotion.
///
/// This is the layer worth putting on its own host — it is where a vector store
/// and its index live.
///
/// **Failure semantic: hard.** An empty recall on failure is indistinguishable
/// from "nothing relevant is known", and the model would proceed as if it had
/// checked.
pub struct GrpcSemantic {
    client: pb::semantic_client::SemanticClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcSemantic {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::semantic_client::SemanticClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl agent_core::SemanticStore for GrpcSemantic {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        let req = pb::RecallQuery::from(query.clone());
        // A pure read, so retrying a blip is free.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
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

    async fn distill(&self, episodic: &[MemoryEvent]) -> Result<usize> {
        let req = pb::SemanticDistillRequest {
            episodic: episodic.iter().cloned().map(Into::into).collect(),
        };
        // NOT retried: distillation WRITES facts. A retry after a lost response
        // promotes the same window twice, and the returned count would describe
        // only the second pass.
        let mut client = self.client.clone();
        let resp = client
            .distill(outbound(req))
            .await
            .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(usize::try_from(resp.into_inner().count).unwrap_or(usize::MAX))
    }
}
