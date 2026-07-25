//! The `MetricsProxy` seam over the wire. Fails **soft** end to end: a transport
//! error becomes a `PromResult` with a class-only `error`, matching the seam's
//! read-only, never-Err contract.

use agent_core::{MetricsProxy, PromQuery, PromRangeQuery, PromResult, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcMetricsProxy {
    client: pb::metrics_proxy_service_client::MetricsProxyServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcMetricsProxy {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::metrics_proxy_service_client::MetricsProxyServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl MetricsProxy for GrpcMetricsProxy {
    async fn query(&self, q: &PromQuery) -> Result<PromResult> {
        let req = pb::PromQuery::from(q.clone());
        let res = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.query(outbound(r)).await }
        })
        .await;
        Ok(soft(res))
    }

    async fn query_range(&self, q: &PromRangeQuery) -> Result<PromResult> {
        let req = pb::PromRangeQuery::from(q.clone());
        let res = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.query_range(outbound(r)).await }
        })
        .await;
        Ok(soft(res))
    }
}

/// Fold a transport outcome into a `PromResult` — the response on success, else a
/// class-only error shape (the seam never surfaces a transport `Err`).
fn soft(res: std::result::Result<tonic::Response<pb::PromResult>, tonic::Status>) -> PromResult {
    match res {
        Ok(resp) => resp.into_inner().into(),
        Err(s) => {
            tracing::warn!("metrics-proxy RPC failed ({s}); returning empty");
            PromResult {
                error: "proxy transport failure".into(),
                ..Default::default()
            }
        }
    }
}
