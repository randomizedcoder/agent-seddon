//! The `TaskClassifier` seam over the wire — a classifier dialed as a remote
//! `ModeService`. Fails **safe**: a transport error resolves to `TaskMode::Other`
//! (the normal loop), never a spurious switch.

use agent_core::{ClassifyCtx, ModeVerdict, Result, TaskClassifier, TaskMode};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcClassifier {
    client: pb::mode_service_client::ModeServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcClassifier {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::mode_service_client::ModeServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl TaskClassifier for GrpcClassifier {
    fn name(&self) -> &str {
        "grpc"
    }

    async fn classify(&self, ctx: &ClassifyCtx<'_>) -> ModeVerdict {
        let req = pb::ClassifyRequest {
            prompt: ctx.prompt.to_string(),
            history: ctx.history.iter().cloned().map(Into::into).collect(),
        };
        let res = unary!(self, classify, req);
        match res {
            Ok(resp) => resp.into_inner().into(),
            Err(s) => {
                tracing::warn!("mode classify RPC failed ({s}); failing safe to Other");
                ModeVerdict {
                    mode: TaskMode::Other,
                    confidence: 0.0,
                    reason: "grpc classify failed".into(),
                }
            }
        }
    }
}
