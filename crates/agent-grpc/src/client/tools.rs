//! The `Tool` seam over the wire — connect, discover, and present remote tools
//! as `Arc<dyn Tool>`.

use std::sync::Arc;

use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

struct GrpcTool {
    client: pb::tool_service_client::ToolServiceClient<Channel>,
    schema: ToolSchema,
    parallel_safe: bool,
    retry: agent_retry::RetryPolicy,
}

#[async_trait]
impl Tool for GrpcTool {
    fn name(&self) -> &str {
        &self.schema.name
    }

    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    /// Preserve the remote tool's concurrency contract (carried in `DescribeAll`),
    /// so a non-parallel-safe remote tool isn't run concurrently by the loop.
    fn parallel_safe(&self) -> bool {
        self.parallel_safe
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<Observation> {
        let req = pb::ExecuteRequest {
            name: self.schema.name.clone(),
            arguments: Some(args.into()),
            context: Some(ctx.into()),
        };
        // Mirror `McpTool`: transport failures surface as an error observation, not
        // a hard `Err`, so one flaky worker doesn't abort the turn — but retry a
        // transient overload/availability blip first.
        match call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.execute(outbound(r)).await }
        })
        .await
        {
            // A malformed block from a remote tool fails closed rather than
            // silently becoming empty content.
            Ok(resp) => Ok(
                Observation::try_from(resp.into_inner()).unwrap_or_else(|e| {
                    Observation::error(format!(
                        "grpc tool `{}` sent a bad observation: {e}",
                        self.schema.name
                    ))
                }),
            ),
            Err(s) => Ok(Observation::error(format!(
                "grpc tool `{}` failed: {s}",
                self.schema.name
            ))),
        }
    }
}
/// Connect to a remote tool worker, discover its tools (`DescribeAll`), and return
/// one `Arc<dyn Tool>` per remote tool (each calls `Execute`). Mirrors
/// `agent-mcp`'s `connect_tools`.
pub async fn grpc_tools(endpoint: &Endpoint) -> Result<Vec<Arc<dyn Tool>>> {
    let channel = endpoint
        .connect_lazy()
        .map_err(|e| agent_core::Error::Tool(e.to_string()))?;
    let client = pb::tool_service_client::ToolServiceClient::new(channel.clone());
    let policy = grpc_retry_policy();
    let resp = call_retry(&policy, || {
        let mut client = client.clone();
        async move {
            client
                .describe_all(outbound(pb::DescribeAllRequest {}))
                .await
        }
    })
    .await
    .map_err(|s| agent_core::Error::Tool(s.to_string()))?;
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();
    for schema in resp.into_inner().tools {
        // Read the concurrency flag off the wire before converting (agent-core's
        // `ToolSchema` has no such field).
        let parallel_safe = schema.parallel_safe;
        let schema: ToolSchema = schema
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Tool(e.to_string()))?;
        tools.push(Arc::new(GrpcTool {
            client: pb::tool_service_client::ToolServiceClient::new(channel.clone()),
            schema,
            parallel_safe,
            retry: grpc_retry_policy(),
        }));
    }
    Ok(tools)
}
