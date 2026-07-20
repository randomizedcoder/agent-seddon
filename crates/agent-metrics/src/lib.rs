//! Prometheus metrics for a running agent — the shared registry + handles.
//!
//! `Metrics` owns a `prometheus::Registry` and every metric handle. It is a
//! cheap `Clone` (each handle is `Arc`-backed) and is threaded into **every seam
//! impl** — the providers, tools, memory, context, policy, MCP and gRPC
//! transports each hold a copy and record their own timings/counts into the one
//! registry. The `agent-runtime` loop also records the top-level run/iteration
//! metrics. Whatever a given process runs, its `/metrics` endpoint emits.
//!
//! Instrumentation is unconditional and cheap; only *serving* the endpoint (or
//! pushing to a Pushgateway) is gated by config, so when metrics are disabled the
//! registry simply goes unscraped. Metric names follow the tracing span tree in
//! `docs/tracing.md`, so a span and its metric line up by component + operation.
//!
//! This crate lives below the seams (it only depends on `prometheus`) so an impl
//! crate can hold a `Metrics` without a cycle back through `agent-runtime`.

use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Opts, Registry, TextEncoder,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,

    // --- loop-level (recorded by agent-runtime) ---------------------------
    api_calls: IntCounterVec,
    api_call_seconds: HistogramVec,
    tokens: IntCounterVec,
    context_tokens: IntGauge,
    context_messages: IntGauge,
    tool_calls: IntCounterVec,
    iterations: IntCounter,
    runs: IntCounterVec,
    run_seconds: Histogram,
    active: IntGauge,

    // --- provider (recorded inside agent-providers) -----------------------
    provider_request_seconds: HistogramVec,
    provider_ttft_seconds: HistogramVec,
    provider_stream_chunks: IntCounterVec,
    provider_errors: IntCounterVec,

    // --- tools (recorded inside agent-tools) ------------------------------
    tool_exec_seconds: HistogramVec,
    tool_errors: IntCounterVec,

    // --- memory (recorded inside agent-memory) ----------------------------
    memory_op_seconds: HistogramVec,
    memory_recall_items: Histogram,
    memory_errors: IntCounterVec,

    // --- context (recorded inside agent-context) --------------------------
    context_op_seconds: HistogramVec,
    context_compactions: IntCounter,
    context_compact_tokens: IntGaugeVec,

    // --- policy (recorded by the policy metrics wrapper) ------------------
    policy_authorize: IntCounterVec,
    policy_authorize_seconds: Histogram,
    policy_guard: IntCounterVec,

    // --- search (recorded by the search metrics wrapper) ------------------
    // Labelled by `backend` so tantivy vs. a second backend can be compared
    // head-to-head under the same interface.
    search_query_seconds: HistogramVec,
    search_hits: HistogramVec,
    search_index_seconds: HistogramVec,
    search_index_files: IntGaugeVec,
    search_index_fresh: IntGaugeVec,
    search_errors: IntCounterVec,
    search_reindex: IntCounterVec,

    // --- git / repo (recorded by the repo metrics wrapper) ----------------
    // Labelled by `backend` (cli/hybrid/grpc), like the search families.
    repo_op_seconds: HistogramVec,
    repo_errors: IntCounterVec,
    repo_worktrees: IntGaugeVec,
    repo_fetch_seconds: HistogramVec,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        // --- loop-level -------------------------------------------------------
        let api_calls = IntCounterVec::new(
            Opts::new("agent_api_calls_total", "LLM completion calls"),
            &["model", "finish_reason"],
        )
        .unwrap();
        let api_call_seconds = HistogramVec::new(
            HistogramOpts::new("agent_api_call_duration_seconds", "LLM call latency"),
            &["model"],
        )
        .unwrap();
        let tokens = IntCounterVec::new(
            Opts::new("agent_tokens_total", "Tokens consumed"),
            &["model", "kind"],
        )
        .unwrap();
        let context_tokens = IntGauge::new(
            "agent_context_tokens",
            "Prompt tokens of the last request (context size)",
        )
        .unwrap();
        let context_messages = IntGauge::new(
            "agent_context_messages",
            "Messages in the working set of the last request",
        )
        .unwrap();
        let tool_calls = IntCounterVec::new(
            Opts::new("agent_tool_calls_total", "Tool invocations"),
            &["tool", "status"],
        )
        .unwrap();
        let iterations =
            IntCounter::new("agent_iterations_total", "Agent loop iterations").unwrap();
        let runs = IntCounterVec::new(
            Opts::new("agent_runs_total", "Completed agent runs"),
            &["outcome"],
        )
        .unwrap();
        let run_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_run_duration_seconds",
            "Wall-clock duration of an agent run",
        ))
        .unwrap();
        let active = IntGauge::new("agent_active", "1 while a run is in progress").unwrap();

        // --- provider ---------------------------------------------------------
        let provider_request_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_provider_request_seconds",
                "LlmProvider request latency (measured inside the provider impl)",
            ),
            &["provider", "stream"],
        )
        .unwrap();
        let provider_ttft_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_provider_ttft_seconds",
                "Streaming time-to-first-token",
            ),
            &["provider"],
        )
        .unwrap();
        let provider_stream_chunks = IntCounterVec::new(
            Opts::new(
                "agent_provider_stream_chunks_total",
                "Streaming chunks received from the provider",
            ),
            &["provider"],
        )
        .unwrap();
        let provider_errors = IntCounterVec::new(
            Opts::new("agent_provider_errors_total", "Provider request errors"),
            &["provider", "kind"],
        )
        .unwrap();

        // --- tools ------------------------------------------------------------
        let tool_exec_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_tool_exec_seconds",
                "Tool execution latency (measured inside the tool impl)",
            ),
            &["tool"],
        )
        .unwrap();
        let tool_errors = IntCounterVec::new(
            Opts::new("agent_tool_errors_total", "Tool execution errors"),
            &["tool", "kind"],
        )
        .unwrap();

        // --- memory -----------------------------------------------------------
        let memory_op_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_memory_op_seconds",
                "Memory operation latency (recall/append/distill)",
            ),
            &["op"],
        )
        .unwrap();
        let memory_recall_items = Histogram::with_opts(HistogramOpts::new(
            "agent_memory_recall_items",
            "Items returned by a memory recall",
        ))
        .unwrap();
        let memory_errors = IntCounterVec::new(
            Opts::new("agent_memory_errors_total", "Memory operation errors"),
            &["op"],
        )
        .unwrap();

        // --- context ----------------------------------------------------------
        let context_op_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_context_op_seconds",
                "Context strategy latency (assemble/compact)",
            ),
            &["op"],
        )
        .unwrap();
        let context_compactions =
            IntCounter::new("agent_context_compactions_total", "Context compactions run").unwrap();
        let context_compact_tokens = IntGaugeVec::new(
            Opts::new(
                "agent_context_compact_tokens",
                "Token count around the last compaction",
            ),
            &["when"],
        )
        .unwrap();

        // --- policy -----------------------------------------------------------
        let policy_authorize = IntCounterVec::new(
            Opts::new("agent_policy_authorize_total", "Policy authorize decisions"),
            &["policy", "decision"],
        )
        .unwrap();
        let policy_authorize_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_policy_authorize_seconds",
            "Policy authorize latency",
        ))
        .unwrap();
        // Guard hits: a dangerous-command / sensitive-path match, labelled by the
        // rule category and the action taken (deny / prompt / allowed-after-prompt).
        let policy_guard = IntCounterVec::new(
            Opts::new(
                "agent_policy_guard_total",
                "Policy guard matches (dangerous command / sensitive path)",
            ),
            &["category", "action"],
        )
        .unwrap();

        // --- search -----------------------------------------------------------
        let search_query_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_search_query_seconds",
                "Search query latency (measured inside the backend)",
            ),
            &["backend", "mode"],
        )
        .unwrap();
        let search_hits = HistogramVec::new(
            HistogramOpts::new("agent_search_hits", "Hits returned by a search query"),
            &["backend", "mode"],
        )
        .unwrap();
        let search_index_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_search_index_seconds",
                "Reindex (build/update) duration",
            ),
            &["backend"],
        )
        .unwrap();
        let search_index_files = IntGaugeVec::new(
            Opts::new("agent_search_index_files", "Files in the search index"),
            &["backend"],
        )
        .unwrap();
        let search_index_fresh = IntGaugeVec::new(
            Opts::new(
                "agent_search_index_fresh",
                "1 when the index is fresh with the working tree, else 0",
            ),
            &["backend"],
        )
        .unwrap();
        let search_errors = IntCounterVec::new(
            Opts::new("agent_search_errors_total", "Search operation errors"),
            &["backend", "op"],
        )
        .unwrap();
        let search_reindex = IntCounterVec::new(
            Opts::new("agent_search_reindex_total", "Reindex runs"),
            &["backend", "trigger"],
        )
        .unwrap();

        // --- git / repo -------------------------------------------------------
        let repo_op_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_repo_op_seconds",
                "RepoBackend operation latency (measured inside the backend)",
            ),
            &["backend", "op"],
        )
        .unwrap();
        let repo_errors = IntCounterVec::new(
            Opts::new("agent_repo_errors_total", "RepoBackend operation errors"),
            &["backend", "op"],
        )
        .unwrap();
        let repo_worktrees = IntGaugeVec::new(
            Opts::new("agent_repo_worktrees_live", "Live disposable worktrees"),
            &["backend"],
        )
        .unwrap();
        let repo_fetch_seconds = HistogramVec::new(
            HistogramOpts::new("agent_repo_fetch_seconds", "Mirror fetch duration"),
            &["backend"],
        )
        .unwrap();

        let collectors: Vec<Box<dyn prometheus::core::Collector>> = vec![
            Box::new(api_calls.clone()),
            Box::new(api_call_seconds.clone()),
            Box::new(tokens.clone()),
            Box::new(context_tokens.clone()),
            Box::new(context_messages.clone()),
            Box::new(tool_calls.clone()),
            Box::new(iterations.clone()),
            Box::new(runs.clone()),
            Box::new(run_seconds.clone()),
            Box::new(active.clone()),
            Box::new(provider_request_seconds.clone()),
            Box::new(provider_ttft_seconds.clone()),
            Box::new(provider_stream_chunks.clone()),
            Box::new(provider_errors.clone()),
            Box::new(tool_exec_seconds.clone()),
            Box::new(tool_errors.clone()),
            Box::new(memory_op_seconds.clone()),
            Box::new(memory_recall_items.clone()),
            Box::new(memory_errors.clone()),
            Box::new(context_op_seconds.clone()),
            Box::new(context_compactions.clone()),
            Box::new(context_compact_tokens.clone()),
            Box::new(policy_authorize.clone()),
            Box::new(policy_authorize_seconds.clone()),
            Box::new(policy_guard.clone()),
            Box::new(search_query_seconds.clone()),
            Box::new(search_hits.clone()),
            Box::new(search_index_seconds.clone()),
            Box::new(search_index_files.clone()),
            Box::new(search_index_fresh.clone()),
            Box::new(search_errors.clone()),
            Box::new(search_reindex.clone()),
            Box::new(repo_op_seconds.clone()),
            Box::new(repo_errors.clone()),
            Box::new(repo_worktrees.clone()),
            Box::new(repo_fetch_seconds.clone()),
        ];
        for m in collectors {
            registry.register(m).expect("register metric");
        }

        Self {
            registry: Arc::new(registry),
            api_calls,
            api_call_seconds,
            tokens,
            context_tokens,
            context_messages,
            tool_calls,
            iterations,
            runs,
            run_seconds,
            active,
            provider_request_seconds,
            provider_ttft_seconds,
            provider_stream_chunks,
            provider_errors,
            tool_exec_seconds,
            tool_errors,
            memory_op_seconds,
            memory_recall_items,
            memory_errors,
            context_op_seconds,
            context_compactions,
            context_compact_tokens,
            policy_authorize,
            policy_authorize_seconds,
            policy_guard,
            search_query_seconds,
            search_hits,
            search_index_seconds,
            search_index_files,
            search_index_fresh,
            search_errors,
            search_reindex,
            repo_op_seconds,
            repo_errors,
            repo_worktrees,
            repo_fetch_seconds,
        }
    }

    /// Encode all metrics in the Prometheus text exposition format.
    pub fn encode_text(&self) -> String {
        let mut buf = Vec::new();
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        let _ = encoder.encode(&families, &mut buf);
        String::from_utf8(buf).unwrap_or_default()
    }

    // --- loop-level instrumentation ---------------------------------------

    pub fn run_started(&self) {
        self.active.set(1);
    }
    pub fn run_finished(&self, outcome: &str, seconds: f64) {
        self.active.set(0);
        self.runs.with_label_values(&[outcome]).inc();
        self.run_seconds.observe(seconds);
    }
    pub fn on_iteration(&self) {
        self.iterations.inc();
    }
    pub fn on_api_call(&self, model: &str, finish_reason: &str, seconds: f64) {
        self.api_calls
            .with_label_values(&[model, finish_reason])
            .inc();
        self.api_call_seconds
            .with_label_values(&[model])
            .observe(seconds);
    }
    pub fn add_tokens(&self, model: &str, prompt: u64, completion: u64) {
        self.tokens
            .with_label_values(&[model, "prompt"])
            .inc_by(prompt);
        self.tokens
            .with_label_values(&[model, "completion"])
            .inc_by(completion);
    }
    pub fn set_context(&self, prompt_tokens: i64, messages: i64) {
        self.context_tokens.set(prompt_tokens);
        self.context_messages.set(messages);
    }
    pub fn on_tool(&self, tool: &str, status: &str) {
        self.tool_calls.with_label_values(&[tool, status]).inc();
    }

    // --- provider instrumentation -----------------------------------------

    /// Record a completed provider request. `stream` distinguishes the streaming
    /// path from the buffered one.
    pub fn on_provider_request(&self, provider: &str, stream: bool, seconds: f64) {
        self.provider_request_seconds
            .with_label_values(&[provider, bool_label(stream)])
            .observe(seconds);
    }
    /// Record streaming time-to-first-token.
    pub fn on_provider_ttft(&self, provider: &str, seconds: f64) {
        self.provider_ttft_seconds
            .with_label_values(&[provider])
            .observe(seconds);
    }
    /// Count streaming chunks received (call once per chunk, or batched via `n`).
    pub fn add_provider_chunks(&self, provider: &str, n: u64) {
        self.provider_stream_chunks
            .with_label_values(&[provider])
            .inc_by(n);
    }
    /// Count a provider error, tagged with a coarse `kind` (e.g. `http`, `parse`).
    pub fn on_provider_error(&self, provider: &str, kind: &str) {
        self.provider_errors
            .with_label_values(&[provider, kind])
            .inc();
    }

    // --- tool instrumentation ---------------------------------------------

    pub fn on_tool_exec(&self, tool: &str, seconds: f64) {
        self.tool_exec_seconds
            .with_label_values(&[tool])
            .observe(seconds);
    }
    pub fn on_tool_error(&self, tool: &str, kind: &str) {
        self.tool_errors.with_label_values(&[tool, kind]).inc();
    }

    // --- memory instrumentation -------------------------------------------

    pub fn on_memory_op(&self, op: &str, seconds: f64) {
        self.memory_op_seconds
            .with_label_values(&[op])
            .observe(seconds);
    }
    pub fn observe_recall_items(&self, n: usize) {
        self.memory_recall_items.observe(n as f64);
    }
    pub fn on_memory_error(&self, op: &str) {
        self.memory_errors.with_label_values(&[op]).inc();
    }

    // --- context instrumentation ------------------------------------------

    pub fn on_context_op(&self, op: &str, seconds: f64) {
        self.context_op_seconds
            .with_label_values(&[op])
            .observe(seconds);
    }
    /// Record a compaction, capturing the token count before and after.
    pub fn on_compaction(&self, before: i64, after: i64) {
        self.context_compactions.inc();
        self.context_compact_tokens
            .with_label_values(&["before"])
            .set(before);
        self.context_compact_tokens
            .with_label_values(&["after"])
            .set(after);
    }

    // --- policy instrumentation -------------------------------------------

    pub fn on_authorize(&self, policy: &str, decision: &str, seconds: f64) {
        self.policy_authorize
            .with_label_values(&[policy, decision])
            .inc();
        self.policy_authorize_seconds.observe(seconds);
    }

    /// A guard rule matched a call: `category` is the rule family
    /// (`dangerous_command` / `sensitive_path`), `action` is what happened
    /// (`deny` / `prompt_denied` / `prompt_allowed`).
    pub fn on_policy_guard(&self, category: &str, action: &str) {
        self.policy_guard
            .with_label_values(&[category, action])
            .inc();
    }

    // --- search instrumentation -------------------------------------------

    /// Record a completed search query: latency + the number of hits, both
    /// labelled by backend + query mode for head-to-head comparison.
    pub fn on_search_query(&self, backend: &str, mode: &str, seconds: f64, hits: usize) {
        self.search_query_seconds
            .with_label_values(&[backend, mode])
            .observe(seconds);
        self.search_hits
            .with_label_values(&[backend, mode])
            .observe(hits as f64);
    }
    /// Record a completed reindex: duration + the resulting file count, and mark
    /// the index fresh. Timed at the seam boundary (the metrics wrapper).
    pub fn observe_reindex(&self, backend: &str, seconds: f64, files: i64) {
        self.search_index_seconds
            .with_label_values(&[backend])
            .observe(seconds);
        self.set_search_files(backend, files);
        self.set_search_fresh(backend, true);
    }
    /// Set the indexed-file-count gauge (also refreshed by `status()` so the count
    /// is populated even when the index was already fresh and no reindex ran).
    pub fn set_search_files(&self, backend: &str, files: i64) {
        self.search_index_files
            .with_label_values(&[backend])
            .set(files);
    }
    /// Count a reindex run, tagged with what triggered it (`startup`/`manual`).
    /// Called by whoever initiates the reindex (it knows the trigger).
    pub fn on_search_reindex(&self, backend: &str, trigger: &str) {
        self.search_reindex
            .with_label_values(&[backend, trigger])
            .inc();
    }
    /// Set the index-freshness gauge (1 = fresh, 0 = stale/missing/building).
    pub fn set_search_fresh(&self, backend: &str, fresh: bool) {
        self.search_index_fresh
            .with_label_values(&[backend])
            .set(fresh as i64);
    }
    /// Count a search error, tagged with the operation (`query`/`status`/`reindex`).
    pub fn on_search_error(&self, backend: &str, op: &str) {
        self.search_errors.with_label_values(&[backend, op]).inc();
    }

    // --- repo (git seam) instrumentation ----------------------------------

    /// Record a RepoBackend operation's latency, labelled by backend + op name.
    pub fn on_repo_op(&self, backend: &str, op: &str, seconds: f64) {
        self.repo_op_seconds
            .with_label_values(&[backend, op])
            .observe(seconds);
    }
    /// Count a RepoBackend error, tagged with the operation.
    pub fn on_repo_error(&self, backend: &str, op: &str) {
        self.repo_errors.with_label_values(&[backend, op]).inc();
    }
    /// Set the live-worktree gauge (refreshed on `status`/`worktree_list`).
    pub fn set_repo_worktrees(&self, backend: &str, n: i64) {
        self.repo_worktrees.with_label_values(&[backend]).set(n);
    }
    /// Record a mirror fetch's duration.
    pub fn observe_repo_fetch(&self, backend: &str, seconds: f64) {
        self.repo_fetch_seconds
            .with_label_values(&[backend])
            .observe(seconds);
    }
}

