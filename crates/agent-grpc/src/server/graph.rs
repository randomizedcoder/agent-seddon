//! The `GraphStore` seam as a service — the cognition-graph document
//! (cognition-graph 04). The caller is untrusted (the portal's editor, another
//! agent, a script): `Put` is validate-then-accept — an invalid document comes
//! back `INVALID_ARGUMENT` carrying the typed issues, never partially stored —
//! and a document that cannot even *decode* (unknown edge kind, malformed
//! params) is rejected at conversion, before validation. `Get` failing means
//! the stored document is unusable (missing/oversized/invalid on disk): that is
//! server-side state, surfaced as `FAILED_PRECONDITION` so the caller falls
//! back to graph-less behavior rather than treating it as its own bad request.

use std::sync::Arc;

use agent_core::GraphStore;
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct GraphSvc {
    inner: Arc<dyn GraphStore>,
}

impl GraphSvc {
    pub fn new(inner: Arc<dyn GraphStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::graph_service_server::GraphServiceServer<Self> {
        pb::graph_service_server::GraphServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::graph_service_server::GraphService for GraphSvc {
    async fn get(
        &self,
        request: Request<pb::GetGraphRequest>,
    ) -> Result<Response<pb::GetGraphResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("graph.get", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let doc = inner.get().await.map_err(|e| match e {
                agent_core::Error::Graph(_) => Status::failed_precondition(e.to_string()),
                _ => Status::internal(e.to_string()),
            })?;
            Ok(Response::new(pb::GetGraphResponse {
                graph: Some(doc.into()),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn put(
        &self,
        request: Request<pb::PutGraphRequest>,
    ) -> Result<Response<pb::PutGraphResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("graph.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let doc: agent_core::GraphDoc = request
                .into_inner()
                .graph
                .ok_or_else(|| Status::invalid_argument("graph is required"))?
                .try_into()
                .map_err(|e| Status::invalid_argument(format!("invalid graph: {e}")))?;
            inner.put(doc).await.map_err(|e| match e {
                // Validation rejects are the caller's document at fault.
                agent_core::Error::Graph(_) => Status::invalid_argument(e.to_string()),
                _ => Status::internal(e.to_string()),
            })?;
            Ok(Response::new(pb::PutGraphResponse {}))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn validate(
        &self,
        request: Request<pb::ValidateGraphRequest>,
    ) -> Result<Response<pb::ValidateGraphResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("graph.validate", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let doc: agent_core::GraphDoc = request
                .into_inner()
                .graph
                .ok_or_else(|| Status::invalid_argument("graph is required"))?
                .try_into()
                .map_err(|e| Status::invalid_argument(format!("invalid graph: {e}")))?;
            let issues = inner
                .validate(&doc)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(pb::ValidateGraphResponse {
                issues: issues.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn describe_node_types(
        &self,
        request: Request<pb::DescribeNodeTypesRequest>,
    ) -> Result<Response<pb::DescribeNodeTypesResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("graph.describe_node_types", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let types = inner
                .node_types()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(pb::DescribeNodeTypesResponse {
                node_types: types.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn graph_router(inner: Arc<dyn GraphStore>) -> Router {
    Server::builder().add_service(GraphSvc::new(inner).into_server())
}
