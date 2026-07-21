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

/// Default retry policy for the gRPC seam clients: the canonical backoff (+ full
/// jitter) with 3 attempts. Threading a `[grpc] max_retries` value here is a
/// trivial follow-up; the wiring below is the substance.
fn grpc_retry_policy() -> agent_retry::RetryPolicy {
    agent_retry::RetryPolicy::new(3)
}

/// Retry decision for a gRPC `Status`, in the shape `agent_retry::retry` wants:
/// retry the transient "overloaded" codes (`UNAVAILABLE` / `RESOURCE_EXHAUSTED`),
/// honouring the server's `grpc-retry-pushback-ms` hint (including its `-1`
/// "do not retry" sentinel); fail fast on every other status.
fn grpc_retry_decision(status: &tonic::Status) -> Option<Option<std::time::Duration>> {
    use agent_retry::grpc::Pushback;
    let retryable = agent_retry::grpc::retryable_code(status.code() as i32);
    match status
        .metadata()
        .get("grpc-retry-pushback-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(agent_retry::grpc::parse_pushback)
    {
        // The server explicitly forbids a retry — always honoured.
        Some(Pushback::DoNotRetry) => None,
        // A positive pushback only modulates the *delay* for a code we would already
        // retry. It must NOT let a hostile server force retries of a permanent error
        // (INVALID_ARGUMENT / PERMISSION_DENIED / …) by attaching a pushback header —
        // that is an amplification/abuse vector.
        Some(Pushback::RetryAfter(d)) if retryable => Some(Some(d)),
        Some(Pushback::RetryAfter(_)) => None,
        // No (usable) hint: fall back to code classification.
        None if retryable => Some(None),
        None => None,
    }
}

/// Run a **unary** gRPC call `op` through the canonical retry driver with the gRPC
/// classifier. `op` is re-invoked per attempt, so clone the request inside it.
/// (Streaming RPCs are intentionally not retried — a partial stream can't replay.)
async fn call_retry<T, Fut>(
    policy: &agent_retry::RetryPolicy,
    op: impl FnMut() -> Fut,
) -> std::result::Result<T, tonic::Status>
where
    Fut: std::future::Future<Output = std::result::Result<T, tonic::Status>>,
{
    agent_retry::retry(policy, grpc_retry_decision, op).await
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct GrpcProvider {
    client: pb::provider_client::ProviderClient<Channel>,
    caps: ModelCapabilities,
    retry: agent_retry::RetryPolicy,
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
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl LlmProvider for GrpcProvider {
    fn capabilities(&self) -> ModelCapabilities {
        self.caps.clone()
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let pbreq = pb::CompletionRequest::from(req);
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.complete(outbound(r)).await }
        })
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
    retry: agent_retry::RetryPolicy,
}

impl GrpcMemory {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Memory(e.to_string()))?;
        Ok(Self {
            client: pb::memory_client::MemoryClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl MemoryStore for GrpcMemory {
    async fn recall(&self, query: &RecallQuery) -> Result<Vec<MemoryItem>> {
        let pbreq = pb::RecallQuery::from(query.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.recall(outbound(r)).await }
        })
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
        let pbreq = pb::MemoryEvent::from(event);
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.append(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Memory(s.to_string()))?;
        Ok(())
    }

    async fn distill(&self) -> Result<usize> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.distill(outbound(pb::DistillRequest {})).await }
        })
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
    retry: agent_retry::RetryPolicy,
}

impl GrpcContext {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Provider(e.to_string()))?;
        Ok(Self {
            client: pb::context_service_client::ContextServiceClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl ContextStrategy for GrpcContext {
    async fn assemble(&self, input: ContextInput) -> Result<Vec<Message>> {
        let pbreq = pb::ContextInput::from(input);
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.assemble(outbound(r)).await }
        })
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
        let req = pb::CompactRequest {
            working: Some(std::mem::take(working).into()),
            budget: Some(budget.clone().into()),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.compact(outbound(r)).await }
        })
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
    retry: agent_retry::RetryPolicy,
}

