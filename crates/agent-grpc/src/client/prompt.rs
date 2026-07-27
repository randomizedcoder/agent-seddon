//! The `PromptStore` seam over the wire. Fails **hard**: a transport or validation
//! error surfaces as an `Err` (mapped from `tonic::Status`) rather than degrading,
//! because a prompt read/write the operator asked for should not silently no-op.

use agent_core::{
    Error, Message, PromptContext, PromptEntry, PromptKind, PromptRef, PromptStore, Result,
};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcPrompts {
    client: pb::prompt_service_client::PromptServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcPrompts {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::prompt_service_client::PromptServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl PromptStore for GrpcPrompts {
    async fn list(&self, kind: Option<PromptKind>) -> Result<Vec<PromptEntry>> {
        let kind = kind
            .map(|k| pb::PromptKind::from(k) as i32)
            .unwrap_or(pb::PromptKind::Unspecified as i32);
        let req = pb::PromptListRequest { kind };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            // `PromptListRequest` is `Copy` (a single enum tag).
            let r = req;
            async move { client.list(outbound(r)).await }
        })
        .await
        .map_err(status_to_err)?;
        resp.into_inner()
            .entries
            .into_iter()
            .map(|e| e.try_into().map_err(convert_err))
            .collect()
    }

    async fn get(&self, r: &PromptRef) -> Result<PromptEntry> {
        let req = pb::PromptRef::from(r.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.get(outbound(r)).await }
        })
        .await
        .map_err(status_to_err)?;
        resp.into_inner().try_into().map_err(convert_err)
    }

    async fn put(&self, entry: PromptEntry) -> Result<PromptEntry> {
        let req = pb::PromptEntry::from(entry);
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.put(outbound(r)).await }
        })
        .await
        .map_err(status_to_err)?;
        resp.into_inner().try_into().map_err(convert_err)
    }

    async fn delete(&self, r: &PromptRef) -> Result<bool> {
        let req = pb::PromptRef::from(r.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.delete(outbound(r)).await }
        })
        .await
        .map_err(status_to_err)?;
        Ok(resp.into_inner().deleted)
    }

    async fn select(&self, ctx: &PromptContext) -> Result<Vec<PromptEntry>> {
        let req = pb::PromptContext::from(ctx.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.select(outbound(r)).await }
        })
        .await
        .map_err(status_to_err)?;
        resp.into_inner()
            .entries
            .into_iter()
            .map(|e| e.try_into().map_err(convert_err))
            .collect()
    }

    async fn preview_assembled(&self, ctx: &PromptContext, goal: &str) -> Result<Vec<Message>> {
        let req = pb::PreviewRequest {
            mode: String::new(),
            goal: goal.to_string(),
            context: Some(pb::PromptContext::from(ctx.clone())),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.preview_assembled(outbound(r)).await }
        })
        .await
        .map_err(status_to_err)?;
        // Server → client only: reconstruct a display message from role + content.
        Ok(resp
            .into_inner()
            .messages
            .into_iter()
            .map(|m| match m.role.as_str() {
                "user" => Message::user(m.content),
                "assistant" => Message::assistant(m.content),
                "tool" => Message::tool("", m.content),
                _ => Message::system(m.content),
            })
            .collect())
    }
}

fn status_to_err(s: tonic::Status) -> Error {
    Error::Prompt(s.message().to_string())
}

fn convert_err(e: agent_proto::ConvertError) -> Error {
    Error::Prompt(e.to_string())
}
