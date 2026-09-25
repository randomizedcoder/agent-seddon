//! The `SearchBackend` seam as a service, including the server-streaming
//! `Reindex` that bridges the core callback-style progress fn to a stream.
//!
//! Every RPC scopes the caller's ambient identity (`identity_key` + `run_scoped`,
//! mirroring `prompt.rs`/`memory.rs`) before touching the backend, so a
//! `PerTenant<dyn SearchBackend>`-wrapped index (`agent-runtime/src/tenant.rs`)
//! routes a standalone `--serve-search` caller to *their* tenant's index rather
//! than the default `local` one. `reindex` runs the backend call in a detached
//! `tokio::spawn`, which does **not** inherit the task-local `AGENT_IDENTITY`, so
//! it re-enters the scope *inside* the spawned task. Identity is
//! attacker-controlled and fails **closed** to `None` → the `local` tenant.

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
        let key = super::identity_key(request.metadata());
        let sp = span("search.status", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        let work = async move {
            let status = inner.status().await.map_err(|e| status_from_error(&e))?;
            let mut pb_status = pb::IndexStatus::from(status);
            pb_status.backend = label;
            Ok(Response::new(pb::StatusResponse {
                backends: vec![pb_status],
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn capabilities(
        &self,
        request: Request<pb::SearchCapabilitiesRequest>,
    ) -> Result<Response<pb::SearchCapabilitiesResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("search.capabilities", request.metadata());
        let inner = self.inner.clone();
        let work = async move {
            let caps = pb::SearchCapabilities::from(inner.capabilities());
            Ok(Response::new(pb::SearchCapabilitiesResponse {
                backends: vec![caps],
            }))
        }
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    type ReindexStream = Pin<Box<dyn Stream<Item = Result<pb::ReindexProgress, Status>> + Send>>;

    // `tonic::Status` is a large Err type, but the stream item type is fixed by the
    // generated trait.
    #[allow(clippy::result_large_err)]
    async fn reindex(
        &self,
        request: Request<pb::ReindexRequest>,
    ) -> Result<Response<Self::ReindexStream>, Status> {
        let key = super::identity_key(request.metadata());
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
                    pp.backend.clone_from(&label);
                    let _ = tx_progress.send(Ok(pp));
                };
                // A `tokio::spawn` does NOT inherit the task-local `AGENT_IDENTITY`,
                // so re-enter the caller's tenant scope *inside* the spawned task —
                // otherwise the reindex would route to the default `local` store
                // even though the outer handler scoped correctly.
                let job = async move {
                    if let Err(e) = inner.reindex(&progress).await {
                        let _ = tx.send(Err(status_from_error(&e)));
                    }
                };
                super::run_scoped(key, job).await;
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
        let key = super::identity_key(request.metadata());
        let sp = span("search.query", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        let work = async move {
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
        .instrument(sp);
        super::run_scoped(key, work).await
    }

    async fn list_files(
        &self,
        request: Request<pb::ListFilesRequest>,
    ) -> Result<Response<pb::ListFilesResponse>, Status> {
        let key = super::identity_key(request.metadata());
        let sp = span("search.list_files", request.metadata());
        let inner = self.inner.clone();
        let label = self.label();
        let work = async move {
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
        .instrument(sp);
        super::run_scoped(key, work).await
    }
}

pub fn search_router(inner: Arc<dyn SearchBackend>) -> Router {
    Server::builder().add_service(SearchServiceSvc::new(inner).into_server())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{IndexState, IndexStatus, SearchCapabilities, SearchHit, SearchQuery};
    use agent_proto::identity::{SESSION_ID_KEY, USER_ID_KEY};
    use futures_util::StreamExt as _;
    use pb::search_service_server::SearchService as _;
    use rstest::rstest;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    // Records the ambient tenant (`current_identity().user`) at each *data* call — proves the
    // served handler scopes the caller into a `PerTenant`-wrapped index (C31 Principle 3). Note
    // `capabilities()` deliberately does NOT record: it is called by `label()` outside the scope
    // (backend name is tenant-invariant metadata), so recording it would pollute assertions.
    #[derive(Default)]
    struct TenantProbeSearch {
        seen: Arc<Mutex<Vec<Option<String>>>>,
    }
    impl TenantProbeSearch {
        fn record(&self) {
            self.seen
                .lock()
                .unwrap()
                .push(agent_core::current_identity().map(|k| k.user.as_str().to_string()));
        }
    }
    #[tonic::async_trait]
    impl SearchBackend for TenantProbeSearch {
        fn capabilities(&self) -> SearchCapabilities {
            SearchCapabilities {
                backend: "probe".into(),
                modes: vec![],
                content_search: false,
                scored: false,
                incremental: true,
                max_concurrent_queries: 1,
            }
        }
        async fn status(&self) -> agent_core::Result<IndexStatus> {
            self.record();
            Ok(IndexStatus {
                state: IndexState::Missing,
                indexed_files: 0,
                last_indexed_ms: 0,
                manifest_digest: String::new(),
            })
        }
        async fn reindex(
            &self,
            _progress: agent_core::ProgressFn<'_>,
        ) -> agent_core::Result<IndexStatus> {
            // Record exactly once (don't delegate to `status`, which also records).
            self.record();
            Ok(IndexStatus {
                state: IndexState::Missing,
                indexed_files: 0,
                last_indexed_ms: 0,
                manifest_digest: String::new(),
            })
        }
        async fn query(&self, _q: &SearchQuery) -> agent_core::Result<Vec<SearchHit>> {
            self.record();
            Ok(vec![])
        }
        async fn list_files(
            &self,
            _globs: &[String],
        ) -> agent_core::Result<Vec<std::path::PathBuf>> {
            self.record();
            Ok(vec![])
        }
    }

    fn req_with<T>(payload: T, user: Option<&str>, session: Option<&str>) -> Request<T> {
        let mut req = Request::new(payload);
        if let Some(u) = user {
            req.metadata_mut()
                .insert(USER_ID_KEY, MetadataValue::try_from(u).unwrap());
        }
        if let Some(s) = session {
            req.metadata_mut()
                .insert(SESSION_ID_KEY, MetadataValue::try_from(s).unwrap());
        }
        req
    }

    // Every caller-identity class → the tenant the backend runs under. Present + path-safe
    // scopes; partial/hostile fails closed to the default tenant (`None`).
    #[rstest]
    #[case::positive_tenant_header_scopes_to_tenant(Some("acme".into()), Some("s1".into()), Some("acme".into()))]
    #[case::negative_no_identity_runs_as_local(None, None, None)]
    #[case::negative_second_tenant_is_isolated(Some("globex".into()), Some("s1".into()), Some("globex".into()))]
    #[case::boundary_max_len_tenant_segment(Some("a".repeat(128)), Some("s1".into()), Some("a".repeat(128)))]
    #[case::corner_user_without_session_runs_as_local(Some("acme".into()), None, None)]
    #[case::adversarial_traversal_header_fails_to_local(Some("../../heads/main".into()), Some("s1".into()), None)]
    #[case::adversarial_empty_header_fails_to_local(Some(String::new()), Some("s1".into()), None)]
    #[tokio::test]
    async fn search_status_scopes_caller_tenant(
        #[case] user: Option<String>,
        #[case] session: Option<String>,
        #[case] expected: Option<String>,
    ) {
        let store = Arc::new(TenantProbeSearch::default());
        let svc = SearchServiceSvc::new(store.clone());
        let req = req_with(
            pb::StatusRequest::default(),
            user.as_deref(),
            session.as_deref(),
        );
        svc.status(req).await.unwrap();
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[expected]);
    }

    // Two tenants over the wire hit disjoint scopes across different RPCs.
    #[tokio::test]
    async fn positive_two_tenants_are_isolated_across_rpcs() {
        let store = Arc::new(TenantProbeSearch::default());
        let svc = SearchServiceSvc::new(store.clone());
        svc.search(req_with(
            pb::SearchRequest {
                query: Some(pb::SearchQuery::default()),
                ..Default::default()
            },
            Some("acme"),
            Some("s1"),
        ))
        .await
        .unwrap();
        svc.status(req_with(
            pb::StatusRequest::default(),
            Some("globex"),
            Some("s1"),
        ))
        .await
        .unwrap();
        assert_eq!(
            store.seen.lock().unwrap().as_slice(),
            &[Some("acme".to_string()), Some("globex".to_string())]
        );
    }

    // The subtle case: `reindex` runs the backend call in a detached `tokio::spawn`, which does
    // not inherit the task-local identity — the fix re-enters the scope *inside* the spawn. Drain
    // the stream to completion (tx drops when the task ends) so the recorded identity is settled.
    #[tokio::test]
    async fn corner_reindex_scopes_inside_spawned_task() {
        let store = Arc::new(TenantProbeSearch::default());
        let svc = SearchServiceSvc::new(store.clone());
        let resp = svc
            .reindex(req_with(
                pb::ReindexRequest::default(),
                Some("acme"),
                Some("s1"),
            ))
            .await
            .unwrap();
        let mut stream = resp.into_inner();
        while stream.next().await.is_some() {}
        assert_eq!(
            store.seen.lock().unwrap().as_slice(),
            &[Some("acme".to_string())]
        );
    }

    // Same detached-spawn path, hostile identity: reindex must fail closed to `local`, not run
    // under a traversal-laced tenant segment.
    #[tokio::test]
    async fn adversarial_reindex_hostile_identity_fails_to_local() {
        let store = Arc::new(TenantProbeSearch::default());
        let svc = SearchServiceSvc::new(store.clone());
        let resp = svc
            .reindex(req_with(
                pb::ReindexRequest::default(),
                Some("../../heads/main"),
                Some("s1"),
            ))
            .await
            .unwrap();
        let mut stream = resp.into_inner();
        while stream.next().await.is_some() {}
        assert_eq!(store.seen.lock().unwrap().as_slice(), &[None]);
    }
}
