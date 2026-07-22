//! The `SearchBackend` seam as a service, including the server-streaming
//! `Reindex` that bridges the core callback-style progress fn to a stream.

use std::pin::Pin;
use std::sync::Arc;

use agent_core::SearchBackend;
use agent_proto::{pb, status_from_error};
use futures_util::Stream;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;

use super::{missing, span};

pub struct SearchServiceSvc {
    inner: Arc<dyn SearchBackend>,
}

impl SearchServiceSvc {
    pub fn new(inner: Arc<dyn SearchBackend>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::search_service_server::SearchServiceServer<Self> {
        pb::search_service_server::SearchServiceServer::new(self)
    }
    /// The served backend's name — the label echoed on responses + progress.
    fn label(&self) -> String {
        self.inner.capabilities().backend
    }
}

#[tonic::async_trait]
impl pb::search_service_server::SearchService for SearchServiceSvc {
    async fn status(
        &self,
        request: Request<pb::StatusRequest>,
    ) -> Result<Response<pb::StatusResponse>, Status> {
        let sp = span("search.status", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let status = inner.status().await.map_err(|e| status_from_error(&e))?;
            let mut pb_status = pb::IndexStatus::from(status);
            pb_status.backend = label;
            Ok(Response::new(pb::StatusResponse {
                backends: vec![pb_status],
            }))
        }
        .instrument(sp)
        .await
    }

    async fn capabilities(
        &self,
        request: Request<pb::SearchCapabilitiesRequest>,
    ) -> Result<Response<pb::SearchCapabilitiesResponse>, Status> {
        let _sp = span("search.capabilities", request.metadata()).entered();
        let caps = pb::SearchCapabilities::from(self.inner.capabilities());
        Ok(Response::new(pb::SearchCapabilitiesResponse {
            backends: vec![caps],
        }))
    }

    type ReindexStream = Pin<Box<dyn Stream<Item = Result<pb::ReindexProgress, Status>> + Send>>;

    // `tonic::Status` is a large Err type, but the stream item type is fixed by the
    // generated trait.
    #[allow(clippy::result_large_err)]
    async fn reindex(
        &self,
        request: Request<pb::ReindexRequest>,
    ) -> Result<Response<Self::ReindexStream>, Status> {
        let sp = span("search.reindex", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            // Bridge the reindex progress callback into a server-streamed response:
            // a background task drives `reindex`, forwarding each progress increment
            // (and any terminal error) into an mpsc channel that becomes the stream.
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                let tx_progress = tx.clone();
                let progress = move |p: agent_core::ReindexProgress| {
                    let mut pp = pb::ReindexProgress::from(p);
                    pp.backend = label.clone();
                    let _ = tx_progress.send(Ok(pp));
                };
                if let Err(e) = inner.reindex(&progress).await {
                    let _ = tx.send(Err(status_from_error(&e)));
                }
            });
            let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            Ok(Response::new(Box::pin(stream) as Self::ReindexStream))
        }
        .instrument(sp)
        .await
    }

    async fn search(
        &self,
        request: Request<pb::SearchRequest>,
    ) -> Result<Response<pb::SearchResponse>, Status> {
        let sp = span("search.query", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let q = req
                .query
                .ok_or_else(|| missing("SearchRequest.query"))?
                .into();
            let hits = inner.query(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::SearchResponse {
                hits: hits.into_iter().map(Into::into).collect(),
                backend: label,
            }))
        }
        .instrument(sp)
        .await
    }

    async fn list_files(
        &self,
        request: Request<pb::ListFilesRequest>,
    ) -> Result<Response<pb::ListFilesResponse>, Status> {
        let sp = span("search.list_files", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        async move {
            let req = request.into_inner();
            let paths = inner
                .list_files(&req.globs)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ListFilesResponse {
                paths: paths
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                backend: label,
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn search_router(inner: Arc<dyn SearchBackend>) -> Router {
    Server::builder().add_service(SearchServiceSvc::new(inner).into_server())
}
