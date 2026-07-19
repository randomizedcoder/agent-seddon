//! gRPC **clients** — each implements an `agent_core` seam trait by calling a
//! remote server, so the loop can't tell a remote seam from a local one.
//!
//! Channels are built **lazily** (see [`crate::transport::Endpoint::connect_lazy`])
//! so the runtime's synchronous seam factories can construct a client without
//! awaiting. Every outbound request carries the current W3C trace context in its
//! metadata (via [`outbound`]) so the server can continue the trace.

use std::sync::Arc;

use agent_core::{
    BlobContent, Checkpoint, ChunkStream, CommitInfo, CompletionRequest, CompletionResponse,
    ContextInput, ContextStrategy, Decision, DiffResult, GrepHit, IndexStatus, LlmProvider,
    MemoryEvent, MemoryItem, MemoryStore, Message, ModelCapabilities, Observation, Oid, Policy,
    ProgressFn, RecallQuery, RepoBackend, RepoStatus, Result, Revision, SearchBackend,
    SearchCapabilities, SearchHit, SearchMode, SearchQuery, TokenBudget, Tool, ToolContext,
    ToolSchema, TreeEntry, WorkingSet, WorktreeHandle, WorktreeSpec,
};
use agent_proto::pb;
use async_trait::async_trait;
use futures_util::StreamExt;
use tonic::transport::Channel;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::transport::Endpoint;

/// Wrap a message in a request carrying the caller's trace context.
///
/// We inject the *active `tracing` span's* OTel context, not
/// `opentelemetry::Context::current()` — the tracing-opentelemetry bridge keeps a
/// span's OTel context in the span's extensions, not the OTel thread-local, so the
/// latter is empty here and the server would see no parent. With the loop's seam
/// calls wrapped in spans, this makes the server's handler span a child of the
/// caller's span → one trace across the process boundary.
fn outbound<T>(msg: T) -> tonic::Request<T> {
    let mut req = tonic::Request::new(msg);
    let cx = tracing::Span::current().context();
    agent_proto::trace::inject_context(&cx, req.metadata_mut());
    req
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct GrpcProvider {
    client: pb::provider_client::ProviderClient<Channel>,
    caps: ModelCapabilities,
}

impl GrpcProvider {
    /// Connect lazily. `caps` is config-derived so `capabilities()` (a sync trait
    /// method) needs no round-trip and the factory stays synchronous.
    pub fn connect(endpoint: &Endpoint, caps: ModelCapabilities) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::provider_client::ProviderClient::new(channel),
            caps,
        })
    }
}

#[async_trait]
impl LlmProvider for GrpcProvider {
    fn capabilities(&self) -> ModelCapabilities {
        self.caps.clone()
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let mut client = self.client.clone();
        let resp = client
            .complete(outbound(pb::CompletionRequest::from(req)))
            .await
            .map_err(|s| agent_core::Error::Provider(s.to_string()))?;
        resp.into_inner()
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        let mut client = self.client.clone();
        let stream = client
            .stream(outbound(pb::CompletionRequest::from(req)))
            .await
            .map_err(|s| agent_core::Error::Provider(s.to_string()))?
            .into_inner();
        let mapped = stream.map(|item| match item {
            Ok(chunk) => agent_core::CompletionChunk::try_from(chunk)
                .map_err(|e| agent_core::Error::Provider(e.to_string())),
            Err(s) => Err(agent_core::Error::Provider(s.to_string())),
        });
        Ok(Box::pin(mapped))
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

pub struct GrpcMemory {
    client: pb::memory_client::MemoryClient<Channel>,
}

impl GrpcMemory {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Memory(e.to_string()))?;
        Ok(Self {
            client: pb::memory_client::MemoryClient::new(channel),
        })
    }
}

#[async_trait]
impl MemoryStore for GrpcMemory {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        let mut client = self.client.clone();
        let resp = client
            .recall(outbound(pb::RecallQuery::from(query.clone())))
            .await
            .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(resp
            .into_inner()
            .items
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn append(&self, event: MemoryEvent) -> Result<()> {
        let mut client = self.client.clone();
        client
            .append(outbound(pb::MemoryEvent::from(event)))
            .await
            .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(())
    }

    async fn distill(&self) -> Result<usize> {
        let mut client = self.client.clone();
        let resp = client
            .distill(outbound(pb::DistillRequest {}))
            .await
            .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(resp.into_inner().count as usize)
    }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

pub struct GrpcContext {
    client: pb::context_service_client::ContextServiceClient<Channel>,
}

impl GrpcContext {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::context_service_client::ContextServiceClient::new(channel),
        })
    }
}

