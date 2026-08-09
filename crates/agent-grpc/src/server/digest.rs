//! The `DigestStore` seam as a service — the per-session digest ledger
//! (cognition-graph 02/04). Both RPCs treat the caller as untrusted: `Put`
//! re-validates via the store's sanitizers (invalid ids reject, sizes cap) and an
//! unknown `kind` is rejected at conversion (fail closed); `Query` re-validates
//! the session id and caps the limit server-side. Unlike the fail-soft dimension
//! seam, errors PROPAGATE: the ledger is a durable write path (the distiller
//! counts failures) and a read miss must trigger the caller's fallback, not
//! silently look like an empty ledger.

use std::sync::Arc;

use agent_core::{DigestQuery, DigestStore};
use agent_proto::pb;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::span;

pub struct DigestSvc {
    inner: Arc<dyn DigestStore>,
}

impl DigestSvc {
    pub fn new(inner: Arc<dyn DigestStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::digest_service_server::DigestServiceServer<Self> {
        pb::digest_service_server::DigestServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::digest_service_server::DigestService for DigestSvc {
    async fn put(
        &self,
        request: Request<pb::PutDigestRequest>,
    ) -> Result<Response<pb::PutDigestResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("digest.put", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let digest: agent_core::Digest = request
                .into_inner()
                .digest
                .ok_or_else(|| Status::invalid_argument("digest is required"))?
                .try_into()
                .map_err(|e| Status::invalid_argument(format!("invalid digest: {e}")))?;
            inner.put(digest).await.map_err(|e| match e {
                // The store's sanitizers reject hostile ids — caller error.
                agent_core::Error::Memory(ref m) if m.contains("invalid") => {
                    Status::invalid_argument(e.to_string())
                }
                _ => Status::internal(e.to_string()),
            })?;
            Ok(Response::new(pb::PutDigestResponse {}))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn query(
        &self,
        request: Request<pb::QueryDigestsRequest>,
    ) -> Result<Response<pb::QueryDigestsResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("digest.query", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let req = request.into_inner();
            // Fail closed on an unknown kind filter (an empty string = all kinds).
            let kind = match req.kind.as_str() {
                "" => None,
                k => Some(
                    agent_core::DigestKind::parse(k)
                        .ok_or_else(|| Status::invalid_argument("unknown digest kind"))?,
                ),
            };
            let q = DigestQuery {
                session_id: req.session_id,
                kind,
                since_seq: (req.since_seq > 0).then_some(req.since_seq),
                keywords_any: req.keywords_any,
                // 0 = server cap; the store clamps a hostile value regardless.
                limit: req.limit as usize,
            };
            let rows = inner.query(&q).await.map_err(|e| match e {
                agent_core::Error::Memory(ref m) if m.contains("invalid") => {
                    Status::invalid_argument(e.to_string())
                }
                _ => Status::internal(e.to_string()),
            })?;
            Ok(Response::new(pb::QueryDigestsResponse {
                digests: rows.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn digest_router(inner: Arc<dyn DigestStore>) -> Router {
    Server::builder().add_service(DigestSvc::new(inner).into_server())
}
