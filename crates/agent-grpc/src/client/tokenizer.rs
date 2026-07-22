//! The `Tokenizer` seam over the wire.
//!
//! **Failure semantic: hard.** A count is an input to budget and compaction
//! decisions; a fabricated one on failure would silently mis-size the context
//! window. Callers already have a heuristic fallback for "no tokenizer wired" —
//! an `Err` lets them choose it knowingly.

use agent_core::{Message, Result, Tokenizer};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

/// A `Tokenizer` that calls a remote `TokenizerService`.
///
/// Every agent dialling one tokenizer produces identical counts, so budget
/// decisions stay consistent across a fleet instead of drifting with whichever
/// backend each was built with.
pub struct GrpcTokenizer {
    client: pb::tokenizer_service_client::TokenizerServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcTokenizer {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::tokenizer_service_client::TokenizerServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

fn err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Tokenizer(s.to_string())
}

#[async_trait]
impl Tokenizer for GrpcTokenizer {
    fn backend(&self) -> &str {
        // A sync accessor can't round-trip; this names the transport. The server
        // labels its own spans with the real backend.
        "grpc"
    }

    async fn count(&self, text: &str, model: &str) -> Result<u32> {
        let req = pb::TokCountRequest {
            text: text.to_string(),
            model: model.to_string(),
        };
        // Pure and side-effect free, so retrying a blip is free.
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.count(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().tokens)
    }

    /// Overridden rather than left to the trait default: the default counts each
    /// field with a separate `count` call, which over a network would be one
    /// round trip *per message field* on the loop's hot path. One call instead.
    async fn count_messages(&self, messages: &[Message], model: &str) -> Result<u32> {
        let req = pb::TokCountMessagesRequest {
            messages: messages.iter().cloned().map(Into::into).collect(),
            model: model.to_string(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.count_messages(outbound(r)).await }
        })
        .await
        .map_err(err)?;
        Ok(resp.into_inner().tokens)
    }
}
