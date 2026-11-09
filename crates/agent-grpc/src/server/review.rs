//! The `ReviewCollector` seam as a service — grounded fact collection.
//!
//! > **It executes local git.** Like `--serve-repo`, the collector shells out to
//! > git on the host it runs on; the socket's permissions are the access control
//! > (see docs/grpc.md). It does not run model-authored code.

use std::sync::Arc;

use agent_core::ReviewCollector;
use agent_proto::{pb, status_from_error};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;
use crate::review_target::decode_target;

pub struct FactCollectorServiceSvc {
    inner: Arc<dyn ReviewCollector>,
}

impl FactCollectorServiceSvc {
    pub fn new(inner: Arc<dyn ReviewCollector>) -> Self {
        Self { inner }
    }
    pub fn into_server(
        self,
    ) -> pb::fact_collector_service_server::FactCollectorServiceServer<Self> {
        pb::fact_collector_service_server::FactCollectorServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::fact_collector_service_server::FactCollectorService for FactCollectorServiceSvc {
    async fn collect(
        &self,
        request: Request<pb::ReviewCollectRequest>,
    ) -> Result<Response<pb::ReviewFacts>, Status> {
        let sp = span("review.collect", request.metadata());
        let inner = self.inner.clone();
        async move {
            let target =
                decode_target(&request.into_inner().target).map_err(Status::invalid_argument)?;
            let facts = inner
                .collect(&target)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(facts.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn review_router(inner: Arc<dyn ReviewCollector>) -> Router {
    Server::builder().add_service(FactCollectorServiceSvc::new(inner).into_server())
}