impl GrpcPolicy {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Config(e.to_string()))?;
        Ok(Self {
            client: pb::policy_client::PolicyClient::new(channel),
            retry: grpc_retry_policy(),
        })
    }
}

#[async_trait]
impl Policy for GrpcPolicy {
    async fn authorize(&self, call: &agent_core::ToolCall) -> Decision {
        let pbreq = pb::ToolCall::from(call.clone());
        // Retry a transient policy-service blip before falling back; a persistent
        // failure fails safe (deny) rather than silently allowing.
        match call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = pbreq.clone();
            async move { client.authorize(outbound(r)).await }
        })
        .await
        {
            Ok(resp) => resp.into_inner().into(),
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
    retry: agent_retry::RetryPolicy,
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
        let req = pb::ExecuteRequest {
            name: self.schema.name.clone(),
            arguments: Some(args.into()),
            context: Some(ctx.into()),
        };
        // Mirror `McpTool`: transport failures surface as an error observation, not
        // a hard `Err`, so one flaky worker doesn't abort the turn — but retry a
        // transient overload/availability blip first.
        match call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.execute(outbound(r)).await }
        })
        .await
        {
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
    retry: agent_retry::RetryPolicy,
}

impl GrpcSearch {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Search(e.to_string()))?;
        Ok(Self {
            client: pb::search_service_client::SearchServiceClient::new(channel),
            retry: grpc_retry_policy(),
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
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move {
                client
                    .status(outbound(pb::StatusRequest {
                        backend: String::new(),
                    }))
                    .await
            }
        })
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
        let req = pb::SearchRequest {
            query: Some(pb::SearchQuery::from(q.clone())),
            backend: String::new(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.search(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Search(s.to_string()))?;
        Ok(resp.into_inner().hits.into_iter().map(Into::into).collect())
    }

    async fn list_files(&self, globs: &[String]) -> Result<Vec<std::path::PathBuf>> {
        let req = pb::ListFilesRequest {
            globs: globs.to_vec(),
            backend: String::new(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.list_files(outbound(r)).await }
        })
        .await
        .map_err(|s| agent_core::Error::Search(s.to_string()))?;
        Ok(resp
            .into_inner()
            .paths
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect())
    }
}

/// A `RepoBackend` that calls a remote `RepoService` (multi-branch git gateway).
pub struct GrpcRepo {
    client: pb::repo_service_client::RepoServiceClient<Channel>,
    retry: agent_retry::RetryPolicy,
}

