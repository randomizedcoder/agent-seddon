//! gRPC **servers** — one adapter per seam that wraps a locally-built
//! `Arc<dyn Trait>` and serves the generated tonic service.
//!
//! Each handler converts proto → core on the way in and core → proto out (via
//! `agent-proto`), maps `agent_core::Error` to a `tonic::Status`, and makes its
//! span a child of the caller's W3C trace context (extracted from request
//! metadata) so a trace spans the hop.
//!
//! The `*_router` helpers build a ready-to-serve `Router`, keeping tonic out of
//! the CLI; feed one to [`crate::transport::Bound::serve`].

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use agent_core::{
    ContextStrategy, EpisodicStore, LlmProvider, MemoryStore, Policy, RepoBackend, SearchBackend,
    SemanticStore, ToolRegistry,
};
use agent_proto::{pb, status_from_error};
use futures_util::{Stream, StreamExt};
use tonic::transport::server::Router;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Build a per-call span parented on the caller's extracted trace context.
fn span(rpc: &'static str, meta: &tonic::metadata::MetadataMap) -> tracing::Span {
    let s = tracing::info_span!("grpc.server", rpc);
    s.set_parent(agent_proto::trace::extract_context(meta));
    s
}

fn missing(field: &'static str) -> Status {
    Status::invalid_argument(format!("missing required field `{field}`"))
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct ProviderService {
    inner: Arc<dyn LlmProvider>,
}

impl ProviderService {
    pub fn new(inner: Arc<dyn LlmProvider>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::provider_server::ProviderServer<Self> {
        pb::provider_server::ProviderServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::provider_server::Provider for ProviderService {
    async fn capabilities(
        &self,
        request: Request<pb::CapabilitiesRequest>,
    ) -> Result<Response<pb::ModelCapabilities>, Status> {
        let sp = span("provider.capabilities", request.metadata());
        let inner = self.inner.clone();
        async move { Ok(Response::new(inner.capabilities().into())) }
            .instrument(sp)
            .await
    }

    async fn complete(
        &self,
        request: Request<pb::CompletionRequest>,
    ) -> Result<Response<pb::CompletionResponse>, Status> {
        let sp = span("provider.complete", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner().try_into()?;
            let resp = inner
                .complete(req)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(resp.into()))
        }
        .instrument(sp)
        .await
    }

    type StreamStream = Pin<Box<dyn Stream<Item = Result<pb::CompletionChunk, Status>> + Send>>;

    // `tonic::Status` is a large Err type, but the stream item type is fixed by the
    // generated trait — boxing it would defeat the point.
    #[allow(clippy::result_large_err)]
    async fn stream(
        &self,
        request: Request<pb::CompletionRequest>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        let sp = span("provider.stream", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner().try_into()?;
            let chunks = inner.stream(req).await.map_err(|e| status_from_error(&e))?;
            let mapped = chunks.map(|item| {
                item.map(pb::CompletionChunk::from)
                    .map_err(|e| status_from_error(&e))
            });
            Ok(Response::new(Box::pin(mapped) as Self::StreamStream))
        }
        .instrument(sp)
        .await
    }
}

pub fn provider_router(inner: Arc<dyn LlmProvider>) -> Router {
    Server::builder().add_service(ProviderService::new(inner).into_server())
}

// ---------------------------------------------------------------------------
// Tools (a worker hosting a ToolRegistry)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Memory (facade) + Episodic + Semantic layer adapters
// ---------------------------------------------------------------------------

pub struct MemoryService {
    inner: Arc<dyn MemoryStore>,
}

impl MemoryService {
    pub fn new(inner: Arc<dyn MemoryStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::memory_server::MemoryServer<Self> {
        pb::memory_server::MemoryServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::memory_server::Memory for MemoryService {
    async fn recall(
        &self,
        request: Request<pb::RecallQuery>,
    ) -> Result<Response<pb::RecallResponse>, Status> {
        let sp = span("memory.recall", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q = request.into_inner().into();
            let items = inner.recall(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::RecallResponse {
                items: items.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn append(
        &self,
        request: Request<pb::MemoryEvent>,
    ) -> Result<Response<pb::AppendResponse>, Status> {
        let sp = span("memory.append", request.metadata());
        let inner = self.inner.clone();
        async move {
            let event = request.into_inner().try_into()?;
            inner
                .append(event)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AppendResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn distill(
        &self,
        request: Request<pb::DistillRequest>,
    ) -> Result<Response<pb::DistillResponse>, Status> {
        let sp = span("memory.distill", request.metadata());
        let inner = self.inner.clone();
        async move {
            let count = inner.distill().await.map_err(|e| status_from_error(&e))? as u64;
            Ok(Response::new(pb::DistillResponse { count }))
        }
        .instrument(sp)
        .await
    }
}

pub struct EpisodicService {
    inner: Arc<dyn EpisodicStore>,
}

impl EpisodicService {
    pub fn new(inner: Arc<dyn EpisodicStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::episodic_server::EpisodicServer<Self> {
        pb::episodic_server::EpisodicServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::episodic_server::Episodic for EpisodicService {
    async fn append(
        &self,
        request: Request<pb::MemoryEvent>,
    ) -> Result<Response<pb::AppendResponse>, Status> {
        let sp = span("episodic.append", request.metadata());
        let inner = self.inner.clone();
        async move {
            let event = request.into_inner().try_into()?;
            inner
                .append(event)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AppendResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn recent(
        &self,
        request: Request<pb::RecentRequest>,
    ) -> Result<Response<pb::RecentResponse>, Status> {
        let sp = span("episodic.recent", request.metadata());
        let inner = self.inner.clone();
        async move {
            let limit = request.into_inner().limit as usize;
            let events = inner
                .recent(limit)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::RecentResponse {
                events: events.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub struct SemanticService {
    inner: Arc<dyn SemanticStore>,
}

impl SemanticService {
    pub fn new(inner: Arc<dyn SemanticStore>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::semantic_server::SemanticServer<Self> {
        pb::semantic_server::SemanticServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::semantic_server::Semantic for SemanticService {
    async fn recall(
        &self,
        request: Request<pb::RecallQuery>,
    ) -> Result<Response<pb::RecallResponse>, Status> {
        let sp = span("semantic.recall", request.metadata());
        let inner = self.inner.clone();
        async move {
            let q = request.into_inner().into();
            let items = inner.recall(&q).await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::RecallResponse {
                items: items.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn distill(
        &self,
        request: Request<pb::SemanticDistillRequest>,
    ) -> Result<Response<pb::DistillResponse>, Status> {
        let sp = span("semantic.distill", request.metadata());
        let inner = self.inner.clone();
        async move {
            let episodic = request
                .into_inner()
                .episodic
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?;
            let count = inner
                .distill(&episodic)
                .await
                .map_err(|e| status_from_error(&e))? as u64;
            Ok(Response::new(pb::DistillResponse { count }))
        }
        .instrument(sp)
        .await
    }
}

pub fn memory_router(inner: Arc<dyn MemoryStore>) -> Router {
    Server::builder().add_service(MemoryService::new(inner).into_server())
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

pub struct ContextSvc {
    inner: Arc<dyn ContextStrategy>,
}

impl ContextSvc {
    pub fn new(inner: Arc<dyn ContextStrategy>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::context_service_server::ContextServiceServer<Self> {
        pb::context_service_server::ContextServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::context_service_server::ContextService for ContextSvc {
    async fn assemble(
        &self,
        request: Request<pb::ContextInput>,
    ) -> Result<Response<pb::AssembleResponse>, Status> {
        let sp = span("context.assemble", request.metadata());
        let inner = self.inner.clone();
        async move {
            let input = request.into_inner().into();
            let messages = inner
                .assemble(input)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::AssembleResponse {
                messages: messages.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn compact(
        &self,
        request: Request<pb::CompactRequest>,
    ) -> Result<Response<pb::CompactResponse>, Status> {
        let sp = span("context.compact", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let mut working = req
                .working
                .ok_or_else(|| missing("CompactRequest.working"))?
                .try_into()?;
            let budget = req
                .budget
                .ok_or_else(|| missing("CompactRequest.budget"))?
                .into();
            inner
                .compact(&mut working, &budget)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::CompactResponse {
                working: Some(working.into()),
            }))
        }
        .instrument(sp)
        .await
    }
}

pub fn context_router(inner: Arc<dyn ContextStrategy>) -> Router {
    Server::builder().add_service(ContextSvc::new(inner).into_server())
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

pub struct PolicySvc {
    inner: Arc<dyn Policy>,
}

impl PolicySvc {
    pub fn new(inner: Arc<dyn Policy>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::policy_server::PolicyServer<Self> {
        pb::policy_server::PolicyServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::policy_server::Policy for PolicySvc {
    async fn authorize(
        &self,
        request: Request<pb::ToolCall>,
    ) -> Result<Response<pb::Decision>, Status> {
        let sp = span("policy.authorize", request.metadata());
        let inner = self.inner.clone();
        async move {
            let call = request.into_inner().try_into()?;
            let decision = inner.authorize(&call).await;
            Ok(Response::new(decision.into()))
        }
        .instrument(sp)
        .await
    }
}

pub fn policy_router(inner: Arc<dyn Policy>) -> Router {
    Server::builder().add_service(PolicySvc::new(inner).into_server())
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Repo (multi-branch git)
// ---------------------------------------------------------------------------

pub struct RepoServiceSvc {
    inner: Arc<dyn RepoBackend>,
}

impl RepoServiceSvc {
    pub fn new(inner: Arc<dyn RepoBackend>) -> Self {
        Self { inner }
    }
    pub fn into_server(self) -> pb::repo_service_server::RepoServiceServer<Self> {
        pb::repo_service_server::RepoServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl pb::repo_service_server::RepoService for RepoServiceSvc {
    async fn resolve(
        &self,
        request: Request<pb::ResolveRequest>,
    ) -> Result<Response<pb::ResolveResponse>, Status> {
        let sp = span("repo.resolve", request.metadata());
        let inner = self.inner.clone();
        async move {
            let rev = agent_core::Revision(request.into_inner().revision);
            let oid = inner
                .resolve(&rev)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ResolveResponse { oid: oid.0 }))
        }
        .instrument(sp)
        .await
    }

    async fn read_file(
        &self,
        request: Request<pb::ReadFileRequest>,
    ) -> Result<Response<pb::BlobContent>, Status> {
        let sp = span("repo.read_file", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let blob = inner
                .read_file(&rev, std::path::Path::new(&req.path))
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(blob.into()))
        }
        .instrument(sp)
        .await
    }

    async fn list_tree(
        &self,
        request: Request<pb::ListTreeRequest>,
    ) -> Result<Response<pb::ListTreeResponse>, Status> {
        let sp = span("repo.list_tree", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let entries = inner
                .list_tree(&rev, std::path::Path::new(&req.path), req.recursive)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::ListTreeResponse {
                entries: entries.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn diff(
        &self,
        request: Request<pb::DiffRequest>,
    ) -> Result<Response<pb::DiffResult>, Status> {
        let sp = span("repo.diff", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let base = agent_core::Revision(req.base);
            let target = agent_core::Revision(req.target);
            let result = inner
                .diff(&base, &target, &req.path_globs)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(result.into()))
        }
        .instrument(sp)
        .await
    }

    async fn grep(
        &self,
        request: Request<pb::GrepRequest>,
    ) -> Result<Response<pb::GrepResponse>, Status> {
        let sp = span("repo.grep", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let hits = inner
                .grep(&rev, &req.pattern, &req.path_globs, req.limit as usize)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::GrepResponse {
                hits: hits.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn log(
        &self,
        request: Request<pb::LogRequest>,
    ) -> Result<Response<pb::LogResponse>, Status> {
        let sp = span("repo.log", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let rev = agent_core::Revision(req.revision);
            let path = req.path.map(std::path::PathBuf::from);
            let commits = inner
                .log(&rev, path.as_deref(), req.limit as usize)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::LogResponse {
                commits: commits.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn branches(
        &self,
        request: Request<pb::BranchesRequest>,
    ) -> Result<Response<pb::BranchesResponse>, Status> {
        let sp = span("repo.branches", request.metadata());
        let inner = self.inner.clone();
        async move {
            let branches = inner.branches().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::BranchesResponse {
                branches: branches
                    .into_iter()
                    .map(|(name, oid)| pb::Branch { name, oid: oid.0 })
                    .collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn status(
        &self,
        request: Request<pb::RepoStatusRequest>,
    ) -> Result<Response<pb::RepoStatus>, Status> {
        let sp = span("repo.status", request.metadata());
        let inner = self.inner.clone();
        async move {
            let status = inner.status().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(status.into()))
        }
        .instrument(sp)
        .await
    }

    async fn fetch(
        &self,
        request: Request<pb::FetchRequest>,
    ) -> Result<Response<pb::RepoStatus>, Status> {
        let sp = span("repo.fetch", request.metadata());
        let inner = self.inner.clone();
        async move {
            let status = inner.fetch().await.map_err(|e| status_from_error(&e))?;
            Ok(Response::new(status.into()))
        }
        .instrument(sp)
        .await
    }

    async fn worktree_add(
        &self,
        request: Request<pb::WorktreeSpec>,
    ) -> Result<Response<pb::WorktreeHandle>, Status> {
        let sp = span("repo.worktree_add", request.metadata());
        let inner = self.inner.clone();
        async move {
            let spec = request.into_inner().into();
            let handle = inner
                .worktree_add(&spec)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(handle.into()))
        }
        .instrument(sp)
        .await
    }

    async fn worktree_list(
        &self,
        request: Request<pb::WorktreeListRequest>,
    ) -> Result<Response<pb::WorktreeListResponse>, Status> {
        let sp = span("repo.worktree_list", request.metadata());
        let inner = self.inner.clone();
        async move {
            let ws = inner
                .worktree_list()
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::WorktreeListResponse {
                worktrees: ws.into_iter().map(Into::into).collect(),
            }))
        }
        .instrument(sp)
        .await
    }

    async fn worktree_remove(
        &self,
        request: Request<pb::WorktreeRemoveRequest>,
    ) -> Result<Response<pb::WorktreeRemoveResponse>, Status> {
        let sp = span("repo.worktree_remove", request.metadata());
        let inner = self.inner.clone();
        async move {
            inner
                .worktree_remove(&request.into_inner().id)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::WorktreeRemoveResponse {}))
        }
        .instrument(sp)
        .await
    }

    async fn create_checkpoint(
        &self,
        request: Request<pb::CheckpointRequest>,
    ) -> Result<Response<pb::Checkpoint>, Status> {
        let sp = span("repo.checkpoint", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let cp = inner
                .checkpoint(&req.worktree_id, &req.name)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(cp.into()))
        }
        .instrument(sp)
        .await
    }

    async fn push(
        &self,
        request: Request<pb::PushRequest>,
    ) -> Result<Response<pb::PushResponse>, Status> {
        let sp = span("repo.push", request.metadata());
        let inner = self.inner.clone();
        async move {
            let req = request.into_inner();
            let checkpoint = req
                .checkpoint
                .ok_or_else(|| missing("PushRequest.checkpoint"))?;
            inner
                .push(&checkpoint.into(), &req.remote_ref)
                .await
                .map_err(|e| status_from_error(&e))?;
            Ok(Response::new(pb::PushResponse {}))
        }
        .instrument(sp)
        .await
    }
}

pub fn repo_router(inner: Arc<dyn RepoBackend>) -> Router {
    Server::builder().add_service(RepoServiceSvc::new(inner).into_server())
}
