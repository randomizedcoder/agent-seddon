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
    CounterVec, Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec,
    IntGauge, IntGaugeVec, Opts, Registry, TextEncoder,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,

    // --- loop-level (recorded by agent-runtime) ---------------------------
    api_calls: IntCounterVec,
    api_call_seconds: HistogramVec,
    tokens: IntCounterVec,
    // USD cost + cache-token accounting (recorded once a price table is applied,
    // see agent-tokenizer + parity spec 23). `cost_usd` is a float counter (money);
    // `cache_tokens` splits prompt-cache reads/writes so the cache-hit ratio
    // (cache_read / (cache_read + input)) is derivable in PromQL.
    cost_usd: CounterVec,
    cache_tokens: IntCounterVec,
    context_tokens: IntGauge,
    context_messages: IntGauge,
    tool_calls: IntCounterVec,
    // Tool-call verifier verdicts by verifier name, verdict (allow|revise|deny),
    // and mode (shadow|enforce). Labels are bounded enums + built-in verifier
    // names — never model-controlled free text.
    verifier_verdicts: IntCounterVec,
    // Multimodal content accounting (parity spec 26): blocks sent to the model by
    // modality, and blocks dropped because the model lacks vision support.
    content_blocks: IntCounterVec,
    // Content security scanning (parity spec 18): findings by severity/rule/kind,
    // and scan latency. Labels are bounded enums + built-in rule ids — never the
    // scanned content.
    // Prompt-cache anchors placed, by strategy (parity spec 24). Read alongside
    // `agent_cache_tokens_total` to tell a low hit-rate caused by bad placement
    // apart from one caused by a merely cold cache.
    // Live web search (parity spec 12): per-backend outcome, latency, and the
    // number of results returned. Labels are the configured backend name — never
    // the query text or the API key.
    // Provider routing (parity spec 25): which target a request went to, and
    // how often the router fell over or skipped an unhealthy candidate. Labels
    // are the configured candidate names — bounded by config, never user input.
    // Lifecycle hook dispatches (parity spec 22), by hook name and attachment
    // point. Labels are bounded by config + the fixed point set.
    // Forge API calls (parity spec 27), by backend, operation, and outcome.
    // Labels are bounded enums — never a token, URL, or remote content.
    // Scheduled runs (parity spec 28), by outcome — including `skipped`, so a
    // dropped overlapping fire is visible rather than silent.
    // Interactive terminal sessions (parity spec 29): a live gauge, byte
    // volumes by direction, and session outcomes.
    pty_active: IntGauge,
    pty_bytes: IntCounterVec,
    pty_sessions: IntCounterVec,
    scheduled_runs: IntCounterVec,
    scheduled_seconds: Histogram,
    forge_calls: IntCounterVec,
    forge_seconds: HistogramVec,
    hook_dispatches: IntCounterVec,
    route_decisions: IntCounterVec,
    // LLM pool (docs/design/code-review/llm-pool.md).
    pool_members_alive: IntGaugeVec,
    pool_probe_seconds: HistogramVec,
    pool_dispatch_seconds: HistogramVec,
    pool_member_calls: IntCounterVec,
    // Code review flow (docs/design/code-review/).
    review_collect_seconds: Histogram,
    review_collector_seconds: HistogramVec,
    review_collectors: IntCounterVec,
    review_change_files: Histogram,
    review_gitstate: IntCounterVec,
    review_findings: IntCounterVec,
    review_signatures: IntCounterVec,
    review_callgraph_nodes: Histogram,
    review_callgraph_edges: Histogram,
    review_style_conformance: IntCounterVec,
    review_summaries: IntCounterVec,
    review_cochange: IntCounterVec,
    review_churn: IntCounterVec,
    review_salience: IntCounterVec,
    review_runs: IntCounterVec,
    review_total_duration: HistogramVec,
    review_parallelism: Histogram,
    web_searches: IntCounterVec,
    web_search_seconds: HistogramVec,
    web_search_results: IntCounterVec,
    cache_breakpoints: IntCounterVec,
    scanner_findings: IntCounterVec,
    scan_seconds: Histogram,
    content_blocks_dropped: IntCounter,
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

    // --- web (web_fetch seam) ---------------------------------------------
    // Deliberately NOT labelled by host: the model is untrusted and chooses the
    // URL, so a host label is an unbounded-cardinality Prometheus DoS vector.
    // The host lands on the `web.fetch` span (per-trace, not an accumulating
    // series) instead. Only the outcome is labelled here.
    web_fetch_total: IntCounterVec,
    web_fetch_seconds: Histogram,
    web_fetch_bytes: Histogram,

    // --- tasks (TaskTracker seam) -----------------------------------------
    // Plan progress as a graphable signal: open (pending + in_progress) vs closed
    // (completed + cancelled), refreshed on every write/update/clear.
    tasks_open: IntGauge,
    tasks_closed: IntGauge,

    // --- structured output (OutputSchema seam) ----------------------------
    // Per-completion outcome (pass / repaired / exhausted) + validation latency.
    structured_total: IntCounterVec,
    structured_validate_seconds: Histogram,

    // --- lsp (LspBackend seam) --------------------------------------------
    // Per-method request latency + errors, and diagnostics by severity. Labels
    // (method, severity) are bounded enums — safe cardinality.
    lsp_request_seconds: HistogramVec,
    lsp_errors: IntCounterVec,
    lsp_diagnostics: IntCounterVec,

    // --- sandbox (Sandbox seam) -------------------------------------------
    // Per-backend exec latency + outcome. `backend` is a config-bounded label.
    sandbox_exec_seconds: HistogramVec,
    sandbox_exec_total: IntCounterVec,

    // --- embed (Embedder seam) --------------------------------------------
    // Per-backend embed latency + batch size (config-bounded `backend` label).
    embed_seconds: HistogramVec,
    embed_batch: HistogramVec,

    // --- session (SessionStore seam) --------------------------------------
    // Session-history mutations by op (checkpoint/restore/branch/undo/fork/prune)
    // + GC objects reclaimed. `op` is a bounded enum.
    session_ops: IntCounterVec,
    session_gc_reclaimed: IntCounter,

    // --- reference (ReferenceResolver seam) -------------------------------
    // `@`-mention expansion latency, refs resolved by (kind, outcome), and
    // budget-blocked expansions. `kind` (file/dir/symbol/url) + `outcome`
    // (block/warn) are bounded enums — never the attacker-controlled target.
    reference_resolve_seconds: Histogram,
    reference_refs: IntCounterVec,
    reference_blocked: IntCounter,
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
        let cost_usd = CounterVec::new(
            Opts::new("agent_cost_usd_total", "Cumulative USD cost by billed line"),
            &["model", "kind"],
        )
        .unwrap();
        let cache_tokens = IntCounterVec::new(
            Opts::new(
                "agent_cache_tokens_total",
                "Prompt-cache tokens (read = hit, write = created)",
            ),
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
        let verifier_verdicts = IntCounterVec::new(
            Opts::new(
                "agent_verifier_verdicts_total",
                "Tool-call verifier verdicts",
            ),
            &["verifier", "verdict", "mode"],
        )
        .unwrap();
        let pty_active = IntGauge::new("agent_pty_active_sessions", "Live pty sessions").unwrap();
        let pty_bytes = IntCounterVec::new(
            Opts::new("agent_pty_bytes_total", "Bytes through pty sessions"),
            &["direction"],
        )
        .unwrap();
        let pty_sessions = IntCounterVec::new(
            Opts::new("agent_pty_sessions_total", "Pty sessions, by outcome"),
            &["outcome"],
        )
        .unwrap();
        let scheduled_runs = IntCounterVec::new(
            Opts::new("agent_scheduled_runs_total", "Scheduled runs, by outcome"),
            &["outcome"],
        )
        .unwrap();
        let scheduled_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_scheduled_run_duration_seconds",
            "Scheduled run duration",
        ))
        .unwrap();
        let forge_calls = IntCounterVec::new(
            Opts::new(
                "agent_forge_calls_total",
                "Forge API calls, by backend, op and outcome",
            ),
            &["backend", "op", "outcome"],
        )
        .unwrap();
        let forge_seconds = HistogramVec::new(
            HistogramOpts::new("agent_forge_duration_seconds", "Forge API latency"),
            &["backend", "op"],
        )
        .unwrap();
        let hook_dispatches = IntCounterVec::new(
            Opts::new(
                "agent_hook_dispatches_total",
                "Lifecycle hook dispatches, by hook and attachment point",
            ),
            &["hook", "point"],
        )
        .unwrap();
        let route_decisions = IntCounterVec::new(
            Opts::new(
                "agent_route_decisions_total",
                "Router decisions, by target provider and outcome",
            ),
            &["target", "decision"],
        )
        .unwrap();
        let pool_members_alive = IntGaugeVec::new(
            Opts::new("agent_pool_members_alive", "Live LLM pool members, by tier"),
            &["tier"],
        )
        .unwrap();
        let pool_probe_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_pool_probe_duration_seconds",
                "LLM pool probe latency",
            ),
            &["member", "outcome"],
        )
        .unwrap();
        let pool_dispatch_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_pool_dispatch_duration_seconds",
                "LLM pool dispatch latency, by mode",
            ),
            &["mode"],
        )
        .unwrap();
        let pool_member_calls = IntCounterVec::new(
            Opts::new(
                "agent_pool_member_calls_total",
                "LLM pool member calls, by member and outcome",
            ),
            &["member", "outcome"],
        )
        .unwrap();
        let review_collect_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_review_collect_duration_seconds",
            "Whole review fact-collection fan-out wall-clock",
        ))
        .unwrap();
        let review_collector_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_review_collector_duration_seconds",
                "Per-collector review fact-collection latency",
            ),
            &["collector", "status"],
        )
        .unwrap();
        let review_collectors = IntCounterVec::new(
            Opts::new(
                "agent_review_collectors_total",
                "Review fact collectors, by collector and status",
            ),
            &["collector", "status"],
        )
        .unwrap();
        let review_change_files = Histogram::with_opts(HistogramOpts::new(
            "agent_review_change_files",
            "Changed-file count in a review's change set",
        ))
        .unwrap();
        let review_gitstate = IntCounterVec::new(
            Opts::new(
                "agent_review_gitstate_total",
                "Review git-state facts, by relationship, host and project",
            ),
            &["relationship", "host", "project"],
        )
        .unwrap();
        let review_findings = IntCounterVec::new(
            Opts::new(
                "agent_review_findings_total",
                "Static-analysis findings, by tool, severity and whether in the change",
            ),
            &["tool", "severity", "in_change"],
        )
        .unwrap();
        let review_signatures = IntCounterVec::new(
            Opts::new(
                "agent_review_signature_changes_total",
                "Changed function signatures, by language and kind (added/removed/modified)",
            ),
            &["lang", "kind"],
        )
        .unwrap();
        let review_callgraph_nodes = Histogram::with_opts(HistogramOpts::new(
            "agent_review_callgraph_nodes",
            "Node count of a review's call graph",
        ))
        .unwrap();
        let review_callgraph_edges = Histogram::with_opts(HistogramOpts::new(
            "agent_review_callgraph_edges",
            "Edge count of a review's call graph",
        ))
        .unwrap();
        let review_style_conformance = IntCounterVec::new(
            Opts::new(
                "agent_review_style_diff_conformance_total",
                "Whether a change matched the repo's own style, by outcome",
            ),
            &["matches"],
        )
        .unwrap();
        let review_summaries = IntCounterVec::new(
            Opts::new(
                "agent_review_summaries_total",
                "Cheap-LLM function summaries, by outcome (produced/failed/omitted)",
            ),
            &["outcome"],
        )
        .unwrap();
        let review_cochange = IntCounterVec::new(
            Opts::new(
                "agent_review_cochange_total",
                "Co-change signal: surfaced entries and partners absent from the diff",
            ),
            &["kind"],
        )
        .unwrap();
        let review_churn = IntCounterVec::new(
            Opts::new(
                "agent_review_churn_total",
                "Churn/ownership signal: files with an entry and single-owner (bus≤1) files",
            ),
            &["kind"],
        )
        .unwrap();
        let review_salience = IntCounterVec::new(
            Opts::new(
                "agent_review_salience_total",
                "Salience verdicts: files with a verdict and load-bearing (critical/foundational) files",
            ),
            &["kind"],
        )
        .unwrap();
        let review_runs = IntCounterVec::new(
            Opts::new(
                "agent_review_runs_total",
                "Completed review runs, by project, trigger mode and outcome",
            ),
            &["project", "mode_via", "outcome"],
        )
        .unwrap();
        let review_total_duration = HistogramVec::new(
            HistogramOpts::new(
                "agent_review_total_duration_seconds",
                "Whole review fan-out wall-clock, by project",
            ),
            &["project"],
        )
        .unwrap();
        let review_parallelism = Histogram::with_opts(HistogramOpts::new(
            "agent_review_parallelism_ratio",
            "Review parallelism payoff (sum of collector work ÷ total wall-clock)",
        ))
        .unwrap();
        let web_searches = IntCounterVec::new(
            Opts::new(
                "agent_web_searches_total",
                "Web searches, by backend and outcome",
            ),
            &["backend", "outcome"],
        )
        .unwrap();
        let web_search_seconds = HistogramVec::new(
            HistogramOpts::new("agent_web_search_duration_seconds", "Web search latency"),
            &["backend"],
        )
        .unwrap();
        let web_search_results = IntCounterVec::new(
            Opts::new(
                "agent_web_search_results_total",
                "Web search results returned",
            ),
            &["backend"],
        )
        .unwrap();
        let cache_breakpoints = IntCounterVec::new(
            Opts::new(
                "agent_cache_breakpoints_total",
                "Prompt-cache anchors placed, by placement strategy",
            ),
            &["strategy"],
        )
        .unwrap();
        let scanner_findings = IntCounterVec::new(
            Opts::new(
                "agent_scanner_findings_total",
                "Security findings, by severity, rule and scanned content kind",
            ),
            &["severity", "rule", "kind"],
        )
        .unwrap();
        let scan_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_scan_duration_seconds",
            "Content scan latency",
        ))
        .unwrap();
        let content_blocks = IntCounterVec::new(
            Opts::new(
                "agent_content_blocks_total",
                "Message content blocks sent to the model, by modality",
            ),
            &["modality"],
        )
        .unwrap();
        let content_blocks_dropped = IntCounter::new(
            "agent_content_blocks_dropped_total",
            "Media blocks dropped because the selected model has no vision support",
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

        // --- web (recorded by the web metrics wrapper) ------------------------
        let web_fetch_total = IntCounterVec::new(
            Opts::new(
                "agent_web_fetch_total",
                "web_fetch calls by outcome (ok/error)",
            ),
            &["outcome"],
        )
        .unwrap();
        let web_fetch_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_web_fetch_seconds",
            "web_fetch latency (measured at the seam boundary)",
        ))
        .unwrap();
        let web_fetch_bytes = Histogram::with_opts(HistogramOpts::new(
            "agent_web_fetch_bytes",
            "web_fetch decoded body size in bytes",
        ))
        .unwrap();

        // --- tasks (recorded by the tasks metrics wrapper) --------------------
        let tasks_open = IntGauge::new(
            "agent_tasks_open",
            "Open todos in the current plan (pending + in_progress)",
        )
        .unwrap();
        let tasks_closed = IntGauge::new(
            "agent_tasks_closed",
            "Closed todos in the current plan (completed + cancelled)",
        )
        .unwrap();

        // --- structured output (recorded by the structured helper) ------------
        let structured_total = IntCounterVec::new(
            Opts::new(
                "agent_structured_total",
                "Structured completions by outcome (pass/repaired/exhausted)",
            ),
            &["outcome"],
        )
        .unwrap();
        let structured_validate_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_structured_validate_seconds",
            "OutputSchema validation latency",
        ))
        .unwrap();

        // --- lsp (recorded by the lsp metrics wrapper) ------------------------
        let lsp_request_seconds = HistogramVec::new(
            HistogramOpts::new("agent_lsp_request_seconds", "LSP request latency"),
            &["method"],
        )
        .unwrap();
        let lsp_errors = IntCounterVec::new(
            Opts::new("agent_lsp_errors_total", "LSP request errors"),
            &["method"],
        )
        .unwrap();
        let lsp_diagnostics = IntCounterVec::new(
            Opts::new(
                "agent_lsp_diagnostics_total",
                "Diagnostics observed, by severity",
            ),
            &["severity"],
        )
        .unwrap();

        // --- sandbox (recorded by the sandbox metrics wrapper) ----------------
        let sandbox_exec_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_sandbox_exec_seconds",
                "Sandboxed exec latency, by backend",
            ),
            &["backend"],
        )
        .unwrap();
        let sandbox_exec_total = IntCounterVec::new(
            Opts::new(
                "agent_sandbox_exec_total",
                "Sandboxed execs by backend + outcome (ok/error)",
            ),
            &["backend", "outcome"],
        )
        .unwrap();

        // --- embed (recorded by the embedder metrics wrapper) ----------------
        let embed_seconds = HistogramVec::new(
            HistogramOpts::new("agent_embed_seconds", "Embedding latency, by backend"),
            &["backend"],
        )
        .unwrap();
        let embed_batch = HistogramVec::new(
            HistogramOpts::new("agent_embed_batch", "Texts embedded per call, by backend"),
            &["backend"],
        )
        .unwrap();

        // --- session (recorded by the session metrics wrapper) ----------------
        let session_ops = IntCounterVec::new(
            Opts::new("agent_session_ops_total", "Session-history mutations by op"),
            &["op"],
        )
        .unwrap();
        let session_gc_reclaimed = IntCounter::new(
            "agent_session_gc_reclaimed_total",
            "Checkpoint objects reclaimed by prune",
        )
        .unwrap();

        // --- reference (recorded by the reference metrics wrapper) ------------
        let reference_resolve_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_reference_resolve_seconds",
            "`@`-reference expansion latency per prompt",
        ))
        .unwrap();
        let reference_refs = IntCounterVec::new(
            Opts::new(
                "agent_reference_refs_total",
                "References resolved by kind + outcome (block/warn)",
            ),
            &["kind", "outcome"],
        )
        .unwrap();
        let reference_blocked = IntCounter::new(
            "agent_reference_blocked_total",
            "Reference expansions dropped for exceeding the token budget",
        )
        .unwrap();

        let collectors: Vec<Box<dyn prometheus::core::Collector>> = vec![
            Box::new(api_calls.clone()),
            Box::new(api_call_seconds.clone()),
            Box::new(tokens.clone()),
            Box::new(cost_usd.clone()),
            Box::new(cache_tokens.clone()),
            Box::new(context_tokens.clone()),
            Box::new(context_messages.clone()),
            Box::new(tool_calls.clone()),
            Box::new(verifier_verdicts.clone()),
            Box::new(pty_active.clone()),
            Box::new(pty_bytes.clone()),
            Box::new(pty_sessions.clone()),
            Box::new(scheduled_runs.clone()),
            Box::new(scheduled_seconds.clone()),
            Box::new(forge_calls.clone()),
            Box::new(forge_seconds.clone()),
            Box::new(hook_dispatches.clone()),
            Box::new(route_decisions.clone()),
            Box::new(pool_members_alive.clone()),
            Box::new(pool_probe_seconds.clone()),
            Box::new(pool_dispatch_seconds.clone()),
            Box::new(pool_member_calls.clone()),
            Box::new(review_collect_seconds.clone()),
            Box::new(review_collector_seconds.clone()),
            Box::new(review_collectors.clone()),
            Box::new(review_change_files.clone()),
            Box::new(review_gitstate.clone()),
            Box::new(review_findings.clone()),
            Box::new(review_signatures.clone()),
            Box::new(review_callgraph_nodes.clone()),
            Box::new(review_callgraph_edges.clone()),
            Box::new(review_style_conformance.clone()),
            Box::new(review_summaries.clone()),
            Box::new(review_cochange.clone()),
            Box::new(review_churn.clone()),
            Box::new(review_salience.clone()),
            Box::new(review_runs.clone()),
            Box::new(review_total_duration.clone()),
            Box::new(review_parallelism.clone()),
            Box::new(web_searches.clone()),
            Box::new(web_search_seconds.clone()),
            Box::new(web_search_results.clone()),
            Box::new(cache_breakpoints.clone()),
            Box::new(scanner_findings.clone()),
            Box::new(scan_seconds.clone()),
            Box::new(content_blocks.clone()),
            Box::new(content_blocks_dropped.clone()),
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
            Box::new(web_fetch_total.clone()),
            Box::new(web_fetch_seconds.clone()),
            Box::new(web_fetch_bytes.clone()),
            Box::new(tasks_open.clone()),
            Box::new(tasks_closed.clone()),
            Box::new(structured_total.clone()),
            Box::new(structured_validate_seconds.clone()),
            Box::new(lsp_request_seconds.clone()),
            Box::new(lsp_errors.clone()),
            Box::new(lsp_diagnostics.clone()),
            Box::new(sandbox_exec_seconds.clone()),
            Box::new(sandbox_exec_total.clone()),
            Box::new(embed_seconds.clone()),
            Box::new(embed_batch.clone()),
            Box::new(session_ops.clone()),
            Box::new(session_gc_reclaimed.clone()),
            Box::new(reference_resolve_seconds.clone()),
            Box::new(reference_refs.clone()),
            Box::new(reference_blocked.clone()),
        ];
        for m in collectors {
            registry.register(m).expect("register metric");
        }

        Self {
            registry: Arc::new(registry),
            api_calls,
            api_call_seconds,
            tokens,
            cost_usd,
            cache_tokens,
            context_tokens,
            context_messages,
            tool_calls,
            verifier_verdicts,
            pty_active,
            pty_bytes,
            pty_sessions,
            scheduled_runs,
            scheduled_seconds,
            forge_calls,
            forge_seconds,
            hook_dispatches,
            route_decisions,
            pool_members_alive,
            pool_probe_seconds,
            pool_dispatch_seconds,
            pool_member_calls,
            review_collect_seconds,
            review_collector_seconds,
            review_collectors,
            review_change_files,
            review_gitstate,
            review_findings,
            review_signatures,
            review_callgraph_nodes,
            review_callgraph_edges,
            review_style_conformance,
            review_summaries,
            review_cochange,
            review_churn,
            review_salience,
            review_runs,
            review_total_duration,
            review_parallelism,
            web_searches,
            web_search_seconds,
            web_search_results,
            cache_breakpoints,
            scanner_findings,
            scan_seconds,
            content_blocks,
            content_blocks_dropped,
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
            web_fetch_total,
            web_fetch_seconds,
            web_fetch_bytes,
            tasks_open,
            tasks_closed,
            structured_total,
            structured_validate_seconds,
            lsp_request_seconds,
            lsp_errors,
            lsp_diagnostics,
            sandbox_exec_seconds,
            sandbox_exec_total,
            embed_seconds,
            embed_batch,
            session_ops,
            session_gc_reclaimed,
            reference_resolve_seconds,
            reference_refs,
            reference_blocked,
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
    /// Count one content block about to be sent, by modality (parity spec 26).
    /// Takes the label rather than a `Message` so this stays a leaf crate.
    pub fn on_content_block(&self, modality: &str) {
        self.content_blocks.with_label_values(&[modality]).inc();
    }
    /// Media blocks stripped because the model cannot accept them.
    pub fn on_content_blocks_dropped(&self, n: u64) {
        self.content_blocks_dropped.inc_by(n);
    }
    /// One security finding (parity spec 18).
    pub fn on_scanner_finding(&self, severity: &str, rule: &str, kind: &str) {
        self.scanner_findings
            .with_label_values(&[severity, rule, kind])
            .inc();
    }
    /// Latency of one content scan.
    pub fn on_scan(&self, seconds: f64) {
        self.scan_seconds.observe(seconds);
    }
    /// A pty session opened (parity spec 29).
    pub fn on_pty_open(&self) {
        self.pty_active.inc();
    }
    /// A pty session ended.
    pub fn on_pty_close(&self, outcome: &str) {
        self.pty_active.dec();
        self.pty_sessions.with_label_values(&[outcome]).inc();
    }
    /// Bytes through a pty, by direction (`in`/`out`).
    pub fn on_pty_bytes(&self, direction: &str, n: u64) {
        self.pty_bytes.with_label_values(&[direction]).inc_by(n);
    }
    /// One scheduled run (parity spec 28).
    pub fn on_scheduled_run(&self, outcome: &str, seconds: f64) {
        self.scheduled_runs.with_label_values(&[outcome]).inc();
        self.scheduled_seconds.observe(seconds);
    }
    /// One forge API call (parity spec 27).
    pub fn on_forge_call(&self, backend: &str, op: &str, outcome: &str, seconds: f64) {
        self.forge_calls
            .with_label_values(&[backend, op, outcome])
            .inc();
        self.forge_seconds
            .with_label_values(&[backend, op])
            .observe(seconds);
    }
    /// One lifecycle hook dispatch (parity spec 22).
    pub fn on_hook(&self, hook: &str, point: &str) {
        self.hook_dispatches.with_label_values(&[hook, point]).inc();
    }
    /// One router decision: `routed` / `fellover` / `skipped_unhealthy` /
    /// `exhausted`, by target (parity spec 25).
    pub fn on_route_decision(&self, target: &str, decision: &str) {
        self.route_decisions
            .with_label_values(&[target, decision])
            .inc();
    }
    /// LLM pool: set the live-member gauge for a tier.
    pub fn set_pool_members_alive(&self, tier: &str, n: i64) {
        self.pool_members_alive.with_label_values(&[tier]).set(n);
    }
    /// LLM pool: one member probe (outcome `live`/`dead`) and its latency.
    pub fn on_pool_probe(&self, member: &str, outcome: &str, seconds: f64) {
        self.pool_probe_seconds
            .with_label_values(&[member, outcome])
            .observe(seconds);
    }
    /// LLM pool: one dispatch (`one`/`all`) and its wall-clock.
    pub fn on_pool_dispatch(&self, mode: &str, seconds: f64) {
        self.pool_dispatch_seconds
            .with_label_values(&[mode])
            .observe(seconds);
    }
    /// LLM pool: one member call outcome (`ok`/`error`).
    pub fn on_pool_member_call(&self, member: &str, outcome: &str) {
        self.pool_member_calls
            .with_label_values(&[member, outcome])
            .inc();
    }
    /// Review: the whole fact-collection fan-out wall-clock.
    pub fn on_review_collect(&self, seconds: f64) {
        self.review_collect_seconds.observe(seconds);
    }
    /// Review: one collector's status + latency (`collector`, `status`).
    pub fn on_review_collector(&self, collector: &str, status: &str, seconds: f64) {
        self.review_collectors
            .with_label_values(&[collector, status])
            .inc();
        self.review_collector_seconds
            .with_label_values(&[collector, status])
            .observe(seconds);
    }
    /// Review: the changed-file count of one change set.
    pub fn on_review_change_files(&self, n: u64) {
        self.review_change_files.observe(n as f64);
    }
    /// Review: one git-state fact triple.
    pub fn on_review_gitstate(&self, relationship: &str, host: &str, project: &str) {
        self.review_gitstate
            .with_label_values(&[relationship, host, project])
            .inc();
    }
    /// Review: a bucket of static-analysis findings. `count` is a trusted internal
    /// aggregate (never a hostile number), so `inc_by` is safe here.
    pub fn on_review_findings(&self, tool: &str, severity: &str, in_change: bool, count: u64) {
        let ic = if in_change { "true" } else { "false" };
        self.review_findings
            .with_label_values(&[tool, severity, ic])
            .inc_by(count);
    }
    /// Review: a bucket of changed function signatures. `count` is a trusted
    /// internal aggregate, so `inc_by` is safe.
    pub fn on_review_signatures(&self, lang: &str, kind: &str, count: u64) {
        self.review_signatures
            .with_label_values(&[lang, kind])
            .inc_by(count);
    }
    /// Review: the size of one call graph (node + edge counts).
    pub fn on_review_callgraph(&self, nodes: f64, edges: f64) {
        self.review_callgraph_nodes.observe(nodes);
        self.review_callgraph_edges.observe(edges);
    }
    /// Review: whether a change conformed to the repo's own style.
    pub fn on_review_style(&self, matches: bool) {
        let v = if matches { "true" } else { "false" };
        self.review_style_conformance.with_label_values(&[v]).inc();
    }
    /// Review: function-summary outcomes. Counts are trusted internal aggregates.
    pub fn on_review_summaries(&self, produced: u64, failed: u64, omitted: u64) {
        self.review_summaries
            .with_label_values(&["produced"])
            .inc_by(produced);
        self.review_summaries
            .with_label_values(&["failed"])
            .inc_by(failed);
        self.review_summaries
            .with_label_values(&["omitted"])
            .inc_by(omitted);
    }
    /// Review: co-change signal — entries surfaced and usual partners absent.
    pub fn on_review_cochange(&self, entries: u64, missing: u64) {
        self.review_cochange
            .with_label_values(&["entries"])
            .inc_by(entries);
        self.review_cochange
            .with_label_values(&["missing_partners"])
            .inc_by(missing);
    }
    /// Review: churn/ownership signal — files with an entry and single-owner files.
    pub fn on_review_churn(&self, files: u64, single_owner: u64) {
        self.review_churn
            .with_label_values(&["files"])
            .inc_by(files);
        self.review_churn
            .with_label_values(&["single_owner"])
            .inc_by(single_owner);
    }
    /// Review: salience verdicts — files with a verdict and load-bearing files.
    pub fn on_review_salience(&self, files: u64, critical: u64) {
        self.review_salience
            .with_label_values(&["files"])
            .inc_by(files);
        self.review_salience
            .with_label_values(&["critical"])
            .inc_by(critical);
    }
    /// Review: one completed run — its count (by project/mode/outcome) + wall-clock.
    pub fn on_review_run(&self, project: &str, mode_via: &str, outcome: &str, seconds: f64) {
        self.review_runs
            .with_label_values(&[project, mode_via, outcome])
            .inc();
        self.review_total_duration
            .with_label_values(&[project])
            .observe(seconds.max(0.0));
    }
    /// Review: the parallelism payoff (Σ collector work ÷ total wall-clock).
    pub fn on_review_parallelism(&self, ratio: f64) {
        if ratio.is_finite() && ratio >= 0.0 {
            self.review_parallelism.observe(ratio);
        }
    }
    /// One web search: outcome, latency, and result count (parity spec 12).
    pub fn on_web_search(&self, backend: &str, outcome: &str, seconds: f64, results: u64) {
        self.web_searches
            .with_label_values(&[backend, outcome])
            .inc();
        self.web_search_seconds
            .with_label_values(&[backend])
            .observe(seconds);
        self.web_search_results
            .with_label_values(&[backend])
            .inc_by(results);
    }
    /// Prompt-cache anchors placed on one request (parity spec 24).
    pub fn on_cache_breakpoints(&self, strategy: &str, n: u64) {
        self.cache_breakpoints
            .with_label_values(&[strategy])
            .inc_by(n);
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
    /// Record a turn's USD cost, one line per billed `kind`
    /// (`input`/`output`/`cache_read`/`cache_write`). Zero lines are still
    /// recorded (a `0` increment is a no-op) so the series exists for a model.
    pub fn add_cost(
        &self,
        model: &str,
        input: f64,
        output: f64,
        cache_read: f64,
        cache_write: f64,
    ) {
        for (kind, usd) in [
            ("input", input),
            ("output", output),
            ("cache_read", cache_read),
            ("cache_write", cache_write),
        ] {
            // Only record a finite, positive amount: `inc_by` panics on a negative
            // value, and a non-finite (NaN/inf) — from a malformed/hostile price row
            // — would poison the counter. Both are dropped defensively.
            if usd.is_finite() && usd > 0.0 {
                self.cost_usd.with_label_values(&[model, kind]).inc_by(usd);
            }
        }
    }

    /// Record prompt-cache token counts for a turn: `read` = tokens served from the
    /// cache (a hit), `write` = tokens written into it. The cache-hit ratio is
    /// derived downstream as `cache_read / (cache_read + input)`.
    pub fn add_cache_tokens(&self, model: &str, read: u64, write: u64) {
        if read > 0 {
            self.cache_tokens
                .with_label_values(&[model, "read"])
                .inc_by(read);
        }
        if write > 0 {
            self.cache_tokens
                .with_label_values(&[model, "write"])
                .inc_by(write);
        }
    }

    pub fn set_context(&self, prompt_tokens: i64, messages: i64) {
        self.context_tokens.set(prompt_tokens);
        self.context_messages.set(messages);
    }
    pub fn on_tool(&self, tool: &str, status: &str) {
        self.tool_calls.with_label_values(&[tool, status]).inc();
    }
    /// One tool-call verifier verdict. `verdict` is `allow|revise|deny`; `mode` is
    /// `shadow|enforce`. Labels are bounded — callers pass built-in verifier names.
    pub fn on_verifier(&self, verifier: &str, verdict: &str, mode: &str) {
        self.verifier_verdicts
            .with_label_values(&[verifier, verdict, mode])
            .inc();
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

    // --- web (web_fetch seam) instrumentation -----------------------------

    /// Record a completed `web_fetch`: outcome (`ok`/`error`), latency, and the
    /// decoded body size. Not labelled by host (untrusted URL → cardinality DoS);
    /// the host is a `web.fetch` span attribute instead.
    pub fn on_web_fetch(&self, outcome: &str, seconds: f64, bytes: u64) {
        self.web_fetch_total.with_label_values(&[outcome]).inc();
        self.web_fetch_seconds.observe(seconds);
        self.web_fetch_bytes.observe(bytes as f64);
    }

    // --- tasks (TaskTracker seam) instrumentation -------------------------

    /// Set the plan-progress gauges to the current open / closed todo counts.
    /// Called by the tasks metrics wrapper after every write/update/clear.
    pub fn set_tasks_progress(&self, open: i64, closed: i64) {
        self.tasks_open.set(open);
        self.tasks_closed.set(closed);
    }

    // --- structured output (OutputSchema seam) instrumentation ------------

    /// Count a completed structured request by outcome (`pass`/`repaired`/`exhausted`).
    pub fn on_structured_outcome(&self, outcome: &str) {
        self.structured_total.with_label_values(&[outcome]).inc();
    }
    /// Record one schema-validation's latency (per attempt).
    pub fn on_structured_validate(&self, seconds: f64) {
        self.structured_validate_seconds.observe(seconds);
    }

    // --- lsp (LspBackend seam) instrumentation ----------------------------

    /// Record an LSP request's latency, labelled by method.
    pub fn on_lsp_request(&self, method: &str, seconds: f64) {
        self.lsp_request_seconds
            .with_label_values(&[method])
            .observe(seconds);
    }
    /// Count an LSP request error, labelled by method.
    pub fn on_lsp_error(&self, method: &str) {
        self.lsp_errors.with_label_values(&[method]).inc();
    }
    /// Count one observed diagnostic, labelled by severity.
    pub fn on_lsp_diagnostic(&self, severity: &str) {
        self.lsp_diagnostics.with_label_values(&[severity]).inc();
    }

    // --- sandbox (Sandbox seam) instrumentation ---------------------------

    /// Record a sandboxed exec: latency + outcome (`ok`/`error`), by backend.
    pub fn on_sandbox_exec(&self, backend: &str, outcome: &str, seconds: f64) {
        self.sandbox_exec_seconds
            .with_label_values(&[backend])
            .observe(seconds);
        self.sandbox_exec_total
            .with_label_values(&[backend, outcome])
            .inc();
    }

    // --- embed (Embedder seam) instrumentation ----------------------------

    /// Record an embed call: latency + batch size, by backend.
    pub fn on_embed(&self, backend: &str, seconds: f64, batch: usize) {
        self.embed_seconds
            .with_label_values(&[backend])
            .observe(seconds);
        self.embed_batch
            .with_label_values(&[backend])
            .observe(batch as f64);
    }

    // --- session (SessionStore seam) instrumentation ----------------------

    /// Count a session-history mutation, labelled by op.
    pub fn on_session_op(&self, op: &str) {
        self.session_ops.with_label_values(&[op]).inc();
    }
    /// Count checkpoint objects reclaimed by a prune.
    pub fn on_session_gc(&self, reclaimed: usize) {
        self.session_gc_reclaimed.inc_by(reclaimed as u64);
    }

    // --- reference (ReferenceResolver seam) instrumentation ---------------

    /// Record a prompt's `@`-reference expansion: total latency.
    pub fn on_reference_resolve(&self, seconds: f64) {
        self.reference_resolve_seconds.observe(seconds);
    }
    /// Count one resolved reference by kind + outcome (`block`/`warn`).
    pub fn on_reference_ref(&self, kind: &str, outcome: &str) {
        self.reference_refs
            .with_label_values(&[kind, outcome])
            .inc();
    }
    /// Count an expansion dropped for exceeding the token budget.
    pub fn on_reference_blocked(&self) {
        self.reference_blocked.inc();
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
        m.add_cost("test-model", 0.003, 0.015, 0.0003, 0.0);
        m.add_cache_tokens("test-model", 80, 20);
        m.set_context(100, 4);
        m.on_tool("bash", "ok");
        m.on_verifier("schema", "allow", "shadow");
        m.run_finished("success", 1.5);

        let text = m.encode_text();
        for name in [
            "agent_iterations_total",
            "agent_api_calls_total",
            "agent_tokens_total",
            "agent_cost_usd_total",
            "agent_cache_tokens_total",
            "agent_context_tokens",
            "agent_tool_calls_total",
            "agent_verifier_verdicts_total",
            "agent_runs_total",
        ] {
            assert!(text.contains(name), "missing metric `{name}` in:\n{text}");
        }
        assert!(text.contains("test-model"));
    }

    #[test]
    fn on_verifier_records_verdict_by_verifier_mode() {
        let m = Metrics::new();
        m.on_verifier("schema", "revise", "enforce");
        m.on_verifier("schema", "allow", "shadow");
        let text = m.encode_text();
        assert!(text.contains("agent_verifier_verdicts_total"), "{text}");
        assert!(
            text.contains("verifier=\"schema\"")
                && text.contains("verdict=\"revise\"")
                && text.contains("mode=\"enforce\""),
            "labels missing: {text}"
        );
    }

    #[test]
    fn add_cost_drops_non_finite_and_non_positive_lines() {
        let m = Metrics::new();
        // Must not panic (inc_by panics on negatives; NaN/inf would poison the
        // counter). Only the one finite-positive line is recorded.
        m.add_cost("m", f64::NAN, -1.0, f64::INFINITY, 0.003);
        let text = m.encode_text();
        assert!(text.contains("kind=\"cache_write\""), "{text}");
        assert!(
            !text.contains("kind=\"input\""),
            "NaN input recorded: {text}"
        );
        assert!(
            !text.contains("kind=\"output\""),
            "negative output recorded"
        );
        assert!(
            !text.contains("kind=\"cache_read\""),
            "infinite cache_read recorded"
        );
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
