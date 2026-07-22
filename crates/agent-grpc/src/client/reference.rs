//! The `ReferenceResolver` seam over the wire.
//!
//! **Failure semantic: degrade, never fail.** `resolve` has no error channel by
//! design — one bad `@`-mention must not fail the turn — so an unreachable
//! resolver returns an empty resolution carrying a warning. The warning is what
//! reaches the operator; the prompt is simply left unexpanded, which is exactly
//! what happens locally when a reference cannot be resolved.

use agent_core::{ReferenceResolver, Resolution};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `ReferenceResolver` that calls a remote `ReferenceService`.
pub struct GrpcReference {
    client: pb::reference_service_client::ReferenceServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcReference {
    pub fn connect(endpoint: &Endpoint) -> agent_core::Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::reference_service_client::ReferenceServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl ReferenceResolver for GrpcReference {
    async fn resolve(&self, prompt: &str, budget_tokens: usize) -> Resolution {
        let req = pb::RefResolveRequest {
            prompt: prompt.to_string(),
            budget_tokens: budget_tokens as u64,
        };
        // A pure read with no side effects, so a transient blip is worth retrying
        // before degrading to an unexpanded prompt.
        let out = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.resolve(outbound(r)).await }
        })
        .await;

        match out {
            Ok(resp) => resp.into_inner().into(),
            Err(status) => {
                tracing::warn!(
                    target: "reference.transport_failed",
                    code = ?status.code(),
                    "reference resolver unreachable; leaving the prompt unexpanded"
                );
                // NOT `blocked`: blocking means "over the hard budget, leave the
                // prompt alone deliberately". An outage is a degraded expansion
                // with a warning, which is how a local resolver reports a
                // reference it could not read.
                Resolution {
                    blocks: Vec::new(),
                    warnings: vec![format!("reference resolver unreachable: {status}")],
                    blocked: false,
                }
            }
        }
    }
}