#[async_trait]
impl ContextStrategy for GrpcContext {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        let mut client = self.client.clone();
        let resp = client
            .assemble(outbound(pb::ContextInput::from(input)))
            .await
            .map_err(|s| agent_core::Error::Provider(s.to_string()))?;
        resp.into_inner()
            .messages
            .into_iter()
            .map(|m| m.try_into())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))
    }

    async fn compact(&self, working: &mut WorkingSet, budget: &TokenBudget) -> Result<()> {
        let mut client = self.client.clone();
        let req = pb::CompactRequest {
            working: Some(std::mem::take(working).into()),
            budget: Some(budget.clone().into()),
        };
        let resp = client
            .compact(outbound(req))
            .await
            .map_err(|s| agent_core::Error::Provider(s.to_string()))?;
        let compacted = resp
            .into_inner()
            .working
            .ok_or_else(|| agent_core::Error::Provider("compact: missing working set".into()))?;
        *working = compacted
            .try_into()
            .map_err(|e: agent_proto::ConvertError| agent_core::Error::Provider(e.to_string()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

pub struct GrpcPolicy {
    client: pb::policy_client::PolicyClient<Channel>,
}

impl GrpcPolicy {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::policy_client::PolicyClient::new(channel),
        })
    }
}

#[async_trait]
impl Policy for GrpcPolicy {
    async fn authorize(&self, call: &agent_core::ToolCall) -> Decision {
        let mut client = self.client.clone();
        match client
            .authorize(outbound(pb::ToolCall::from(call.clone())))
            .await
        {
            Ok(resp) => resp.into_inner().into(),
            // Fail safe: a broken policy service denies rather than silently allows.
            Err(s) => Decision::Deny(format!("policy rpc failed: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Tools — connect, discover, and present remote tools as `Arc<dyn Tool>`
// ---------------------------------------------------------------------------

struct GrpcTool {
    client: pb::tool_service_client::ToolServiceClient<Channel>,
    schema: ToolSchema,
    parallel_safe: bool,
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
        let mut client = self.client.clone();
        let req = pb::ExecuteRequest {
            name: self.schema.name.clone(),
            arguments: Some(args.into()),
            context: Some(ctx.into()),
        };
        // Mirror `McpTool`: transport failures surface as an error observation, not
        // a hard `Err`, so one flaky worker doesn't abort the turn.
        match client.execute(outbound(req)).await {
            Ok(resp) => Ok(resp.into_inner().into()),
            Err(s) => Ok(Observation::error(format!(
                "grpc tool `{}` failed: {s}",
                self.schema.name
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

pub struct GrpcSearch {
    client: pb::search_service_client::SearchServiceClient<Channel>,
}

impl GrpcSearch {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Search(e.to_string()))?;
        Ok(Self {
            client: pb::search_service_client::SearchServiceClient::new(channel),
        })
    }
}

#[async_trait]
impl SearchBackend for GrpcSearch {
    fn capabilities(&self) -> SearchCapabilities {
        // A sync trait method can't round-trip, so the remote client advertises a
        // permissive capability set (the real backend behind the gateway enforces
        // the actual modes and rejects anything it can't serve).
        SearchCapabilities {
            backend: "grpc".into(),
            modes: vec![
                SearchMode::Literal,
                SearchMode::Phrase,
                SearchMode::Fuzzy,
                SearchMode::Regex,
            ],
            content_search: true,
            scored: true,
            incremental: true,
            max_concurrent_queries: 0,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        let mut client = self.client.clone();
        let resp = client
            .status(outbound(pb::StatusRequest {
                backend: String::new(),
            }))
            .await
            .map_err(|s| agent_core::Error::Search(s.to_string()))?;
        resp.into_inner()
            .backends
            .into_iter()
            .next()
            .map(IndexStatus::from)
            .ok_or_else(|| agent_core::Error::Search("search status: empty response".into()))
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        let mut client = self.client.clone();
        let mut stream = client
            .reindex(outbound(pb::ReindexRequest {
                backend: String::new(),
            }))
            .await
            .map_err(|s| agent_core::Error::Search(s.to_string()))?
            .into_inner();
        while let Some(item) = stream.next().await {
            let p = item.map_err(|s| agent_core::Error::Search(s.to_string()))?;
            progress(agent_core::ReindexProgress::from(p));
        }
        // The stream carries progress, not a terminal status — fetch final state.
        self.status().await
    }

    async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
        let mut client = self.client.clone();
        let req = pb::SearchRequest {
            query: Some(pb::SearchQuery::from(q.clone())),
            backend: String::new(),
        };
        let resp = client
            .search(outbound(req))
            .await
            .map_err(|s| agent_core::Error::Search(s.to_string()))?;
        Ok(resp.into_inner().hits.into_iter().map(Into::into).collect())
    }
}

/// A `RepoBackend` that calls a remote `RepoService` (multi-branch git gateway).
pub struct GrpcRepo {
    client: pb::repo_service_client::RepoServiceClient<Channel>,
}

impl GrpcRepo {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Repo(e.to_string()))?;
        Ok(Self {
            client: pb::repo_service_client::RepoServiceClient::new(channel),
        })
    }
}

/// Map a transport `Status` to a repo error.
fn repo_err(s: tonic::Status) -> agent_core::Error {
    agent_core::Error::Repo(s.to_string())
}

#[async_trait]
impl RepoBackend for GrpcRepo {
    async fn resolve(&self, rev: &Revision) -> Result<Oid> {
        let mut client = self.client.clone();
        let resp = client
            .resolve(outbound(pb::ResolveRequest {
                revision: rev.0.clone(),
            }))
            .await
            .map_err(repo_err)?;
        Ok(Oid(resp.into_inner().oid))
    }

    async fn read_file(&self, rev: &Revision, path: &std::path::Path) -> Result<BlobContent> {
        let mut client = self.client.clone();
        let resp = client
            .read_file(outbound(pb::ReadFileRequest {
                revision: rev.0.clone(),
                path: path.to_string_lossy().into_owned(),
            }))
            .await
            .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn list_tree(
        &self,
        rev: &Revision,
        path: &std::path::Path,
        recursive: bool,
    ) -> Result<Vec<TreeEntry>> {
        let mut client = self.client.clone();
        let resp = client
            .list_tree(outbound(pb::ListTreeRequest {
                revision: rev.0.clone(),
                path: path.to_string_lossy().into_owned(),
                recursive,
            }))
            .await
            .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .entries
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn diff(
        &self,
        base: &Revision,
        target: &Revision,
        path_globs: &[String],
    ) -> Result<DiffResult> {
        let mut client = self.client.clone();
        let resp = client
            .diff(outbound(pb::DiffRequest {
                base: base.0.clone(),
                target: target.0.clone(),
                path_globs: path_globs.to_vec(),
            }))
            .await
            .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn grep(
        &self,
        rev: &Revision,
        pattern: &str,
        path_globs: &[String],
        limit: usize,
    ) -> Result<Vec<GrepHit>> {
        let mut client = self.client.clone();
        let resp = client
            .grep(outbound(pb::GrepRequest {
                revision: rev.0.clone(),
                pattern: pattern.to_string(),
                path_globs: path_globs.to_vec(),
                limit: limit as u64,
            }))
            .await
            .map_err(repo_err)?;
        Ok(resp.into_inner().hits.into_iter().map(Into::into).collect())
    }

    async fn log(
        &self,
        rev: &Revision,
        path: Option<&std::path::Path>,
        limit: usize,
    ) -> Result<Vec<CommitInfo>> {
        let mut client = self.client.clone();
        let resp = client
            .log(outbound(pb::LogRequest {
                revision: rev.0.clone(),
                path: path.map(|p| p.to_string_lossy().into_owned()),
                limit: limit as u64,
            }))
            .await
            .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .commits
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn branches(&self) -> Result<Vec<(String, Oid)>> {
        let mut client = self.client.clone();
        let resp = client
            .branches(outbound(pb::BranchesRequest {}))
            .await
            .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .branches
            .into_iter()
            .map(|b| (b.name, Oid(b.oid)))
            .collect())
    }

    async fn status(&self) -> Result<RepoStatus> {
        let mut client = self.client.clone();
        let resp = client
            .status(outbound(pb::RepoStatusRequest {}))
            .await
            .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn fetch(&self) -> Result<RepoStatus> {
        let mut client = self.client.clone();
        let resp = client
            .fetch(outbound(pb::FetchRequest {}))
            .await
            .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle> {
        let mut client = self.client.clone();
        let resp = client
            .worktree_add(outbound(pb::WorktreeSpec::from(spec.clone())))
            .await
            .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>> {
        let mut client = self.client.clone();
        let resp = client
            .worktree_list(outbound(pb::WorktreeListRequest {}))
            .await
            .map_err(repo_err)?;
        Ok(resp
            .into_inner()
            .worktrees
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn worktree_remove(&self, id: &str) -> Result<()> {
        let mut client = self.client.clone();
        client
            .worktree_remove(outbound(pb::WorktreeRemoveRequest { id: id.to_string() }))
            .await
            .map_err(repo_err)?;
        Ok(())
    }

    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint> {
        let mut client = self.client.clone();
        let resp = client
            .create_checkpoint(outbound(pb::CheckpointRequest {
                worktree_id: worktree_id.to_string(),
                name: name.to_string(),
            }))
            .await
            .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn push(&self, checkpoint: &Checkpoint, remote_ref: &str) -> Result<()> {
        let mut client = self.client.clone();
        client
            .push(outbound(pb::PushRequest {
                checkpoint: Some(pb::Checkpoint::from(checkpoint.clone())),
                remote_ref: remote_ref.to_string(),
            }))
            .await
            .map_err(repo_err)?;
        Ok(())
    }
}

/// Connect to a remote tool worker, discover its tools (`DescribeAll`), and return
/// one `Arc<dyn Tool>` per remote tool (each calls `Execute`). Mirrors
/// `agent-mcp`'s `connect_tools`.
pub async fn grpc_tools(endpoint: &Endpoint) -> Result<Vec<Arc<dyn Tool>>> {
    let channel = endpoint
        .connect_lazy()
        .map_err(|e| agent_core::Error::Tool(e.to_string()))?;
    let mut client = pb::tool_service_client::ToolServiceClient::new(channel.clone());
    let resp = client
        .describe_all(outbound(pb::DescribeAllRequest {}))
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
        }));
    }
    Ok(tools)
}
