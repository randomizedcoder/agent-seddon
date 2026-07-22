//! The `Scanner` seam as a service — untrusted-content scanning behind gRPC.

use std::sync::Arc;

use agent_core::Scanner;
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct ScannerServiceSvc {
    inner: Arc<dyn Scanner>,
}

impl ScannerServiceSvc {
    pub fn new(inner: Arc<dyn Scanner>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::scanner_service_server::ScannerServiceServer<Self> {
        pb::scanner_service_server::ScannerServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::scanner_service_server::ScannerService for ScannerServiceSvc {
    async fn scan(
        &self,
        request: Request<pb::ScanRequest>,
    ) -> Result<Response<pb::ScanResponse>, Status> {
        let sp = span("scanner.scan", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            // `Scanner::scan` cannot fail by construction, so there is no error
            // path to map here — an empty result means "nothing found".
            let findings = inner
                .scan(
                    agent_proto::convert::scan_kind_from_i32(req.kind),
                    &req.content,
                )
                .await;
            Ok(Response::new(pb::ScanResponse {
                findings: findings.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn scanner_router(inner: Arc<dyn Scanner>) -> Router {
    Server::builder().add_service(ScannerServiceSvc::new(inner).into_server())
}
