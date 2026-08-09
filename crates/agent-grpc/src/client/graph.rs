//! The `GraphStore` seam over the wire. **Errors propagate**: a `get` failure
//! must make the executor fall back to graph-less behavior explicitly — a
//! transport error must never read as "empty graph". Wire→core is fail-closed
//! all the way down: a document with an unknown edge kind, or an issue list
//! carrying an unknown code, fails the *whole* call rather than being silently
//! thinned (dropping issues could flip an invalid document to "valid").

use agent_core::{GraphDoc, GraphIssue, GraphStore, NodeTypeSchema, Result};
use agent_proto::pb;
use async_trait::async_trait;
use tonic::transport::Channel;

use super::{call_retry, grpc_retry_policy, outbound};
use crate::transport::Endpoint;

pub struct GrpcGraphs {
    client: pb::graph_service_client::GraphServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcGraphs {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Graph(e.to_string()))?;
        Ok(Self {
            client: pb::graph_service_client::GraphServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl GraphStore for GrpcGraphs {
    async fn get(&self) -> Result<GraphDoc> {
        let req = pb::GetGraphRequest {};
        let resp = unary!(self, get, req)
            .map_err(|s| agent_core::Error::Graph(format!("graph get RPC: {s}")))?;
        resp.into_inner()
            .graph
            .ok_or_else(|| agent_core::Error::Graph("graph get RPC: empty response".into()))?
            .try_into()
            .map_err(|e| agent_core::Error::Graph(format!("graph decode: {e}")))
    }

    async fn put(&self, doc: GraphDoc) -> Result<()> {
        let req = pb::PutGraphRequest {
            graph: Some(doc.into()),
        };
        unary!(self, put, req)
            .map(|_| ())
            .map_err(|s| agent_core::Error::Graph(format!("graph put RPC: {s}")))
    }

    async fn validate(&self, doc: &GraphDoc) -> Result<Vec<GraphIssue>> {
        let req = pb::ValidateGraphRequest {
            graph: Some(doc.clone().into()),
        };
        let resp = unary!(self, validate, req)
            .map_err(|s| agent_core::Error::Graph(format!("graph validate RPC: {s}")))?;
        resp.into_inner()
            .issues
            .into_iter()
            .map(|i| {
                i.try_into()
                    .map_err(|e| agent_core::Error::Graph(format!("issue decode: {e}")))
            })
            .collect()
    }

    async fn node_types(&self) -> Result<Vec<NodeTypeSchema>> {
        let req = pb::DescribeNodeTypesRequest {};
        let resp = unary!(self, describe_node_types, req)
            .map_err(|s| agent_core::Error::Graph(format!("graph node-types RPC: {s}")))?;
        resp.into_inner()
            .node_types
            .into_iter()
            .map(|t| {
                t.try_into()
                    .map_err(|e| agent_core::Error::Graph(format!("schema decode: {e}")))
            })
            .collect()
    }
}