fn bool_label(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_incremented_metrics() {
        let m = Metrics::new();
        m.on_iteration();
        m.on_api_call("test-model", "stop", 0.5);
        m.add_tokens("test-model", 100, 20);
        m.set_context(100, 4);
        m.on_tool("bash", "ok");
        m.run_finished("success", 1.5);

        let text = m.encode_text();
        for name in [
            "agent_iterations_total",
            "agent_api_calls_total",
            "agent_tokens_total",
            "agent_context_tokens",
            "agent_tool_calls_total",
            "agent_runs_total",
        ] {
            assert!(text.contains(name), "missing metric `{name}` in:\n{text}");
        }
        assert!(text.contains("test-model"));
    }

    #[test]
    fn encodes_per_component_metrics() {
        let m = Metrics::new();
        m.on_provider_request("anthropic", true, 0.4);
        m.on_provider_ttft("anthropic", 0.1);
        m.add_provider_chunks("anthropic", 7);
        m.on_provider_error("anthropic", "http");
        m.on_tool_exec("bash", 0.02);
        m.on_tool_error("edit", "not_found");
        m.on_memory_op("recall", 0.003);
        m.observe_recall_items(3);
        m.on_memory_error("append");
        m.on_context_op("assemble", 0.001);
        m.on_compaction(9000, 4000);
        m.on_authorize("auto-approve", "approved", 0.0001);
        m.on_search_query("tantivy", "literal", 0.002, 5);
        m.observe_reindex("tantivy", 0.5, 120);
        m.on_search_reindex("tantivy", "startup");
        m.set_search_fresh("tantivy", true);
        m.on_search_error("tantivy", "query");
        m.on_repo_op("cli", "diff", 0.01);
        m.on_repo_error("cli", "read_file");
        m.set_repo_worktrees("cli", 2);
        m.observe_repo_fetch("cli", 0.3);

        let text = m.encode_text();
        for name in [
            "agent_provider_request_seconds",
            "agent_provider_ttft_seconds",
            "agent_provider_stream_chunks_total",
            "agent_provider_errors_total",
            "agent_tool_exec_seconds",
            "agent_tool_errors_total",
            "agent_memory_op_seconds",
            "agent_memory_recall_items",
            "agent_memory_errors_total",
            "agent_context_op_seconds",
            "agent_context_compactions_total",
            "agent_context_compact_tokens",
            "agent_policy_authorize_total",
            "agent_policy_authorize_seconds",
            "agent_search_query_seconds",
            "agent_search_hits",
            "agent_search_index_seconds",
            "agent_search_index_files",
            "agent_search_index_fresh",
            "agent_search_errors_total",
            "agent_search_reindex_total",
            "agent_repo_op_seconds",
            "agent_repo_errors_total",
            "agent_repo_worktrees_live",
            "agent_repo_fetch_seconds",
        ] {
            assert!(text.contains(name), "missing metric `{name}` in:\n{text}");
        }
    }
}