impl GrpcRepo {
    pub fn connect(endpoint: &Endpoint) -> Result<Self> {
        let channel = endpoint
            .connect_lazy()
            .map_err(|e| agent_core::Error::Repo(e.to_string()))?;
        Ok(Self {
            client: pb::repo_service_client::RepoServiceClient::new(channel),
            retry: grpc_retry_policy(),
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
        let req = pb::ResolveRequest {
            revision: rev.0.clone(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.resolve(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(Oid(resp.into_inner().oid))
    }

    async fn read_file(&self, rev: &Revision, path: &std::path::Path) -> Result<BlobContent> {
        let req = pb::ReadFileRequest {
            revision: rev.0.clone(),
            path: path.to_string_lossy().into_owned(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.read_file(outbound(r)).await }
        })
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
        let req = pb::ListTreeRequest {
            revision: rev.0.clone(),
            path: path.to_string_lossy().into_owned(),
            recursive,
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.list_tree(outbound(r)).await }
        })
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
        let req = pb::DiffRequest {
            base: base.0.clone(),
            target: target.0.clone(),
            path_globs: path_globs.to_vec(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.diff(outbound(r)).await }
        })
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
        let req = pb::GrepRequest {
            revision: rev.0.clone(),
            pattern: pattern.to_string(),
            path_globs: path_globs.to_vec(),
            limit: limit as u64,
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.grep(outbound(r)).await }
        })
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
        let req = pb::LogRequest {
            revision: rev.0.clone(),
            path: path.map(|p| p.to_string_lossy().into_owned()),
            limit: limit as u64,
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.log(outbound(r)).await }
        })
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
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.branches(outbound(pb::BranchesRequest {})).await }
        })
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
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.status(outbound(pb::RepoStatusRequest {})).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn fetch(&self) -> Result<RepoStatus> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move { client.fetch(outbound(pb::FetchRequest {})).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn worktree_add(&self, spec: &WorktreeSpec) -> Result<WorktreeHandle> {
        let req = pb::WorktreeSpec::from(spec.clone());
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.worktree_add(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn worktree_list(&self) -> Result<Vec<WorktreeHandle>> {
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            async move {
                client
                    .worktree_list(outbound(pb::WorktreeListRequest {}))
                    .await
            }
        })
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
        let req = pb::WorktreeRemoveRequest { id: id.to_string() };
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.worktree_remove(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(())
    }

    async fn checkpoint(&self, worktree_id: &str, name: &str) -> Result<Checkpoint> {
        let req = pb::CheckpointRequest {
            worktree_id: worktree_id.to_string(),
            name: name.to_string(),
        };
        let resp = call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.create_checkpoint(outbound(r)).await }
        })
        .await
        .map_err(repo_err)?;
        Ok(resp.into_inner().into())
    }

    async fn push(&self, checkpoint: &Checkpoint, remote_ref: &str) -> Result<()> {
        let req = pb::PushRequest {
            checkpoint: Some(pb::Checkpoint::from(checkpoint.clone())),
            remote_ref: remote_ref.to_string(),
        };
        call_retry(&self.retry, || {
            let mut client = self.client.clone();
            let r = req.clone();
            async move { client.push(outbound(r)).await }
        })
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
    let client = pb::tool_service_client::ToolServiceClient::new(channel.clone());
    let policy = grpc_retry_policy();
    let resp = call_retry(&policy, || {
        let mut client = client.clone();
        async move {
            client
                .describe_all(outbound(pb::DescribeAllRequest {}))
                .await
        }
    })
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
            retry: grpc_retry_policy(),
        }));
    }
    Ok(tools)
}

#[cfg(test)]
mod retry_decision_tests {
    use super::grpc_retry_decision;
    use agent_retry::grpc::code;
    use rstest::rstest;
    use std::time::Duration;

    /// Build a `Status` with `code` and an optional `grpc-retry-pushback-ms` value.
    fn status(code: i32, pushback: Option<&str>) -> tonic::Status {
        let mut s = tonic::Status::new(tonic::Code::from_i32(code), "test");
        if let Some(v) = pushback {
            s.metadata_mut()
                .insert("grpc-retry-pushback-ms", v.parse().unwrap());
        }
        s
    }

    // The result shape: `None` = don't retry, `Some(None)` = retry with computed
    // backoff, `Some(Some(d))` = retry after the server's delay.
    #[rstest]
    #[case::positive_retryable_no_hint(code::UNAVAILABLE, None, Some(None))]
    #[case::positive_resource_exhausted(code::RESOURCE_EXHAUSTED, None, Some(None))]
    #[case::positive_pushback_delay(
        code::UNAVAILABLE,
        Some("100"),
        Some(Some(Duration::from_millis(100)))
    )]
    #[case::negative_nonretryable_no_hint(code::INVALID_ARGUMENT, None, None)]
    #[case::negative_permission_denied(code::PERMISSION_DENIED, None, None)]
    // Adversarial: `-1` must veto a retry even on a retryable code.
    #[case::adversarial_do_not_retry_overrides(code::UNAVAILABLE, Some("-1"), None)]
    // Adversarial: a positive pushback must NOT force a retry of a permanent error.
    #[case::adversarial_pushback_on_nonretryable(code::INVALID_ARGUMENT, Some("2500"), None)]
    // Corner: an unparseable/garbage hint falls back to code classification.
    #[case::corner_garbage_hint_falls_through(code::UNAVAILABLE, Some("later"), Some(None))]
    #[case::corner_other_negative_not_sentinel(code::UNAVAILABLE, Some("-2"), Some(None))]
    fn decision_cases(
        #[case] code: i32,
        #[case] pushback: Option<&str>,
        #[case] expected: Option<Option<Duration>>,
    ) {
        assert_eq!(grpc_retry_decision(&status(code, pushback)), expected);
    }
}
