//! The `DimensionStore` seam over the wire. Fails **soft**: a transport error
//! yields an empty result (no summaries / no recall), never a loop failure —
//! dimensional memory is a best-effort enrichment layer.

use agent_core::{DimensionStore, DimensionSummary, MemoryEvent, MemoryItem, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcDimensions {
    client: pb::dimension_service_client::DimensionServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcDimensions {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::dimension_service_client::DimensionServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl DimensionStore for GrpcDimensions {
    async fn summarize_step(&self, events: &[MemoryEvent]) -> Result<Vec<DimensionSummary>> {
        let req = pb::SummarizeRequest {
            events: events.iter().cloned().map(Into::into).collect(),
        };
        let res = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.summarize(outbound(r)).await }
        })
        .await;
        match res {
            Ok(resp) => Ok(resp
                .into_inner()
                .summaries
                .into_iter()
                .map(Into::into)
                .collect()),
            Err(s) => {
                tracing::warn!("dimension summarize RPC failed ({s}); skipping step");
                Ok(Vec::new())
            }
        }
    }

    async fn recall_dimension(&self, dimension: &str, limit: usize) -> Result<Vec<MemoryItem>> {
        let req = pb::DimensionRecallRequest {
            dimension: dimension.to_string(),
            limit: limit as u32,
        };
        let res = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.recall(outbound(r)).await }
        })
        .await;
        match res {
            Ok(resp) => Ok(resp
                .into_inner()
                .items
                .into_iter()
                .map(Into::into)
                .collect()),
            Err(s) => {
                tracing::warn!("dimension recall RPC failed ({s}); returning empty");
                Ok(Vec::new())
            }
        }
    }
}
