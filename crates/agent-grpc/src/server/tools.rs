//! The `Tool` seam as a service — a worker hosting a [`ToolRegistry`].

use std::path::PathBuf;

use agent_core::ToolRegistry;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ToolWorker {
    tools: ToolRegistry,
    cwd: PathBuf,
}

impl ToolWorker {
    pub fn new(tools: ToolRegistry, cwd: PathBuf) -> Self {
        Self { tools, cwd }
    }
    pub fn into_server(self) -> pb::tool_service_server::ToolServiceServer<Self> {
        pb::tool_service_server::ToolServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::tool_service_server::ToolService for ToolWorker {
    async fn describe_all(
        &self,
        request: Request<pb::DescribeAllRequest>,
    ) -> Result<Response<pb::DescribeAllResponse>, Status> {
        let _sp = span("tools.describe_all", request.metadata()).entered();
        let tools = self
            .tools
            .describe_all()
            .into_iter()
            .map(|schema| {
                // Carry each tool's real `parallel_safe` flag (the `From` default is
                // `true`); look the tool back up by name so a non-parallel-safe
                // remote tool isn't run concurrently by the client loop.
                let parallel_safe = self
                    .tools
                    .get(&schema.name)
                    .is_none_or(|t| t.parallel_safe());
                pb::ToolSchema {
                    parallel_safe,
                    ..schema.into()
                }
            })
            .collect();
        Ok(Response::new(pb::DescribeAllResponse { tools }))
    }

    async fn execute(
        &self,
        request: Request<pb::ExecuteRequest>,
    ) -> Result<Response<pb::Observation>, Status> {
        let sp = span("tools.execute", request.metadata());
        let tools = self.tools.clone();
        let default_cwd = self.cwd.clone();
        async move {
            let req = request.into_inner();
            let args = req
                .arguments
                .map(TryInto::try_into)
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            let ctx = req
                .context
                .map(agent_core::ToolContext::from)
                .unwrap_or(agent_core::ToolContext { cwd: default_cwd });
            let tool = tools
                .get(&req.name)
                .ok_or_else(|| Status::not_found(format!("no tool `{}`", req.name)))?;
            let obs = tool
                .execute(args, &ctx)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(obs.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn tools_router(tools: ToolRegistry, cwd: PathBuf) -> Router {
    Server::builder().add_service(ToolWorker::new(tools, cwd).into_server())
}
