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
    upstream_tokens: IntCounterVec,
    // USD cost + cache-token accounting (recorded once a price table is applied,
    // see agent-tokenizer + parity spec 23). `cost_usd` is a float counter (money);
    // `cache_tokens` splits prompt-cache reads/writes so the cache-hit ratio
    // (cache_read / (cache_read + input)) is derivable in PromQL.
    cost_usd: CounterVec,
    cache_tokens: IntCounterVec,
    context_tokens: IntGaugeVec,
    context_messages: IntGaugeVec,
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
    router_decided: IntCounterVec,
    router_no_candidate: IntCounterVec,
    registry_upstreams: IntGaugeVec,
    registry_mutations: IntCounterVec,
    router_dispatch: IntCounterVec,
    router_failover: IntCounterVec,
    router_inflight: IntGaugeVec,
    gate_verdicts: IntCounterVec,
    gate_rounds: Histogram,
    gate_phase_seconds: HistogramVec,
    gate_issues: IntCounterVec,
    gate_alternatives: IntCounter,
    distill_jobs: IntCounterVec,
    distill_lag_seconds: HistogramVec,
    graph_branches: IntCounterVec,
    graph_join_wait_seconds: HistogramVec,
    graph_merges: IntCounterVec,
    // LLM pool (docs/design/code-review/llm-pool.md + gpu-pool/).
    pool_members_alive: IntGaugeVec,
    pool_probe_seconds: HistogramVec,
    pool_dispatch_seconds: HistogramVec,
    pool_member_calls: IntCounterVec,
    // Load balancing (docs/design/gpu-pool/01-load-balance.md).
    pool_member_inflight: IntGaugeVec,
    pool_member_latency: HistogramVec,
    pool_selects: IntCounterVec,
    // Capacity / backpressure (docs/design/gpu-pool/02-capacity.md).
    pool_member_saturated: IntGaugeVec,
    pool_saturation_shed: IntCounter,
    grpc_overload_shed: IntCounter,
    // Graded health (docs/design/gpu-pool/03-gpu-health.md).
    pool_member_state: IntGaugeVec,
    pool_member_latency_ewma: IntGaugeVec,
    // General task-mode detection (docs/design/adaptive-cognition/01-mode.md).
    mode_classifications: IntCounterVec,
    mode_switches: IntCounterVec,
    mode_switch_confidence: Histogram,
    // Situational system-prompt fragments (docs/design/prompts/).
    prompt_fragments_selected: IntCounterVec,
    // Dimensional memory (adaptive-cognition 03).
    dimension_summaries: IntCounterVec,
    dimension_summarize_seconds: Histogram,
    dimension_recalls: IntCounterVec,
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
    review_risk: IntCounterVec,
    review_risk_score: Histogram,
    review_runs: IntCounterVec,
    review_total_duration: HistogramVec,
    review_parallelism: Histogram,
    // Review-fleet lifecycle families — bounded `(user, repo)` tenancy, recorded via
    // [`FleetMetrics`] (`for_fleet`). `repo` is the operator-roster `FleetSession.repo`
    // (`O(sessions)`, `safe_segment`-valid); PR is a span attribute, never a label.
    // Distinct repos are backstopped by the `fleet_repos` LRU (docs/design/observability).
    fleet_triggers: IntCounterVec,
    fleet_reviews: IntCounterVec,
    fleet_progress: IntCounterVec,
    fleet_approvals: IntCounterVec,
    fleet_approval_latency: HistogramVec,
    fleet_post_failures: IntCounterVec,
    /// LRU cap on distinct fleet `(user, repo)` label pairs — the lifecycle backstop
    /// for the `repo` dimension. Shared across `Metrics` clones (Arc), so an eviction
    /// removes that pair's series from the one registry.
    fleet_repos: Arc<std::sync::Mutex<FleetRepoLru>>,
    // Message-transport health families (config C37 / D2, Phase 3). These are
    // seam-health + bounded: labelled only by `kind` (the transport impl's own
    // `&'static str`, e.g. slack|matrix) plus a bounded `outcome`/`decision`. NO
    // tenant/repo label — per-repo attribution is meaningless for a shared channel;
    // the fleet beat's repo rides the parent `fleet.progress` **span**, never a label.
    transport_posts: IntCounterVec,
    transport_post_seconds: HistogramVec,
    transport_ratelimit: IntCounterVec,
    // Config-plane observability families (docs/design/observability, Phase 4). The
    // config-store CRUD counter and the gRPC-server RPC counter carry a bounded,
    // LRU-capped `tenant` label (the verified org, C25); their companion latency
    // histograms stay label-less-of-tenant (op/rpc only) — seam-health, not
    // attribution. The auth/authz counters carry only bounded enums (outcome /
    // action×resource_type×decision), never tenant (it rides the `grpc.server` span).
    config_store_ops: IntCounterVec,
    config_store_op_seconds: HistogramVec,
    auth_verify: IntCounterVec,
    authz_decisions: IntCounterVec,
    grpc_server_rpc: IntCounterVec,
    grpc_server_rpc_seconds: HistogramVec,
    /// LRU cap on distinct config-plane `tenant` label values — the lifecycle backstop
    /// shared by `config_store_ops` and `grpc_server_rpc` (the two tenant-labelled
    /// config-plane families). Shared across `Metrics` clones (Arc); an eviction removes
    /// exactly the evicted tenant's recorded series from the one registry.
    config_plane_tenants: Arc<std::sync::Mutex<TenantLru>>,
    /// High-water bound on distinct `grpc_server_rpc` `rpc` label values. The RPC path
    /// is `req.uri().path()` — attacker-controllable (a client can spray junk paths that
    /// still reach the tower layer before tonic routes them to `Unimplemented`), so left
    /// unbounded it is a cardinality DoS. The real method set is fixed and small (~50),
    /// so the first `MAX_RPCS` distinct paths register as themselves and any later
    /// *unknown* path collapses to the `"other"` sentinel — a high-water threshold, not
    /// an LRU (a real method must never be evicted, or its series would strand and its
    /// hits misroute). Shared across `Metrics` clones (Arc).
    rpc_labels: Arc<std::sync::Mutex<RpcBound>>,
    web_searches: IntCounterVec,
    web_search_seconds: HistogramVec,
    web_search_results: IntCounterVec,
    cache_breakpoints: IntCounterVec,
    scanner_findings: IntCounterVec,
    scan_seconds: Histogram,
    content_blocks_dropped: IntCounter,
    iterations: IntCounterVec,
    runs: IntCounterVec,
    run_seconds: HistogramVec,
    active: IntGaugeVec,

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
    // Mode-aware compaction (adaptive-cognition 02): switch reshapes, tokens shed
    // per trigger, and summary fallbacks.
    context_switch_compactions: IntCounterVec,
    context_tokens_shed: HistogramVec,
    context_summary_fallback: IntCounterVec,

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

    // --- ast / code graph (recorded by the ast metrics wrapper) -----------
    // Labelled by `backend` (go/scip/grpc) + `verb` so each code-graph query
    // reads distinctly under the same interface.
    ast_query_seconds: HistogramVec,
    ast_result_nodes: HistogramVec,
    ast_errors: IntCounterVec,

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
        // The curated loop-level families carry a per-tenant `(session, user)` label
        // pair so a run's spend/activity is attributable per session
        // (docs/design/multi-session/06-observability.md). Recorded only via
        // `SessionMetrics`; the seam-health families below stay label-less. `user` is
        // functionally dependent on `session`, so the pair ≈ session count, not a
        // product — within the low-hundreds-sessions budget. Under the org-tier
        // convention (`user = <org>`, C25) the `user` label reads as **org**: still
        // session-coarse (an org spans many sessions, never the reverse), so the
        // cardinality budget is unchanged.
        let api_calls = IntCounterVec::new(
            Opts::new("agent_api_calls_total", "LLM completion calls"),
            &["model", "finish_reason", "session", "user"],
        )
        .unwrap();
        let api_call_seconds = HistogramVec::new(
            HistogramOpts::new("agent_api_call_duration_seconds", "LLM call latency"),
            &["model"],
        )
        .unwrap();
        let tokens = IntCounterVec::new(
            Opts::new("agent_tokens_total", "Tokens consumed"),
            &["model", "kind", "session", "user"],
        )
        .unwrap();
        // Per-upstream attribution (graph-arena R8 / cognition-graph 06): the
        // main loop's `agent_tokens_total` is labeled by the response's MODEL
        // id and only covers the main provider; internal role calls (gate
        // critic, distiller, judge slots) go through named metered providers —
        // this family records their usage under the config-selected upstream
        // NAME, so "what did the critic/local model cost" is answerable.
        let upstream_tokens = IntCounterVec::new(
            Opts::new(
                "agent_upstream_tokens_total",
                "Tokens consumed per named upstream provider",
            ),
            &["upstream", "kind"],
        )
        .unwrap();
        let cost_usd = CounterVec::new(
            Opts::new("agent_cost_usd_total", "Cumulative USD cost by billed line"),
            &["model", "kind", "session", "user"],
        )
        .unwrap();
        let cache_tokens = IntCounterVec::new(
            Opts::new(
                "agent_cache_tokens_total",
                "Prompt-cache tokens (read = hit, write = created)",
            ),
            &["model", "kind", "session", "user"],
        )
        .unwrap();
        let context_tokens = IntGaugeVec::new(
            Opts::new(
                "agent_context_tokens",
                "Prompt tokens of the last request (context size)",
            ),
            &["session", "user"],
        )
        .unwrap();
        let context_messages = IntGaugeVec::new(
            Opts::new(
                "agent_context_messages",
                "Messages in the working set of the last request",
            ),
            &["session", "user"],
        )
        .unwrap();
        let tool_calls = IntCounterVec::new(
            Opts::new("agent_tool_calls_total", "Tool invocations"),
            &["tool", "status", "session", "user"],
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
            &["hook", "point", "tenant"],
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
        let router_decided = IntCounterVec::new(
            Opts::new(
                "agent_router_decisions_total",
                "Task-router policy decisions, by role, task mode, chosen upstream and rule",
            ),
            &["role", "task_mode", "chosen", "rule"],
        )
        .unwrap();
        let router_no_candidate = IntCounterVec::new(
            Opts::new(
                "agent_router_no_candidate_total",
                "Task-router requests every upstream was filtered out for, by role",
            ),
            &["role"],
        )
        .unwrap();
        let registry_upstreams = IntGaugeVec::new(
            Opts::new(
                "agent_registry_upstreams",
                "Provider-registry fleet size, by enabled state (model-router 03)",
            ),
            &["enabled"],
        )
        .unwrap();
        let registry_mutations = IntCounterVec::new(
            Opts::new(
                "agent_registry_mutations_total",
                "Provider-registry control-plane mutations, by op (put|delete|enable|put_policy)",
            ),
            &["op"],
        )
        .unwrap();
        let router_dispatch = IntCounterVec::new(
            Opts::new(
                "agent_router_dispatch_total",
                "Task-router dispatch attempts, by role, upstream and outcome (ok|retryable|terminal)",
            ),
            &["role", "upstream", "outcome"],
        )
        .unwrap();
        let router_failover = IntCounterVec::new(
            Opts::new(
                "agent_router_failover_total",
                "Task-router failover hops, by from/to upstream and reason",
            ),
            &["from", "to", "reason"],
        )
        .unwrap();
        let router_inflight = IntGaugeVec::new(
            Opts::new(
                "agent_router_upstream_inflight",
                "Requests currently in flight per task-router upstream",
            ),
            &["upstream"],
        )
        .unwrap();
        let gate_verdicts = IntCounterVec::new(
            Opts::new(
                "agent_gate_verdicts_total",
                "Consensus-gate outcomes (pass|fixed|alternatives|exhausted|critic_error)",
            ),
            &["outcome"],
        )
        .unwrap();
        let gate_rounds = Histogram::with_opts(
            HistogramOpts::new(
                "agent_gate_rounds",
                "Critic rounds per gated completion (ceiling 5)",
            )
            .buckets(vec![1.0, 2.0, 3.0, 4.0, 5.0]),
        )
        .unwrap();
        let gate_phase_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_gate_phase_duration_seconds",
                "Wall time per gate phase (generate|critique), per gated completion",
            ),
            &["phase"],
        )
        .unwrap();
        let gate_issues = IntCounterVec::new(
            Opts::new(
                "agent_gate_issues_total",
                "Critic issues by fate (raised|resolved|outstanding|dropped_no_evidence)",
            ),
            &["result"],
        )
        .unwrap();
        let gate_alternatives = IntCounter::new(
            "agent_gate_alternatives_total",
            "Alternatives recorded by the consensus gate (the roads not taken)",
        )
        .unwrap();
        let distill_jobs = IntCounterVec::new(
            Opts::new(
                "agent_distill_jobs_total",
                "Background distiller jobs by kind and outcome (succeeded_no_output \
                 is a success; dropped means the queue was full)",
            ),
            &["kind", "outcome"],
        )
        .unwrap();
        let distill_lag_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_distill_lag_seconds",
                "Delivery → digest-row-durable lag, per kind",
            ),
            &["kind"],
        )
        .unwrap();
        let graph_branches = IntCounterVec::new(
            Opts::new(
                "agent_graph_branches_total",
                "Cognition-graph fork branches by split node and fate (won/merged/\
                 lost/cancelled/timeout/error) — the split's cost multiplier, visible",
            ),
            &["split", "fate"],
        )
        .unwrap();
        let graph_join_wait_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_graph_join_wait_seconds",
                "Fork join wait (split → policy satisfied), per activation policy",
            ),
            &["policy"],
        )
        .unwrap();
        let graph_merges = IntCounterVec::new(
            Opts::new(
                "agent_graph_merge_total",
                "Fork merges by strategy and outcome (picked/synthesized/concat/\
                 single_survivor/tie_order/judge_error/degraded_compare/fallback_single)",
            ),
            &["strategy", "outcome"],
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
        let pool_member_inflight = IntGaugeVec::new(
            Opts::new(
                "agent_pool_member_inflight",
                "In-flight requests per LLM pool member (the load-balancing signal)",
            ),
            &["member"],
        )
        .unwrap();
        let pool_member_latency = HistogramVec::new(
            HistogramOpts::new(
                "agent_pool_member_latency_seconds",
                "Per-member LLM pool request latency",
            ),
            &["member"],
        )
        .unwrap();
        let pool_selects = IntCounterVec::new(
            Opts::new(
                "agent_pool_select_total",
                "LLM pool selection dispatches, by policy",
            ),
            &["policy"],
        )
        .unwrap();
        let pool_member_saturated = IntGaugeVec::new(
            Opts::new(
                "agent_pool_member_saturated",
                "Whether an LLM pool member is at its concurrency cap (1) or not (0)",
            ),
            &["member"],
        )
        .unwrap();
        let pool_saturation_shed = IntCounter::new(
            "agent_pool_saturation_shed_total",
            "Dispatches shed because every eligible pool member was saturated",
        )
        .unwrap();
        let grpc_overload_shed = IntCounter::new(
            "agent_grpc_overload_shed_total",
            "gRPC requests shed under overload by the admission layer (RESOURCE_EXHAUSTED)",
        )
        .unwrap();
        let pool_member_state = IntGaugeVec::new(
            Opts::new(
                "agent_pool_member_state",
                "Graded state of an LLM pool member (1 = in this state, 0 = not)",
            ),
            &["member", "state"],
        )
        .unwrap();
        let pool_member_latency_ewma = IntGaugeVec::new(
            Opts::new(
                "agent_pool_member_latency_ewma_ms",
                "Smoothed (EWMA) request latency per LLM pool member, milliseconds",
            ),
            &["member"],
        )
        .unwrap();
        let mode_classifications = IntCounterVec::new(
            Opts::new(
                "agent_mode_classifications_total",
                "Task-mode classifications, by detected mode and stage",
            ),
            &["mode", "via"],
        )
        .unwrap();
        let mode_switches = IntCounterVec::new(
            Opts::new(
                "agent_mode_switches_total",
                "Task-mode switches, by from/to mode",
            ),
            &["from", "to", "session", "user"],
        )
        .unwrap();
        let mode_switch_confidence = Histogram::with_opts(HistogramOpts::new(
            "agent_mode_switch_confidence",
            "Confidence of a decided task-mode switch",
        ))
        .unwrap();
        let prompt_fragments_selected = IntCounterVec::new(
            Opts::new(
                "agent_prompt_fragments_selected_total",
                "Situational system-prompt fragment updates, by mode and action",
            ),
            &["mode", "action"],
        )
        .unwrap();
        let dimension_summaries = IntCounterVec::new(
            Opts::new(
                "agent_dimension_summaries_total",
                "Per-step dimension summaries filed, by dimension and novelty",
            ),
            &["dimension", "is_new"],
        )
        .unwrap();
        let dimension_summarize_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_dimension_summarize_duration_seconds",
            "Per-step dimensional summarize-pass wall-clock",
        ))
        .unwrap();
        let dimension_recalls = IntCounterVec::new(
            Opts::new(
                "agent_dimension_recall_total",
                "Dimension-weighted recalls, by dimension",
            ),
            &["dimension"],
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
        let review_risk = IntCounterVec::new(
            Opts::new(
                "agent_review_risk_total",
                "Risk synthesis: at-risk files and gate-failing runs",
            ),
            &["kind"],
        )
        .unwrap();
        let review_risk_score = Histogram::with_opts(HistogramOpts::new(
            "agent_review_risk_max_score",
            "The highest per-file risk score in a review run (0..1)",
        ))
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
        let fleet_triggers = IntCounterVec::new(
            Opts::new(
                "agent_fleet_triggers_total",
                "Review-fleet triggers, by source (poll|slack), tenant and repo",
            ),
            &["source", "user", "repo"],
        )
        .unwrap();
        let fleet_reviews = IntCounterVec::new(
            Opts::new(
                "agent_fleet_reviews_total",
                "Review-fleet review lifecycle transitions, by status, tenant and repo",
            ),
            &["status", "user", "repo"],
        )
        .unwrap();
        let fleet_progress = IntCounterVec::new(
            Opts::new(
                "agent_fleet_progress_total",
                "Review-fleet progress-feed beats, by beat, outcome, tenant and repo",
            ),
            &["beat", "outcome", "user", "repo"],
        )
        .unwrap();
        let fleet_approvals = IntCounterVec::new(
            Opts::new(
                "agent_fleet_approvals_total",
                "Review-fleet approval outcomes, by outcome, tenant and repo",
            ),
            &["outcome", "user", "repo"],
        )
        .unwrap();
        let fleet_approval_latency = HistogramVec::new(
            HistogramOpts::new(
                "agent_fleet_approval_latency_seconds",
                "Review-fleet drafted→posted approval latency, by tenant and repo",
            ),
            &["user", "repo"],
        )
        .unwrap();
        let fleet_post_failures = IntCounterVec::new(
            Opts::new(
                "agent_fleet_post_failures_total",
                "Review-fleet progress/approval post failures, by transport, tenant and repo",
            ),
            &["transport", "user", "repo"],
        )
        .unwrap();
        let fleet_repos = Arc::new(std::sync::Mutex::new(FleetRepoLru::new(MAX_FLEET_REPOS)));
        let transport_posts = IntCounterVec::new(
            Opts::new(
                "agent_transport_posts_total",
                "Message-transport post attempts, by kind and outcome (ok|ratelimited|error)",
            ),
            &["kind", "outcome"],
        )
        .unwrap();
        let transport_post_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_transport_post_seconds",
                "Message-transport post latency in seconds, by kind (health)",
            ),
            &["kind"],
        )
        .unwrap();
        let transport_ratelimit = IntCounterVec::new(
            Opts::new(
                "agent_transport_ratelimit_total",
                "Message-transport rate-limit decisions, by kind and decision (admit|refuse)",
            ),
            &["kind", "decision"],
        )
        .unwrap();
        let config_store_ops = IntCounterVec::new(
            Opts::new(
                "agent_config_store_ops_total",
                "Config-store operations, by collection, op (get|list|count|tenants|put|delete|ensure_tenant), outcome (ok|error) and tenant",
            ),
            &["collection", "op", "outcome", "tenant"],
        )
        .unwrap();
        let config_store_op_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_config_store_op_seconds",
                "Config-store call latency in seconds, by op (get|list|count|tenants|apply) — seam-health, un-tenanted",
            ),
            &["op"],
        )
        .unwrap();
        let auth_verify = IntCounterVec::new(
            Opts::new(
                "agent_auth_verify_total",
                "Bearer-token verification attempts at the auth layer, by outcome (ok|error)",
            ),
            &["outcome"],
        )
        .unwrap();
        let authz_decisions = IntCounterVec::new(
            Opts::new(
                "agent_authz_decisions_total",
                "RBAC authorization decisions, by action, resource_type and decision (allow|deny)",
            ),
            &["action", "resource_type", "decision"],
        )
        .unwrap();
        let grpc_server_rpc = IntCounterVec::new(
            Opts::new(
                "agent_grpc_server_rpc_total",
                "gRPC server requests, by rpc path, outcome (ok|the grpc code name) and tenant",
            ),
            &["rpc", "outcome", "tenant"],
        )
        .unwrap();
        let grpc_server_rpc_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_grpc_server_rpc_seconds",
                "gRPC server request latency in seconds, by rpc path — seam-health, un-tenanted",
            ),
            &["rpc"],
        )
        .unwrap();
        let config_plane_tenants = Arc::new(std::sync::Mutex::new(TenantLru::new(MAX_TENANTS)));
        let rpc_labels = Arc::new(std::sync::Mutex::new(RpcBound::new(MAX_RPCS)));
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
        let iterations = IntCounterVec::new(
            Opts::new("agent_iterations_total", "Agent loop iterations"),
            &["session", "user"],
        )
        .unwrap();
        let runs = IntCounterVec::new(
            Opts::new("agent_runs_total", "Completed agent runs"),
            &["outcome", "session", "user"],
        )
        .unwrap();
        let run_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_run_duration_seconds",
                "Wall-clock duration of an agent run",
            ),
            &["session", "user"],
        )
        .unwrap();
        let active = IntGaugeVec::new(
            Opts::new("agent_active", "1 while a run is in progress"),
            &["session", "user"],
        )
        .unwrap();

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
        let context_switch_compactions = IntCounterVec::new(
            Opts::new(
                "agent_context_switch_compactions_total",
                "Mode-switch context reshapes run",
            ),
            &["from", "to"],
        )
        .unwrap();
        let context_tokens_shed = HistogramVec::new(
            HistogramOpts::new("agent_context_tokens_shed", "Tokens shed by a compaction")
                .buckets(prometheus::exponential_buckets(50.0, 2.0, 12).unwrap()),
            &["trigger"],
        )
        .unwrap();
        let context_summary_fallback = IntCounterVec::new(
            Opts::new(
                "agent_context_summary_fallback_total",
                "Switch-compaction summary fallbacks (generic|drop)",
            ),
            &["kind"],
        )
        .unwrap();

        // --- policy -----------------------------------------------------------
        let policy_authorize = IntCounterVec::new(
            Opts::new("agent_policy_authorize_total", "Policy authorize decisions"),
            &["policy", "decision", "tenant"],
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
            &["category", "action", "tenant"],
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

        // --- ast / code graph -------------------------------------------------
        let ast_query_seconds = HistogramVec::new(
            HistogramOpts::new(
                "agent_ast_query_seconds",
                "Code-graph query latency (measured inside the backend)",
            ),
            &["backend", "verb"],
        )
        .unwrap();
        let ast_result_nodes = HistogramVec::new(
            HistogramOpts::new(
                "agent_ast_result_nodes",
                "Symbols/nodes returned by a code-graph query",
            ),
            &["backend", "verb"],
        )
        .unwrap();
        let ast_errors = IntCounterVec::new(
            Opts::new("agent_ast_errors_total", "Code-graph query errors"),
            &["backend", "verb"],
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
            &["op", "tenant"],
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
            Box::new(upstream_tokens.clone()),
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
            Box::new(router_decided.clone()),
            Box::new(router_no_candidate.clone()),
            Box::new(registry_upstreams.clone()),
            Box::new(registry_mutations.clone()),
            Box::new(router_dispatch.clone()),
            Box::new(router_failover.clone()),
            Box::new(router_inflight.clone()),
            Box::new(gate_verdicts.clone()),
            Box::new(gate_rounds.clone()),
            Box::new(gate_phase_seconds.clone()),
            Box::new(gate_issues.clone()),
            Box::new(gate_alternatives.clone()),
            Box::new(distill_jobs.clone()),
            Box::new(distill_lag_seconds.clone()),
            Box::new(graph_branches.clone()),
            Box::new(graph_join_wait_seconds.clone()),
            Box::new(graph_merges.clone()),
            Box::new(pool_members_alive.clone()),
            Box::new(pool_probe_seconds.clone()),
            Box::new(pool_dispatch_seconds.clone()),
            Box::new(pool_member_calls.clone()),
            Box::new(pool_member_inflight.clone()),
            Box::new(pool_member_latency.clone()),
            Box::new(pool_selects.clone()),
            Box::new(pool_member_saturated.clone()),
            Box::new(pool_saturation_shed.clone()),
            Box::new(grpc_overload_shed.clone()),
            Box::new(pool_member_state.clone()),
            Box::new(pool_member_latency_ewma.clone()),
            Box::new(mode_classifications.clone()),
            Box::new(mode_switches.clone()),
            Box::new(mode_switch_confidence.clone()),
            Box::new(prompt_fragments_selected.clone()),
            Box::new(dimension_summaries.clone()),
            Box::new(dimension_summarize_seconds.clone()),
            Box::new(dimension_recalls.clone()),
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
            Box::new(review_risk.clone()),
            Box::new(review_risk_score.clone()),
            Box::new(review_runs.clone()),
            Box::new(review_total_duration.clone()),
            Box::new(review_parallelism.clone()),
            Box::new(fleet_triggers.clone()),
            Box::new(fleet_reviews.clone()),
            Box::new(fleet_progress.clone()),
            Box::new(fleet_approvals.clone()),
            Box::new(fleet_approval_latency.clone()),
            Box::new(fleet_post_failures.clone()),
            Box::new(transport_posts.clone()),
            Box::new(transport_post_seconds.clone()),
            Box::new(transport_ratelimit.clone()),
            Box::new(config_store_ops.clone()),
            Box::new(config_store_op_seconds.clone()),
            Box::new(auth_verify.clone()),
            Box::new(authz_decisions.clone()),
            Box::new(grpc_server_rpc.clone()),
            Box::new(grpc_server_rpc_seconds.clone()),
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
            Box::new(context_switch_compactions.clone()),
            Box::new(context_tokens_shed.clone()),
            Box::new(context_summary_fallback.clone()),
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
            Box::new(ast_query_seconds.clone()),
            Box::new(ast_result_nodes.clone()),
            Box::new(ast_errors.clone()),
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
            upstream_tokens,
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
            router_decided,
            router_no_candidate,
            registry_upstreams,
            registry_mutations,
            router_dispatch,
            router_failover,
            router_inflight,
            gate_verdicts,
            gate_rounds,
            gate_phase_seconds,
            gate_issues,
            gate_alternatives,
            distill_jobs,
            distill_lag_seconds,
            graph_branches,
            graph_join_wait_seconds,
            graph_merges,
            pool_members_alive,
            pool_probe_seconds,
            pool_dispatch_seconds,
            pool_member_calls,
            pool_member_inflight,
            pool_member_latency,
            pool_selects,
            pool_member_saturated,
            pool_saturation_shed,
            grpc_overload_shed,
            pool_member_state,
            pool_member_latency_ewma,
            mode_classifications,
            mode_switches,
            mode_switch_confidence,
            prompt_fragments_selected,
            dimension_summaries,
            dimension_summarize_seconds,
            dimension_recalls,
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
            review_risk,
            review_risk_score,
            review_runs,
            review_total_duration,
            review_parallelism,
            fleet_triggers,
            fleet_reviews,
            fleet_progress,
            fleet_approvals,
            fleet_approval_latency,
            fleet_post_failures,
            fleet_repos,
            transport_posts,
            transport_post_seconds,
            transport_ratelimit,
            config_store_ops,
            config_store_op_seconds,
            auth_verify,
            authz_decisions,
            grpc_server_rpc,
            grpc_server_rpc_seconds,
            config_plane_tenants,
            rpc_labels,
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
            context_switch_compactions,
            context_tokens_shed,
            context_summary_fallback,
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
            ast_query_seconds,
            ast_result_nodes,
            ast_errors,
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

    /// A per-tenant recorder view over the curated loop-level families, binding
    /// `(session, user)` once so the loop records spend/activity attributed to this
    /// session (docs/design/multi-session/06-observability.md). The seam-health
    /// families stay on the label-less `Metrics`. Cheap: `Metrics` is a shallow clone.
    pub fn for_session(&self, session: &str, user: &str) -> SessionMetrics {
        SessionMetrics {
            inner: self.clone(),
            session: session.to_string(),
            user: user.to_string(),
        }
    }

    /// A per-`(tenant, repo)` recorder over the review-fleet families. `user` is the
    /// verified org (C25); `repo` is the operator-roster `FleetSession.repo` (`owner__name`),
    /// both `safe_segment`-valid upstream. The pair is admitted into the LRU backstop on
    /// construction, so the number of distinct labelled repos stays bounded — the
    /// least-recently-used pair's fleet series are removed on overflow. Recorded from the
    /// fleet orchestrator/approver/progress-feed in Phase 2 (docs/design/observability).
    pub fn for_fleet(&self, user: &str, repo: &str) -> FleetMetrics {
        if let Ok(mut lru) = self.fleet_repos.lock() {
            if let Some((evicted_user, evicted_repo)) = lru.admit(user, repo) {
                self.remove_fleet_series(&evicted_user, &evicted_repo);
            }
        }
        FleetMetrics {
            inner: self.clone(),
            user: user.to_string(),
            repo: repo.to_string(),
        }
    }

    /// Shrink the fleet-repo LRU cap and clear it — test-only, so the LRU-eviction
    /// backstop can be exercised without inserting `MAX_FLEET_REPOS` distinct repos.
    #[cfg(test)]
    fn set_fleet_repo_cap(&self, cap: usize) {
        if let Ok(mut lru) = self.fleet_repos.lock() {
            *lru = FleetRepoLru::new(cap);
        }
    }

    /// Shrink the config-plane tenant LRU cap and clear it — test-only, so the
    /// tenant-eviction backstop can be exercised without inserting `MAX_TENANTS`
    /// distinct tenants.
    #[cfg(test)]
    fn set_config_plane_tenant_cap(&self, cap: usize) {
        if let Ok(mut lru) = self.config_plane_tenants.lock() {
            *lru = TenantLru::new(cap);
        }
    }

    /// Shrink the `grpc_server_rpc` `rpc` high-water cap and clear it — test-only, so the
    /// `"other"` overflow collapse can be exercised without spraying `MAX_RPCS` paths.
    #[cfg(test)]
    fn set_rpc_label_cap(&self, cap: usize) {
        if let Ok(mut b) = self.rpc_labels.lock() {
            *b = RpcBound::new(cap);
        }
    }

    /// Remove every fleet-family series for one `(user, repo)` pair — used by the LRU
    /// backstop on eviction and by [`FleetMetrics::retire`]. Iterates the *enumerable*
    /// discriminator label values (the fleet families' `source`/`status`/`beat`/`outcome`/
    /// `transport` sets are all small constants); a series that never existed is a silent
    /// no-op. A transport kind not listed here (a deferred teams/irc/signal impl) would
    /// leave a frozen series — acceptable for a lifecycle backstop, and documented.
    fn remove_fleet_series(&self, user: &str, repo: &str) {
        for source in ["poll", "slack"] {
            let _ = self
                .fleet_triggers
                .remove_label_values(&[source, user, repo]);
        }
        for status in ["reviewing", "drafted", "superseded", "uptodate", "failed"] {
            let _ = self
                .fleet_reviews
                .remove_label_values(&[status, user, repo]);
        }
        for beat in ["found", "drafted", "posted"] {
            for outcome in ["posted", "softfailed", "skipped"] {
                let _ = self
                    .fleet_progress
                    .remove_label_values(&[beat, outcome, user, repo]);
            }
        }
        for outcome in ["posted", "already", "notfound"] {
            let _ = self
                .fleet_approvals
                .remove_label_values(&[outcome, user, repo]);
        }
        let _ = self
            .fleet_approval_latency
            .remove_label_values(&[user, repo]);
        for transport in ["slack", "matrix"] {
            let _ = self
                .fleet_post_failures
                .remove_label_values(&[transport, user, repo]);
        }
    }

    // --- loop-level instrumentation ---------------------------------------
    // The curated per-tenant families (`runs`/`tokens`/`cost`/`active`/…) are recorded
    // through [`SessionMetrics`] (see `for_session`), which binds `(session, user)`;
    // they intentionally have no label-less recorder here.

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
    /// One lifecycle hook dispatch (parity spec 22). Per-tenant via the ambient
    /// identity — hooks fire inside the scoped turn (Phase 5).
    pub fn on_hook(&self, hook: &str, point: &str) {
        self.hook_dispatches
            .with_label_values(&[hook, point, &ambient_tenant()])
            .inc();
    }
    /// One router decision: `routed` / `fellover` / `skipped_unhealthy` /
    /// `exhausted`, by target (parity spec 25).
    pub fn on_route_decision(&self, target: &str, decision: &str) {
        self.route_decisions
            .with_label_values(&[target, decision])
            .inc();
    }
    /// One task-router policy decision (model-router 02b). `role`/`task_mode`
    /// are closed enum names, `chosen` a configured upstream id, `rule` the
    /// matched rule index (`rule0`…) or `default` — all bounded cardinality.
    pub fn on_router_decided(&self, role: &str, task_mode: &str, chosen: &str, rule: &str) {
        self.router_decided
            .with_label_values(&[role, task_mode, chosen, rule])
            .inc();
    }
    /// The task-router's hard filter rejected every upstream for `role` —
    /// nothing was dialed (distinct from dispatch exhaustion).
    pub fn on_router_no_candidate(&self, role: &str) {
        self.router_no_candidate.with_label_values(&[role]).inc();
    }
    /// One provider-registry control-plane mutation (model-router 03). `op` is
    /// a closed set (`put|delete|enable|put_policy`) — bounded cardinality.
    pub fn on_registry_mutation(&self, op: &str) {
        self.registry_mutations.with_label_values(&[op]).inc();
    }
    /// One task-router dispatch attempt (model-router 04). All three labels are
    /// bounded: closed role set, configured upstream ids, closed outcome set.
    pub fn on_router_dispatch(&self, role: &str, upstream: &str, outcome: &str) {
        self.router_dispatch
            .with_label_values(&[role, upstream, outcome])
            .inc();
    }
    /// One task-router failover hop (04); `to` is empty for the plain Router.
    pub fn on_router_failover(&self, from: &str, to: &str, reason: &str) {
        self.router_failover
            .with_label_values(&[from, to, reason])
            .inc();
    }
    /// The per-upstream in-flight gauge (04 follow-up). Fed from both edges of
    /// the router's RAII guard — the release fires in Drop, so the gauge
    /// drains to 0 even for cancelled calls (last-writer-wins under
    /// concurrency; the drained state is exact).
    pub fn set_router_inflight(&self, upstream: &str, count: u32) {
        self.router_inflight
            .with_label_values(&[upstream])
            .set(i64::from(count));
    }
    /// The registry fleet size after a mutation (counts are clamped upstream by
    /// the store's `MAX_REGISTRY_UPSTREAMS` cap, so the cast is safe).
    pub fn set_registry_upstreams(&self, enabled: usize, disabled: usize) {
        self.registry_upstreams
            .with_label_values(&["true"])
            .set(enabled as i64);
        self.registry_upstreams
            .with_label_values(&["false"])
            .set(disabled as i64);
    }
    /// One completed consensus-gate run. `outcome` is a closed set
    /// (`pass|fixed|alternatives|exhausted|critic_error`); phase times are the wall
    /// totals across rounds. Issue counters feed the resolution-rate panel
    /// (`resolved / raised`); `dropped_no_evidence` is a critic-quality signal.
    #[allow(clippy::too_many_arguments)]
    pub fn on_gate(
        &self,
        outcome: &str,
        rounds: u8,
        generate_seconds: f64,
        critique_seconds: f64,
        issues_raised: u64,
        issues_resolved: u64,
        issues_outstanding: u64,
        dropped_no_evidence: u64,
        alternatives: u64,
    ) {
        self.gate_verdicts.with_label_values(&[outcome]).inc();
        self.gate_rounds.observe(f64::from(rounds));
        // Hostile/degenerate durations are clamped non-negative-finite before observe.
        let clamp = |s: f64| if s.is_finite() && s >= 0.0 { s } else { 0.0 };
        self.gate_phase_seconds
            .with_label_values(&["generate"])
            .observe(clamp(generate_seconds));
        self.gate_phase_seconds
            .with_label_values(&["critique"])
            .observe(clamp(critique_seconds));
        for (result, n) in [
            ("raised", issues_raised),
            ("resolved", issues_resolved),
            ("outstanding", issues_outstanding),
            ("dropped_no_evidence", dropped_no_evidence),
        ] {
            if n > 0 {
                self.gate_issues.with_label_values(&[result]).inc_by(n);
            }
        }
        if alternatives > 0 {
            self.gate_alternatives.inc_by(alternatives);
        }
    }
    /// One background-distiller job. `kind` is `summary|facts|alternatives`;
    /// `outcome` is a closed set (`succeeded|succeeded_no_output|failed|
    /// store_failed|injection_flagged|dropped`). `lag_seconds` = delivery → row
    /// durable; clamped (a poisoned histogram corrupts every later quantile).
    pub fn on_distill(&self, kind: &str, outcome: &str, lag_seconds: f64) {
        self.distill_jobs.with_label_values(&[kind, outcome]).inc();
        let lag = if lag_seconds.is_finite() && lag_seconds >= 0.0 {
            lag_seconds
        } else {
            0.0
        };
        self.distill_lag_seconds
            .with_label_values(&[kind])
            .observe(lag);
    }
    /// Cognition-graph fork (increment 05): one branch fate. `split` is a
    /// validated node id (bounded, safe segment) — cardinality is document-sized.
    pub fn on_graph_branch(&self, split: &str, fate: &str) {
        self.graph_branches.with_label_values(&[split, fate]).inc();
    }
    /// Cognition-graph fork: the join wait, per activation policy. Hostile
    /// numbers are zeroed, never observed (a NaN panics `observe`).
    pub fn on_graph_join_wait(&self, policy: &str, seconds: f64) {
        let s = if seconds.is_finite() && seconds >= 0.0 {
            seconds
        } else {
            0.0
        };
        self.graph_join_wait_seconds
            .with_label_values(&[policy])
            .observe(s);
    }
    /// Cognition-graph fork: one merge, by strategy and outcome.
    pub fn on_graph_merge(&self, strategy: &str, outcome: &str) {
        self.graph_merges
            .with_label_values(&[strategy, outcome])
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
    /// LLM pool: a member's live in-flight count (the load-balancing signal).
    pub fn set_pool_member_inflight(&self, member: &str, n: i64) {
        self.pool_member_inflight
            .with_label_values(&[member])
            .set(n);
    }
    /// LLM pool: one member's request latency.
    pub fn on_pool_member_latency(&self, member: &str, seconds: f64) {
        self.pool_member_latency
            .with_label_values(&[member])
            .observe(seconds);
    }
    /// LLM pool: one selection dispatch, by policy.
    pub fn on_pool_select(&self, policy: &str) {
        self.pool_selects.with_label_values(&[policy]).inc();
    }
    /// LLM pool: a member's current saturation state (1 = at cap, 0 = has room).
    pub fn set_pool_member_saturated(&self, member: &str, saturated: i64) {
        self.pool_member_saturated
            .with_label_values(&[member])
            .set(saturated);
    }
    /// LLM pool: a dispatch shed because every eligible member was saturated.
    /// A request was shed by the gRPC admission layer under overload (the
    /// server-side counterpart of the `RESOURCE_EXHAUSTED` the client sees).
    pub fn on_grpc_overload_shed(&self) {
        self.grpc_overload_shed.inc();
    }

    pub fn on_pool_saturation_shed(&self) {
        self.pool_saturation_shed.inc();
    }
    /// LLM pool: a member's current graded state (sets 1 for `state`, 0 for the
    /// others so a stale series can't linger).
    pub fn set_pool_member_state(&self, member: &str, state: &str) {
        for s in ["healthy", "degraded", "dead"] {
            self.pool_member_state
                .with_label_values(&[member, s])
                .set(i64::from(s == state));
        }
    }
    /// LLM pool: a member's smoothed latency EWMA (milliseconds).
    pub fn set_pool_member_latency_ewma(&self, member: &str, ms: i64) {
        self.pool_member_latency_ewma
            .with_label_values(&[member])
            .set(ms);
    }
    /// Task mode: one per-turn classification (`via` = prefilter|vote|failsafe).
    pub fn on_mode_classify(&self, mode: &str, via: &str) {
        self.mode_classifications
            .with_label_values(&[mode, via])
            .inc();
    }
    /// Task mode: a decided switch and its confidence.
    /// Situational system-prompt fragment update (docs/design/prompts/): `action` is
    /// `inserted` | `updated` | `removed`. Records neither the tags nor the text.
    pub fn on_prompt_fragments_selected(&self, mode: &str, action: &str) {
        self.prompt_fragments_selected
            .with_label_values(&[mode, action])
            .inc();
    }
    /// Dimensional memory: one filed per-dimension summary (adaptive-cognition 03).
    pub fn on_dimension_summary(&self, dimension: &str, is_new: bool) {
        self.dimension_summaries
            .with_label_values(&[dimension, if is_new { "true" } else { "false" }])
            .inc();
    }
    /// Dimensional memory: the per-step summarize-pass wall-clock.
    pub fn on_dimension_summarize(&self, seconds: f64) {
        self.dimension_summarize_seconds.observe(seconds);
    }
    /// Dimensional memory: a dimension-weighted recall.
    pub fn on_dimension_recall(&self, dimension: &str) {
        self.dimension_recalls.with_label_values(&[dimension]).inc();
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
    /// Review: risk synthesis — at-risk files, the max score, and gate failures.
    pub fn on_review_risk(&self, files: u64, max_score: f64, gate_failed: bool) {
        self.review_risk.with_label_values(&["files"]).inc_by(files);
        if gate_failed {
            self.review_risk.with_label_values(&["gate_failed"]).inc();
        }
        if max_score.is_finite() {
            self.review_risk_score.observe(max_score.clamp(0.0, 1.0));
        }
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
    /// One tool-call verifier verdict. `verdict` is `allow|revise|deny`; `mode` is
    /// `shadow|enforce`. Labels are bounded — callers pass built-in verifier names.
    pub fn on_verifier(&self, verifier: &str, verdict: &str, mode: &str) {
        self.verifier_verdicts
            .with_label_values(&[verifier, verdict, mode])
            .inc();
    }

    // --- provider instrumentation -----------------------------------------

    /// Per-upstream token attribution (`agent_upstream_tokens_total`): recorded
    /// by the metered provider WRAPPER, so internal role calls — gate critic,
    /// distiller, judge slots — are attributed under the config-selected
    /// upstream name (`glm`, `local`, …), which `agent_tokens_total` (main-loop
    /// only, model-id label) cannot see. Un-tenanted: a per-upstream cost view,
    /// like the provider health families.
    pub fn add_upstream_tokens(&self, upstream: &str, prompt: u64, completion: u64) {
        self.upstream_tokens
            .with_label_values(&[upstream, "prompt"])
            .inc_by(prompt);
        self.upstream_tokens
            .with_label_values(&[upstream, "completion"])
            .inc_by(completion);
    }

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
    /// Mode-aware compaction (adaptive-cognition 02): a switch reshape ran.
    pub fn on_switch_compaction(&self, from: &str, to: &str) {
        self.context_switch_compactions
            .with_label_values(&[from, to])
            .inc();
    }
    /// Tokens shed by a compaction, labelled by `trigger` (`budget`|`switch`).
    pub fn on_tokens_shed(&self, trigger: &str, shed: f64) {
        self.context_tokens_shed
            .with_label_values(&[trigger])
            .observe(shed);
    }
    /// A switch-compaction summary fell back (`kind` = `generic`|`drop`).
    pub fn on_summary_fallback(&self, kind: &str) {
        self.context_summary_fallback
            .with_label_values(&[kind])
            .inc();
    }

    // --- policy instrumentation -------------------------------------------

    pub fn on_authorize(&self, policy: &str, decision: &str, seconds: f64) {
        // Per-tenant: which tenant's model is hitting authorize decisions (config-plane
        // observability Phase 5). Read from the ambient identity (the loop scopes every
        // turn), `""` when unscoped. The latency sibling stays un-tenanted seam health.
        self.policy_authorize
            .with_label_values(&[policy, decision, &ambient_tenant()])
            .inc();
        self.policy_authorize_seconds.observe(seconds);
    }

    /// A guard rule matched a call: `category` is the rule family
    /// (`dangerous_command` / `sensitive_path`), `action` is what happened
    /// (`deny` / `prompt_denied` / `prompt_allowed`). Per-tenant via the ambient
    /// identity — guard denials are a per-tenant security signal (Phase 5).
    pub fn on_policy_guard(&self, category: &str, action: &str) {
        self.policy_guard
            .with_label_values(&[category, action, &ambient_tenant()])
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

    // --- ast (code-graph seam) instrumentation ----------------------------

    /// Record a code-graph query's latency + result size, labelled by backend + verb.
    pub fn on_ast_query(&self, backend: &str, verb: &str, seconds: f64, nodes: usize) {
        self.ast_query_seconds
            .with_label_values(&[backend, verb])
            .observe(seconds);
        self.ast_result_nodes
            .with_label_values(&[backend, verb])
            .observe(nodes as f64);
    }
    /// Count a code-graph query error, tagged with the verb.
    pub fn on_ast_error(&self, backend: &str, verb: &str) {
        self.ast_errors.with_label_values(&[backend, verb]).inc();
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

    // --- message transport (MessageTransport seam) instrumentation --------

    /// Count one message-transport post attempt and record its latency (config
    /// C37 / D2, Phase 3). `kind` is the transport impl's own `&'static str`
    /// (`slack`|`matrix`), inherently bounded — not model input, so no
    /// `safe_segment` gate is needed; `outcome` is one of the bounded
    /// `ok`|`ratelimited`|`error`. `seconds` is a wall-clock latency the caller
    /// measured; a hostile/NaN/negative value is clamped to `0.0` before
    /// `observe` (defense in depth, following the local clamp idiom).
    pub fn record_transport_post(&self, kind: &str, outcome: &str, seconds: f64) {
        self.transport_posts
            .with_label_values(&[kind, outcome])
            .inc();
        let secs = if seconds.is_finite() && seconds >= 0.0 {
            seconds
        } else {
            0.0
        };
        self.transport_post_seconds
            .with_label_values(&[kind])
            .observe(secs);
    }

    /// Count one message-transport rate-limit decision (config C37 / D2, Phase
    /// 3): `decision` is `admit` (the post reached the network) or `refuse` (the
    /// per-transport [`RateLimiter`](agent_core::RateLimiter) rejected it before
    /// the network). `kind` is the bounded transport-impl string.
    pub fn record_transport_ratelimit(&self, kind: &str, decision: &str) {
        self.transport_ratelimit
            .with_label_values(&[kind, decision])
            .inc();
    }

    // --- config-plane observability (docs/design/observability, Phase 4) --

    /// Count one config-store operation `{collection, op, outcome, tenant}` (the
    /// `MeteredBackend` decorator, `agent-runtime`). `collection` is bounded (one per
    /// card kind); `op` ∈ get|list|count|tenants|put|delete|ensure_tenant; `outcome`
    /// ∈ ok|error — all caller-supplied bounded constants. `tenant` is the verified
    /// org (C25) and **attacker-influenced**, so it is `safe_segment`-validated here
    /// (the recorder is a funnel — a malformed value is dropped, not sanitized) and
    /// admitted into the shared [`TenantLru`] backstop so the tenant dimension stays
    /// bounded; an eviction removes the evicted tenant's config-plane series.
    /// `collection` is likewise re-validated (defense in depth).
    pub fn record_config_store_op(&self, collection: &str, op: &str, outcome: &str, tenant: &str) {
        if !agent_core::safe_segment(collection) || !agent_core::safe_segment(tenant) {
            return;
        }
        self.admit_config_plane_tenant(
            tenant,
            TenantSeries::ConfigStore {
                collection: collection.to_string(),
                op: op.to_string(),
                outcome: outcome.to_string(),
            },
        );
        self.config_store_ops
            .with_label_values(&[collection, op, outcome, tenant])
            .inc();
    }

    /// Observe one config-store *call* latency `{op}` (get|list|count|tenants|apply) —
    /// seam-health, un-tenanted. A hostile/NaN/negative `seconds` is clamped to `0.0`
    /// before `observe` (the local clamp idiom).
    pub fn record_config_store_latency(&self, op: &str, seconds: f64) {
        let secs = if seconds.is_finite() && seconds >= 0.0 {
            seconds
        } else {
            0.0
        };
        self.config_store_op_seconds
            .with_label_values(&[op])
            .observe(secs);
    }

    /// Count one bearer-token verification `{outcome}` at the auth layer (`ok`|`error`),
    /// bridged via the `AuthObserver` callback so `agent-grpc` keeps no `agent-metrics`
    /// dependency. No tenant here — a failed verify has no trustworthy tenant, and a
    /// successful one is attributed on the `grpc.server` span.
    pub fn record_auth_verify(&self, outcome: &str) {
        self.auth_verify.with_label_values(&[outcome]).inc();
    }

    /// Count one RBAC authorization decision `{action, resource_type, decision}` — all
    /// bounded enums from `agent-core` (`Action`/`ResourceType` `as_str`; decision ∈
    /// allow|deny). No tenant label: the decision rides the `grpc.server` span (which
    /// already carries the validated tenant), keeping this a low-cardinality security
    /// counter.
    pub fn record_authz_decision(&self, action: &str, resource_type: &str, decision: &str) {
        self.authz_decisions
            .with_label_values(&[action, resource_type, decision])
            .inc();
    }

    /// Count one gRPC server request `{rpc, outcome, tenant}` and observe its latency
    /// `{rpc}` (the `MetricsLayer` tower service, bridged via the `RpcObserver`
    /// callback). `rpc` is the bounded request path (`/pkg.Service/Method`); `outcome`
    /// is `ok` or the grpc code name. `tenant` is the **verified** org from the
    /// post-auth identity header — attacker-influenced upstream, so `safe_segment`-
    /// validated here and admitted into the shared [`TenantLru`] backstop; an unset or
    /// malformed tenant is recorded as the empty label (still bounded). Hostile/NaN/
    /// negative `seconds` is clamped to `0.0` before `observe`.
    pub fn record_grpc_rpc(&self, rpc: &str, outcome: &str, tenant: &str, seconds: f64) {
        // The RPC path is attacker-controllable, so it is bounded by a high-water guard:
        // a known/admissible path records as itself; once the cap is reached an unknown
        // path collapses to the `"other"` sentinel (no unbounded `rpc` dimension). The
        // *effective* label is what flows into both the counter and the tenant LRU below,
        // so eviction removes exactly the series that were recorded.
        let admitted = match self.rpc_labels.lock() {
            Ok(mut b) => b.admit(rpc),
            // A poisoned lock must not drop the sample; fold to the sentinel (fail-safe,
            // bounded) rather than record the raw path.
            Err(_) => false,
        };
        let rpc = if admitted { rpc } else { RPC_OTHER };
        // The empty tenant (unauthenticated / no identity header) is a valid, bounded
        // label value; a *non-empty* value must be a safe segment or it is dropped to
        // empty rather than recorded verbatim.
        let tenant = if tenant.is_empty() || agent_core::safe_segment(tenant) {
            tenant
        } else {
            ""
        };
        if !tenant.is_empty() {
            self.admit_config_plane_tenant(
                tenant,
                TenantSeries::GrpcRpc {
                    rpc: rpc.to_string(),
                    outcome: outcome.to_string(),
                },
            );
        }
        self.grpc_server_rpc
            .with_label_values(&[rpc, outcome, tenant])
            .inc();
        let secs = if seconds.is_finite() && seconds >= 0.0 {
            seconds
        } else {
            0.0
        };
        self.grpc_server_rpc_seconds
            .with_label_values(&[rpc])
            .observe(secs);
    }

    /// Record `series` under `tenant` in the shared config-plane tenant LRU, moving
    /// `tenant` to most-recently-used. When admitting a *new* tenant overflows the cap,
    /// the least-recently-used tenant is evicted and each of its recorded series removed
    /// from the registry — so the `tenant` dimension stays bounded under churn/misconfig
    /// (the config-plane analogue of the fleet `repo` LRU). Unlike the fleet families
    /// (small enumerable discriminators), these carry open-ended discriminators (any
    /// card `collection`, any RPC path), so the LRU remembers the exact tuples it
    /// admitted and replays them here for precise removal.
    fn admit_config_plane_tenant(&self, tenant: &str, series: TenantSeries) {
        let evicted = match self.config_plane_tenants.lock() {
            Ok(mut lru) => lru.admit(tenant, series),
            Err(_) => return,
        };
        // Removal touches only the Prometheus vecs, not the LRU — the lock is already
        // released, so eviction can never deadlock against a concurrent recorder.
        if let Some((evicted_tenant, recorded)) = evicted {
            for s in recorded {
                match s {
                    TenantSeries::ConfigStore {
                        collection,
                        op,
                        outcome,
                    } => {
                        let _ = self.config_store_ops.remove_label_values(&[
                            &collection,
                            &op,
                            &outcome,
                            &evicted_tenant,
                        ]);
                    }
                    TenantSeries::GrpcRpc { rpc, outcome } => {
                        let _ = self.grpc_server_rpc.remove_label_values(&[
                            &rpc,
                            &outcome,
                            &evicted_tenant,
                        ]);
                    }
                }
            }
        }
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

    /// Count a session-history mutation, labelled by op. Per-tenant via the ambient
    /// identity — session lifecycle is per-user (Phase 5).
    pub fn on_session_op(&self, op: &str) {
        self.session_ops
            .with_label_values(&[op, &ambient_tenant()])
            .inc();
    }
    /// Count checkpoint objects reclaimed by a prune. **Un-tenanted seam health**: a
    /// prune is a bulk reaper sweeping idle sessions across *many* tenants in one call,
    /// so the batch count cannot be attributed to a single tenant (the reaper is not a
    /// tenant). Per-tenant session activity rides `agent_session_ops_total` instead.
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

/// The LRU cap on distinct fleet `(user, repo)` label pairs. Fleet repos come from the
/// operator `FleetSession` roster (`O(sessions)` — locked at low hundreds), so this is a
/// backstop that normal operation never reaches; it bounds the `repo` dimension under
/// misconfig/churn, the same lifecycle mitigation the session map applies to `(session, user)`.
const MAX_FLEET_REPOS: usize = 1024;

/// Bounded LRU over distinct fleet `(user, repo)` pairs — the lifecycle backstop for the
/// `repo` metric dimension (docs/design/observability/01-metric-census.md). Insertion order
/// is least-recently-used first; re-admitting a live pair moves it to the back. When a new
/// pair would exceed `cap`, the least-recently-used pair is evicted and its fleet series
/// removed by [`Metrics::remove_fleet_series`].
struct FleetRepoLru {
    cap: usize,
    /// `(user, repo)` pairs, least-recently-used at the front.
    seen: std::collections::VecDeque<(String, String)>,
}

impl FleetRepoLru {
    fn new(cap: usize) -> Self {
        Self {
            // A zero cap would make every admit evict itself; clamp to at least one so a
            // hostile/degenerate config can't wedge the recorder.
            cap: cap.max(1),
            seen: std::collections::VecDeque::new(),
        }
    }

    /// Admit `(user, repo)`, returning the evicted pair when admission overflowed `cap`.
    /// A pair already live is moved to the back (most-recently-used) with no eviction.
    fn admit(&mut self, user: &str, repo: &str) -> Option<(String, String)> {
        if let Some(pos) = self.seen.iter().position(|(u, r)| u == user && r == repo) {
            if let Some(pair) = self.seen.remove(pos) {
                self.seen.push_back(pair);
            }
            return None;
        }
        let evicted = if self.seen.len() >= self.cap {
            self.seen.pop_front()
        } else {
            None
        };
        self.seen.push_back((user.to_string(), repo.to_string()));
        evicted
    }
}

/// The ambient tenant (verified `user`, the C25 canonical dimension) for a seam-decorator
/// family recorded outside [`SessionMetrics`] — the policy/guard/hook/session-op families
/// swept in Phase 5. Read from the task-local identity **at record time**, which is sound
/// because it runs inline on the recording task (not the batch trace exporter — unlike the
/// deferred ambient-span hazard). `""` when no identity is scoped (single-tenant / local
/// path); a non-`safe_segment` value fails closed to `""`, so a hostile segment never
/// becomes a label. Loop-*spend* families instead bind `(session, user)` explicitly via
/// [`SessionMetrics`]; these decorator families have no such handle, so the ambient read
/// is the natural, DRY fit.
fn ambient_tenant() -> String {
    agent_core::current_identity()
        .map(|k| k.user.as_str().to_string())
        .filter(|u| agent_core::safe_segment(u))
        .unwrap_or_default()
}

/// The LRU cap on distinct config-plane `tenant` label values, shared by the two
/// tenant-labelled config-plane families (`config_store_ops` + `grpc_server_rpc`).
/// Tenants are verified orgs (C25), locked at low hundreds like sessions, so this is a
/// backstop normal operation never reaches; it bounds the `tenant` dimension under
/// misconfig/churn — the config-plane analogue of [`MAX_FLEET_REPOS`].
const MAX_TENANTS: usize = 1024;

/// High-water cap on distinct `grpc_server_rpc` `rpc` label values (see the `rpc_labels`
/// field). The real gRPC method set is fixed and well under this; the headroom absorbs
/// legitimate growth while still collapsing a junk-path flood to `"other"`.
const MAX_RPCS: usize = 256;

/// The sentinel `rpc` label a path collapses to once [`RpcBound`] is full — so an
/// unbounded flood of unknown paths cannot grow the `rpc` dimension without limit.
const RPC_OTHER: &str = "other";

/// High-water bound over distinct `rpc` label values. Unlike [`TenantLru`] this never
/// evicts: a real method path, once admitted, must stay its own label (evicting it would
/// strand its series and misroute later hits to `"other"`). Once `cap` distinct paths are
/// known, any *new* path is refused (→ the caller records it as [`RPC_OTHER`]).
struct RpcBound {
    cap: usize,
    seen: std::collections::HashSet<String>,
}

impl RpcBound {
    fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            seen: std::collections::HashSet::new(),
        }
    }

    /// `true` ⇒ `rpc` may be its own label (already known, or there was room to admit it);
    /// `false` ⇒ the cap is full and `rpc` is new, so the caller must fold it into
    /// [`RPC_OTHER`].
    fn admit(&mut self, rpc: &str) -> bool {
        if self.seen.contains(rpc) {
            return true;
        }
        if self.seen.len() >= self.cap {
            return false;
        }
        self.seen.insert(rpc.to_string());
        true
    }
}

/// One config-plane counter series recorded under a tenant, remembered so the tenant
/// LRU can remove exactly the series it admitted on eviction. The config-plane
/// discriminators are open-ended (any card `collection`, any RPC path), so — unlike the
/// fleet families' small enumerable sets — precise eviction requires replaying the
/// recorded tuples rather than enumerating constants.
#[derive(Clone, PartialEq, Eq, Hash)]
enum TenantSeries {
    ConfigStore {
        collection: String,
        op: String,
        outcome: String,
    },
    GrpcRpc {
        rpc: String,
        outcome: String,
    },
}

/// Bounded LRU over distinct config-plane `tenant` label values — the lifecycle
/// backstop for the config-plane `tenant` metric dimension
/// (docs/design/observability/01-metric-census.md). Least-recently-used tenant first;
/// recording under a live tenant moves it to the back. Each tenant remembers the exact
/// [`TenantSeries`] recorded under it, so on eviction [`Metrics`] can remove precisely
/// those series from the registry.
struct TenantLru {
    cap: usize,
    /// Tenants in LRU order (least-recently-used at the front), each with the set of
    /// series recorded under it.
    seen: std::collections::VecDeque<(String, std::collections::HashSet<TenantSeries>)>,
}

impl TenantLru {
    fn new(cap: usize) -> Self {
        Self {
            // A zero cap would make every admit evict itself; clamp to at least one so a
            // hostile/degenerate config can't wedge the recorder.
            cap: cap.max(1),
            seen: std::collections::VecDeque::new(),
        }
    }

    /// Record `series` under `tenant`, returning the evicted `(tenant, its series)` when
    /// admitting a **new** tenant overflowed `cap`. A tenant already live is moved to the
    /// back (most-recently-used) and `series` added to its set — no eviction.
    fn admit(&mut self, tenant: &str, series: TenantSeries) -> Option<(String, Vec<TenantSeries>)> {
        if let Some(pos) = self.seen.iter().position(|(t, _)| t == tenant) {
            if let Some(mut entry) = self.seen.remove(pos) {
                entry.1.insert(series);
                self.seen.push_back(entry);
            }
            return None;
        }
        let evicted = if self.seen.len() >= self.cap {
            self.seen
                .pop_front()
                .map(|(t, set)| (t, set.into_iter().collect()))
        } else {
            None
        };
        let mut set = std::collections::HashSet::new();
        set.insert(series);
        self.seen.push_back((tenant.to_string(), set));
        evicted
    }
}

/// A per-tenant recorder over the curated loop-level families, binding `(session,
/// user)` once (built via [`Metrics::for_session`]). The agent loop holds one per
/// session and records spend/activity through it, so a run is attributable per tenant
/// without threading labels to every call (docs/design/multi-session/06-observability.md).
/// The seam-health families are recorded through the plain label-less [`Metrics`].
#[derive(Clone)]
pub struct SessionMetrics {
    inner: Metrics,
    session: String,
    user: String,
}

impl SessionMetrics {
    /// This session's `(session, user)` label pair, appended to each curated family.
    fn tenant(&self) -> [&str; 2] {
        [self.session.as_str(), self.user.as_str()]
    }

    pub fn run_started(&self) {
        self.inner.active.with_label_values(&self.tenant()).set(1);
    }
    pub fn run_finished(&self, outcome: &str, seconds: f64) {
        let (s, u) = (self.session.as_str(), self.user.as_str());
        self.inner.active.with_label_values(&[s, u]).set(0);
        self.inner.runs.with_label_values(&[outcome, s, u]).inc();
        self.inner
            .run_seconds
            .with_label_values(&[s, u])
            .observe(seconds);
    }
    pub fn on_iteration(&self) {
        self.inner
            .iterations
            .with_label_values(&self.tenant())
            .inc();
    }
    pub fn on_api_call(&self, model: &str, finish_reason: &str, seconds: f64) {
        let (s, u) = (self.session.as_str(), self.user.as_str());
        self.inner
            .api_calls
            .with_label_values(&[model, finish_reason, s, u])
            .inc();
        // Latency stays a per-model health histogram (un-tenanted).
        self.inner
            .api_call_seconds
            .with_label_values(&[model])
            .observe(seconds);
    }
    pub fn add_tokens(&self, model: &str, prompt: u64, completion: u64) {
        let (s, u) = (self.session.as_str(), self.user.as_str());
        self.inner
            .tokens
            .with_label_values(&[model, "prompt", s, u])
            .inc_by(prompt);
        self.inner
            .tokens
            .with_label_values(&[model, "completion", s, u])
            .inc_by(completion);
    }
    /// Record a turn's USD cost, one line per billed `kind`
    /// (`input`/`output`/`cache_read`/`cache_write`).
    pub fn add_cost(
        &self,
        model: &str,
        input: f64,
        output: f64,
        cache_read: f64,
        cache_write: f64,
    ) {
        let (s, u) = (self.session.as_str(), self.user.as_str());
        for (kind, usd) in [
            ("input", input),
            ("output", output),
            ("cache_read", cache_read),
            ("cache_write", cache_write),
        ] {
            // Only a finite, positive amount: `inc_by` panics on a negative value, and
            // a non-finite (NaN/inf) from a malformed/hostile price row would poison the
            // counter. Both are dropped defensively.
            if usd.is_finite() && usd > 0.0 {
                self.inner
                    .cost_usd
                    .with_label_values(&[model, kind, s, u])
                    .inc_by(usd);
            }
        }
    }
    /// Prompt-cache token counts for a turn: `read` = served from cache (a hit),
    /// `write` = written into it.
    pub fn add_cache_tokens(&self, model: &str, read: u64, write: u64) {
        let (s, u) = (self.session.as_str(), self.user.as_str());
        if read > 0 {
            self.inner
                .cache_tokens
                .with_label_values(&[model, "read", s, u])
                .inc_by(read);
        }
        if write > 0 {
            self.inner
                .cache_tokens
                .with_label_values(&[model, "write", s, u])
                .inc_by(write);
        }
    }
    pub fn set_context(&self, prompt_tokens: i64, messages: i64) {
        self.inner
            .context_tokens
            .with_label_values(&self.tenant())
            .set(prompt_tokens);
        self.inner
            .context_messages
            .with_label_values(&self.tenant())
            .set(messages);
    }
    pub fn on_tool(&self, tool: &str, status: &str) {
        let (s, u) = (self.session.as_str(), self.user.as_str());
        self.inner
            .tool_calls
            .with_label_values(&[tool, status, s, u])
            .inc();
    }
    pub fn on_mode_switch(&self, from: &str, to: &str, confidence: f64) {
        let (s, u) = (self.session.as_str(), self.user.as_str());
        self.inner
            .mode_switches
            .with_label_values(&[from, to, s, u])
            .inc();
        // Confidence stays an un-tenanted distribution.
        self.inner.mode_switch_confidence.observe(confidence);
    }

    /// Retire this session's **gauge** series on session end
    /// ([`crate::Metrics`] is process-global, so Prometheus would otherwise retain a
    /// dead session's `agent_active = 1` until restart — an over-count of live
    /// sessions). Removing the `(session, user)`-only gauges is exact; the cumulative
    /// counter families are left to accumulate (harmless frozen series, bounded under
    /// the locked low-hundreds-sessions constraint — see 06-observability.md).
    /// Idempotent: a missing series is a no-op.
    pub fn retire(&self) {
        let t = self.tenant();
        let _ = self.inner.active.remove_label_values(&t);
        let _ = self.inner.context_tokens.remove_label_values(&t);
        let _ = self.inner.context_messages.remove_label_values(&t);
    }
}

/// A per-`(tenant, repo)` recorder over the review-fleet families, binding `(user, repo)`
/// once (built via [`Metrics::for_fleet`]). The fleet orchestrator/approver/progress-feed
/// hold one per session and record lifecycle events through it, so the fleet is
/// attributable per tenant and per repo (docs/design/observability/01-metric-census.md).
/// PR is deliberately **not** a field here — it rides the `fleet.*` OTEL span as an
/// attribute, never a Prometheus label. Discriminator values are recorded verbatim; the
/// caller passes bounded constants (`source`/`status`/`beat`/`outcome`/`transport`).
#[derive(Clone)]
pub struct FleetMetrics {
    inner: Metrics,
    user: String,
    repo: String,
}

impl FleetMetrics {
    /// This recorder's `(user, repo)` label pair, for the families labelled by exactly it.
    fn pair(&self) -> [&str; 2] {
        [self.user.as_str(), self.repo.as_str()]
    }

    /// A trigger fired for this repo (`source` = `poll` | `slack`).
    pub fn on_trigger(&self, source: &str) {
        self.inner
            .fleet_triggers
            .with_label_values(&[source, self.user.as_str(), self.repo.as_str()])
            .inc();
    }

    /// A review lifecycle transition (`status` = `reviewing` | `drafted` | `superseded` |
    /// `uptodate` | `failed`). `failed` marks a review that ran but produced no draft
    /// (the run errored — a truncation cap or provider fault — even after the core
    /// loop's forced finalize turn), so the failure is observable rather than silent.
    pub fn on_review(&self, status: &str) {
        self.inner
            .fleet_reviews
            .with_label_values(&[status, self.user.as_str(), self.repo.as_str()])
            .inc();
    }

    /// A progress-feed beat (`beat` = `found` | `drafted` | `posted`; `outcome` =
    /// `posted` | `softfailed` | `skipped`).
    pub fn on_progress(&self, beat: &str, outcome: &str) {
        self.inner
            .fleet_progress
            .with_label_values(&[beat, outcome, self.user.as_str(), self.repo.as_str()])
            .inc();
    }

    /// An approval outcome (`outcome` = `posted` | `already` | `notfound`).
    pub fn on_approval(&self, outcome: &str) {
        self.inner
            .fleet_approvals
            .with_label_values(&[outcome, self.user.as_str(), self.repo.as_str()])
            .inc();
    }

    /// The drafted→posted approval latency. A non-finite or negative value (a hostile or
    /// clock-skewed span pair would produce one) is dropped, mirroring the cost/token
    /// clamp on [`SessionMetrics`], so it can never poison the histogram.
    pub fn observe_approval_latency(&self, seconds: f64) {
        if seconds.is_finite() && seconds >= 0.0 {
            self.inner
                .fleet_approval_latency
                .with_label_values(&self.pair())
                .observe(seconds);
        }
    }

    /// A failed progress/approval post, by `transport` kind (the transport's `kind()`).
    pub fn on_post_failure(&self, transport: &str) {
        self.inner
            .fleet_post_failures
            .with_label_values(&[transport, self.user.as_str(), self.repo.as_str()])
            .inc();
    }

    /// Retire this `(user, repo)` pair's fleet series on session end — the fleet analogue
    /// of [`SessionMetrics::retire`], so a torn-down session's counters don't linger.
    /// Idempotent: a missing series is a no-op.
    pub fn retire(&self) {
        self.inner.remove_fleet_series(&self.user, &self.repo);
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
    use rstest::rstest;

    #[test]
    fn encodes_incremented_metrics() {
        let m = Metrics::new();
        let sm = m.for_session("sess-1", "alice");
        sm.on_iteration();
        sm.on_api_call("test-model", "stop", 0.5);
        sm.add_tokens("test-model", 100, 20);
        sm.add_cost("test-model", 0.003, 0.015, 0.0003, 0.0);
        sm.add_cache_tokens("test-model", 80, 20);
        sm.set_context(100, 4);
        sm.on_tool("bash", "ok");
        m.on_verifier("schema", "allow", "shadow");
        sm.run_finished("success", 1.5);
        m.on_grpc_overload_shed();

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
            "agent_grpc_overload_shed_total",
        ] {
            assert!(text.contains(name), "missing metric `{name}` in:\n{text}");
        }
        assert!(text.contains("test-model"));
    }

    /// Per-upstream attribution: the wrapper-recorded family must expose the
    /// NAMED upstream — the arena's cost columns scrape these exact strings.
    #[test]
    fn add_upstream_tokens_labels_by_upstream_name() {
        let m = Metrics::new();
        m.add_upstream_tokens("glm", 120, 40);
        m.add_upstream_tokens("local", 5, 7);
        let text = m.encode_text();
        assert!(
            text.contains(r#"agent_upstream_tokens_total{kind="prompt",upstream="glm"} 120"#),
            "{text}"
        );
        assert!(
            text.contains(r#"agent_upstream_tokens_total{kind="completion",upstream="local"} 7"#),
            "{text}"
        );
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
    fn on_gate_records_outcome_rounds_phases_issues_alternatives() {
        let m = Metrics::new();
        m.on_gate("fixed", 2, 1.5, 0.8, 3, 2, 0, 1, 0);
        m.on_gate("alternatives", 1, 0.9, 0.4, 0, 0, 0, 0, 2);
        let text = m.encode_text();
        for name in [
            "agent_gate_verdicts_total",
            "agent_gate_rounds",
            "agent_gate_phase_duration_seconds",
            "agent_gate_issues_total",
            "agent_gate_alternatives_total",
        ] {
            assert!(text.contains(name), "missing metric `{name}` in:\n{text}");
        }
        assert!(
            text.contains("outcome=\"fixed\"")
                && text.contains("outcome=\"alternatives\"")
                && text.contains("phase=\"critique\"")
                && text.contains("result=\"resolved\""),
            "labels missing: {text}"
        );
        assert!(
            text.contains("agent_gate_alternatives_total 2"),
            "alternatives count: {text}"
        );
    }

    #[test]
    fn adversarial_on_gate_hostile_durations_do_not_poison() {
        let m = Metrics::new();
        // NaN / negative / infinite phase times are clamped to 0 before observe —
        // a histogram fed NaN would poison every quantile after it.
        m.on_gate("pass", 1, f64::NAN, -5.0, 0, 0, 0, 0, 0);
        m.on_gate("pass", 1, f64::INFINITY, 0.1, 0, 0, 0, 0, 0);
        let text = m.encode_text();
        assert!(
            text.contains("agent_gate_phase_duration_seconds_count"),
            "{text}"
        );
        assert!(!text.contains("NaN"), "NaN leaked into export: {text}");
    }

    #[test]
    fn positive_graph_fork_families_record() {
        let m = Metrics::new();
        m.on_graph_branch("split_impl", "won");
        m.on_graph_branch("split_impl", "lost");
        m.on_graph_join_wait("all", 1.25);
        m.on_graph_merge("compare", "picked");
        let text = m.encode_text();
        assert!(
            text.contains(r#"agent_graph_branches_total{fate="won",split="split_impl"} 1"#),
            "{text}"
        );
        assert!(
            text.contains(r#"agent_graph_merge_total{outcome="picked",strategy="compare"} 1"#),
            "{text}"
        );
        assert!(
            text.contains("agent_graph_join_wait_seconds_count"),
            "{text}"
        );
    }

    #[test]
    fn adversarial_graph_join_wait_hostile_seconds_zeroed() {
        let m = Metrics::new();
        m.on_graph_join_wait("any", f64::NAN);
        m.on_graph_join_wait("any", -3.0);
        m.on_graph_join_wait("any", f64::INFINITY);
        let text = m.encode_text();
        assert!(!text.contains("NaN"), "NaN leaked into export: {text}");
    }

    #[test]
    fn add_cost_drops_non_finite_and_non_positive_lines() {
        let m = Metrics::new();
        // Must not panic (inc_by panics on negatives; NaN/inf would poison the
        // counter). Only the one finite-positive line is recorded.
        m.for_session("s", "u")
            .add_cost("m", f64::NAN, -1.0, f64::INFINITY, 0.003);
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
        m.on_switch_compaction("implement", "review");
        m.on_tokens_shed("switch", 5000.0);
        m.on_summary_fallback("drop");
        m.on_dimension_summary("coding", true);
        m.on_dimension_summarize(0.02);
        m.on_dimension_recall("coding");
        m.on_authorize("auto-approve", "approved", 0.0001);
        m.on_search_query("tantivy", "literal", 0.002, 5);
        m.observe_reindex("tantivy", 0.5, 120);
        m.on_search_reindex("tantivy", "startup");
        m.set_search_fresh("tantivy", true);
        m.on_search_error("tantivy", "query");
        m.on_ast_query("go", "callers", 0.002, 7);
        m.on_ast_error("go", "callers");
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
            "agent_context_switch_compactions_total",
            "agent_context_tokens_shed",
            "agent_context_summary_fallback_total",
            "agent_dimension_summaries_total",
            "agent_dimension_summarize_duration_seconds",
            "agent_dimension_recall_total",
            "agent_policy_authorize_total",
            "agent_policy_authorize_seconds",
            "agent_search_query_seconds",
            "agent_search_hits",
            "agent_search_index_seconds",
            "agent_search_index_files",
            "agent_search_index_fresh",
            "agent_search_errors_total",
            "agent_search_reindex_total",
            "agent_ast_query_seconds",
            "agent_ast_result_nodes",
            "agent_ast_errors_total",
            "agent_repo_op_seconds",
            "agent_repo_errors_total",
            "agent_repo_worktrees_live",
            "agent_repo_fetch_seconds",
        ] {
            assert!(text.contains(name), "missing metric `{name}` in:\n{text}");
        }
    }

    // --- multi-session: per-tenant labels + retire (increment 06) -----------

    #[test]
    fn positive_session_metrics_carry_tenant_labels() {
        let m = Metrics::new();
        let alice = m.for_session("sess-1", "alice");
        alice.run_started();
        alice.on_api_call("gpt", "stop", 0.1);
        alice.add_tokens("gpt", 10, 5);
        alice.set_context(10, 2);
        alice.on_tool("bash", "ok");
        let text = m.encode_text();

        // Every curated family a run touches carries this session's `(session, user)`.
        for fam in [
            "agent_active",
            "agent_api_calls_total",
            "agent_tokens_total",
            "agent_context_tokens",
            "agent_tool_calls_total",
        ] {
            let line = text
                .lines()
                .find(|l| l.starts_with(&format!("{fam}{{")))
                .unwrap_or_else(|| panic!("no series for {fam} in:\n{text}"));
            assert!(
                line.contains("session=\"sess-1\"") && line.contains("user=\"alice\""),
                "{fam} missing tenant labels: {line}"
            );
        }
        // The active gauge reads 1 for the live session.
        let active = text
            .lines()
            .find(|l| l.starts_with("agent_active{"))
            .unwrap();
        assert!(active.trim_end().ends_with(" 1"), "active not 1: {active}");
    }

    #[test]
    fn boundary_retire_removes_the_gauge_series() {
        let m = Metrics::new();
        let alice = m.for_session("sess-1", "alice");
        alice.run_started();
        alice.add_tokens("gpt", 10, 5); // a cumulative counter
        assert!(m.encode_text().contains("agent_active{session=\"sess-1\""));

        alice.retire();
        let after = m.encode_text();
        // The gauge series is gone (no stale `agent_active = 1` for a dead session)…
        assert!(
            !after.lines().any(|l| l.starts_with("agent_active{")),
            "active gauge not retired:\n{after}"
        );
        // …while the cumulative counter is intentionally kept (frozen, not removed).
        assert!(
            after.contains("agent_tokens_total"),
            "counters should persist:\n{after}"
        );
    }

    #[test]
    fn negative_seam_health_families_stay_label_less() {
        // Regression guard: metrics recorded through the plain `Metrics` (seam health)
        // must NEVER gain a session/user label — only the curated loop families do.
        let m = Metrics::new();
        m.on_tool_exec("edit", 0.001);
        let text = m.encode_text();
        for line in text
            .lines()
            .filter(|l| l.starts_with("agent_tool_exec_seconds"))
        {
            assert!(
                !line.contains("session=") && !line.contains("user="),
                "a health metric leaked a tenant label: {line}"
            );
        }
    }

    // --- Phase 5: seam-decorator families swept to +tenant --------------------
    //
    // policy_authorize / policy_guard / hook_dispatches / session_ops gain a `tenant`
    // label read from the ambient identity at record time (`ambient_tenant`). These
    // recorders run inside the scoped turn, so a well-formed identity is present; the
    // recorder's own `safe_segment` funnel drops a hostile segment to `""`.

    /// Build a `(user, session)` key field-wise (bypassing `parse`'s fail-closed
    /// validation) so an adversarial `user` can be forced into the ambient scope to
    /// exercise the recorder's own `safe_segment` funnel.
    fn key_with_user(user: &str) -> agent_core::SessionKey {
        agent_core::SessionKey {
            user: agent_core::UserId::new(user),
            session: agent_core::SessionId::new("s"),
        }
    }

    #[rstest]
    // desc: recorded inside a well-formed scope → the ambient user becomes the tenant label.
    #[case::positive_scoped_user(Some("acme"), "acme")]
    // desc: recorded outside any scope (single-tenant/local path) → tenant is the empty label.
    #[case::corner_unscoped_empty(None, "")]
    // desc (adversarial): a hostile user segment (traversal) reaches the recorder funnel → dropped to "".
    #[case::adversarial_hostile_user_dropped(Some("../../etc"), "")]
    #[tokio::test]
    async fn swept_family_carries_ambient_tenant(
        #[case] scope_user: Option<&str>,
        #[case] want_tenant: &str,
    ) {
        let m = Metrics::new();
        let record = || m.on_session_op("checkpoint");
        match scope_user {
            Some(u) => agent_core::scope(key_with_user(u), async { record() }).await,
            None => record(),
        }
        let text = m.encode_text();
        assert!(
            line_with(
                &text,
                "agent_session_ops_total",
                &[("op", "checkpoint"), ("tenant", want_tenant)]
            )
            .is_some(),
            "expected session_ops tenant={want_tenant:?}:\n{text}"
        );
    }

    // desc: every swept family gains the tenant label under a scope — one representative
    // record per family, all attributed to the ambient tenant.
    #[tokio::test]
    async fn all_swept_families_carry_tenant_under_scope() {
        let m = Metrics::new();
        agent_core::scope(key_with_user("acme"), async {
            m.on_authorize("bash", "allow", 0.001);
            m.on_policy_guard("dangerous_command", "deny");
            m.on_hook("audit", "pre_tool");
            m.on_session_op("fork");
        })
        .await;
        let text = m.encode_text();
        for (family, wants) in [
            (
                "agent_policy_authorize_total",
                vec![("decision", "allow"), ("tenant", "acme")],
            ),
            (
                "agent_policy_guard_total",
                vec![("action", "deny"), ("tenant", "acme")],
            ),
            (
                "agent_hook_dispatches_total",
                vec![("point", "pre_tool"), ("tenant", "acme")],
            ),
            (
                "agent_session_ops_total",
                vec![("op", "fork"), ("tenant", "acme")],
            ),
        ] {
            assert!(
                line_with(&text, family, &wants).is_some(),
                "{family} missing tenant=acme:\n{text}"
            );
        }
        // The policy latency sibling stays un-tenanted seam health.
        for line in text
            .lines()
            .filter(|l| l.starts_with("agent_policy_authorize_seconds"))
        {
            assert!(
                !line.contains("tenant="),
                "policy latency leaked a tenant label: {line}"
            );
        }
    }

    // desc (negative regression): the families the Phase-5 sweep deliberately kept
    // label-less stay so EVEN under a scoped identity — a prune reaper spans tenants, the
    // provider registry is shared (per-tenant CRUD rides the RPC-layer metric), the
    // scheduler counter is driver-health. None may gain a tenant/session/user label.
    #[tokio::test]
    async fn negative_swept_health_families_stay_tenant_less() {
        let m = Metrics::new();
        agent_core::scope(key_with_user("acme"), async {
            m.on_scheduled_run("ok", 0.5);
            m.on_session_gc(3);
            m.on_registry_mutation("put");
            m.set_registry_upstreams(2, 1);
        })
        .await;
        let text = m.encode_text();
        for family in [
            "agent_scheduled_runs_total",
            "agent_scheduled_run_duration_seconds",
            "agent_session_gc_reclaimed_total",
            "agent_registry_mutations_total",
            "agent_registry_upstreams",
        ] {
            for line in text.lines().filter(|l| l.starts_with(family)) {
                assert!(
                    !line.contains("tenant=")
                        && !line.contains("session=")
                        && !line.contains("user="),
                    "kept-health family {family} leaked a tenant label: {line}"
                );
            }
        }
    }

    // --- Phase 1: review-fleet families (tenant + bounded repo) --------------
    //
    // The recorder itself is validation-agnostic (it records the segments verbatim,
    // exactly as `SessionMetrics` does); `safe_segment` validation of a hostile
    // `user`/`repo` is the caller's job at the fleet call sites and the gRPC span helper
    // (agent-grpc), so the adversarial *rejection* rows live there. Here the adversarial
    // coverage is the hostile-number clamp on the latency histogram and the LRU cardinality
    // backstop under repo churn.

    #[derive(Clone, Copy, Debug)]
    enum Ev {
        Trigger,
        Review,
        Progress,
        Approval,
        Latency,
        PostFailure,
    }

    fn fire(fm: &FleetMetrics, ev: Ev) {
        match ev {
            Ev::Trigger => fm.on_trigger("poll"),
            Ev::Review => fm.on_review("drafted"),
            Ev::Progress => fm.on_progress("posted", "posted"),
            Ev::Approval => fm.on_approval("posted"),
            Ev::Latency => fm.observe_approval_latency(2.0),
            Ev::PostFailure => fm.on_post_failure("slack"),
        }
    }

    /// Distinct `repo="…"` label values present for `family` in the exposition.
    fn repos_for(text: &str, family: &str) -> std::collections::BTreeSet<String> {
        text.lines()
            .filter(|l| l.starts_with(family))
            .filter_map(|l| {
                let rest = &l[l.find("repo=\"")? + 6..];
                Some(rest[..rest.find('"')?].to_string())
            })
            .collect()
    }

    #[rstest]
    // desc: a trigger ticks agent_fleet_triggers_total → expect the family present, labelled (user,repo).
    #[case::positive_trigger(Ev::Trigger, "agent_fleet_triggers_total")]
    // desc: a review transition ticks agent_fleet_reviews_total → expect (user,repo) labels.
    #[case::positive_review(Ev::Review, "agent_fleet_reviews_total")]
    // desc: a progress beat ticks agent_fleet_progress_total → expect (user,repo) labels.
    #[case::positive_progress(Ev::Progress, "agent_fleet_progress_total")]
    // desc: an approval ticks agent_fleet_approvals_total → expect (user,repo) labels.
    #[case::positive_approval(Ev::Approval, "agent_fleet_approvals_total")]
    // desc: a latency sample ticks the histogram _count series → expect (user,repo) labels.
    #[case::positive_latency(Ev::Latency, "agent_fleet_approval_latency_seconds_count")]
    // desc: a failed post ticks agent_fleet_post_failures_total → expect (user,repo) labels.
    #[case::positive_post_failure(Ev::PostFailure, "agent_fleet_post_failures_total")]
    fn positive_fleet_event_records_with_tenant_and_repo(#[case] ev: Ev, #[case] family: &str) {
        let m = Metrics::new();
        let fm = m.for_fleet("acme", "acme__web");
        fire(&fm, ev);
        let text = m.encode_text();
        let line = text
            .lines()
            .find(|l| l.starts_with(family))
            .unwrap_or_else(|| panic!("missing `{family}` in:\n{text}"));
        assert!(
            line.contains("user=\"acme\""),
            "no tenant label on {family}: {line}"
        );
        assert!(
            line.contains("repo=\"acme__web\""),
            "no repo label on {family}: {line}"
        );
    }

    #[test]
    // desc (corner_labels_have_no_pr): PR is never a metric label — no agent_fleet_* line carries `pr=`.
    fn corner_labels_have_no_pr() {
        let m = Metrics::new();
        let fm = m.for_fleet("acme", "acme__web");
        for ev in [
            Ev::Trigger,
            Ev::Review,
            Ev::Progress,
            Ev::Approval,
            Ev::Latency,
            Ev::PostFailure,
        ] {
            fire(&fm, ev);
        }
        let text = m.encode_text();
        for line in text.lines().filter(|l| l.starts_with("agent_fleet_")) {
            assert!(
                !line.contains("pr=\""),
                "PR leaked into a fleet metric label: {line}"
            );
        }
    }

    #[rstest]
    // desc: under the cap every distinct repo keeps its series → expect all present.
    #[case::positive_under_cap(3, &["r1", "r2", "r3"], &["r1", "r2", "r3"])]
    // desc (boundary_repo_label_lru_capped): a repo past the cap evicts the least-recently-used → oldest gone.
    #[case::boundary_cap_evicts_oldest(2, &["r1", "r2", "r3"], &["r2", "r3"])]
    // desc (corner): re-touching a live repo refreshes it, so a different repo is evicted instead.
    #[case::corner_readmit_keeps_recent(2, &["r1", "r2", "r1", "r3"], &["r1", "r3"])]
    fn boundary_repo_label_lru_capped(
        #[case] cap: usize,
        #[case] seq: &[&str],
        #[case] expected: &[&str],
    ) {
        let m = Metrics::new();
        m.set_fleet_repo_cap(cap);
        for r in seq {
            m.for_fleet("acme", r).on_trigger("poll");
        }
        let got = repos_for(&m.encode_text(), "agent_fleet_triggers_total");
        let want: std::collections::BTreeSet<String> =
            expected.iter().map(|&s| s.to_string()).collect();
        assert_eq!(got, want, "LRU repo set mismatch");
    }

    #[rstest]
    // desc: a finite positive latency is recorded → expect one sample in _count.
    #[case::positive_finite(1.5, 1)]
    // desc (boundary): zero is a valid non-negative latency → recorded.
    #[case::boundary_zero(0.0, 1)]
    // desc (adversarial): NaN is dropped before observe → no sample, no poisoned series.
    #[case::adversarial_nan(f64::NAN, 0)]
    // desc (adversarial): a negative latency (clock skew / hostile span pair) is dropped.
    #[case::adversarial_negative(-1.0, 0)]
    // desc (adversarial): +inf is dropped.
    #[case::adversarial_inf(f64::INFINITY, 0)]
    fn adversarial_hostile_latency_clamped_before_observe(
        #[case] seconds: f64,
        #[case] expect_count: u64,
    ) {
        let m = Metrics::new();
        m.for_fleet("acme", "acme__web")
            .observe_approval_latency(seconds);
        let count = m
            .encode_text()
            .lines()
            .find(|l| l.starts_with("agent_fleet_approval_latency_seconds_count"))
            .and_then(|l| l.rsplit(' ').next())
            .and_then(|v| v.parse::<f64>().ok())
            .map(|f| f as u64)
            .unwrap_or(0);
        assert_eq!(
            count, expect_count,
            "latency sample count mismatch for {seconds}"
        );
    }

    #[test]
    // desc (boundary_retire_removes_fleet_series): retire() clears the pair's fleet series so a
    // torn-down session's repo dimension is reclaimed (fleet analogue of the gauge retire).
    fn boundary_retire_removes_fleet_series() {
        let m = Metrics::new();
        let fm = m.for_fleet("acme", "acme__web");
        fm.on_trigger("poll");
        fm.on_review("drafted");
        fm.observe_approval_latency(1.0);
        assert!(
            m.encode_text().contains("repo=\"acme__web\""),
            "fleet series should exist before retire"
        );
        fm.retire();
        let after = m.encode_text();
        assert!(
            !after
                .lines()
                .any(|l| l.starts_with("agent_fleet_") && l.contains("repo=\"acme__web\"")),
            "fleet series not retired:\n{after}"
        );
    }

    // --- Phase 3: message-transport health families (kind + bounded outcome) -
    //
    // These are seam-health: labelled only by `kind` + a bounded `outcome`/`decision`,
    // never a tenant/repo (the fleet beat's repo rides the `fleet.progress` span). `kind`
    // is the impl's own `&'static str`, so there is no hostile-string label to reject; the
    // adversarial coverage here is the hostile-number clamp on the post-latency histogram.

    #[rstest]
    // desc: a successful post ticks agent_transport_posts_total{kind,outcome=ok} → expect the ok series.
    #[case::positive_ok("slack", "ok", "agent_transport_posts_total", "outcome=\"ok\"")]
    // desc: a rate-limited post is counted with outcome=ratelimited → expect the ratelimited series.
    #[case::corner_ratelimited(
        "slack",
        "ratelimited",
        "agent_transport_posts_total",
        "outcome=\"ratelimited\""
    )]
    // desc: a failed post (no token / http / decode / api) collapses to outcome=error → expect the error series.
    #[case::negative_error("matrix", "error", "agent_transport_posts_total", "outcome=\"error\"")]
    fn positive_transport_post_records_kind_and_outcome(
        #[case] kind: &str,
        #[case] outcome: &str,
        #[case] family: &str,
        #[case] label: &str,
    ) {
        let m = Metrics::new();
        m.record_transport_post(kind, outcome, 0.01);
        let text = m.encode_text();
        let line = text
            .lines()
            .find(|l| l.starts_with(family) && l.contains(label))
            .unwrap_or_else(|| panic!("missing `{family}` `{label}` in:\n{text}"));
        assert!(
            line.contains(&format!("kind=\"{kind}\"")),
            "no kind label on {family}: {line}"
        );
    }

    #[rstest]
    // desc: a post that reached the network is an admit decision → expect the admit series.
    #[case::positive_admit("slack", "admit")]
    // desc: a rate-limiter refusal is a refuse decision → expect the refuse series.
    #[case::corner_refuse("slack", "refuse")]
    fn positive_transport_ratelimit_records_decision(#[case] kind: &str, #[case] decision: &str) {
        let m = Metrics::new();
        m.record_transport_ratelimit(kind, decision);
        let text = m.encode_text();
        assert!(
            text.lines()
                .any(|l| l.starts_with("agent_transport_ratelimit_total")
                    && l.contains(&format!("kind=\"{kind}\""))
                    && l.contains(&format!("decision=\"{decision}\""))),
            "no ratelimit {decision} series:\n{text}"
        );
    }

    #[rstest]
    // desc: a finite positive latency is recorded → expect one sample in _count.
    #[case::positive_finite(0.25, 1)]
    // desc (boundary): zero is a valid non-negative latency → recorded.
    #[case::boundary_zero(0.0, 1)]
    // desc (adversarial): NaN is dropped before observe → no sample, no poisoned series.
    #[case::adversarial_nan(f64::NAN, 1)]
    // desc (adversarial): a negative latency (clock skew) is clamped to 0.0 → still one sample.
    #[case::adversarial_negative(-1.0, 1)]
    // desc (adversarial): +inf is clamped to 0.0 → one non-poisoning sample.
    #[case::adversarial_inf(f64::INFINITY, 1)]
    fn adversarial_transport_post_latency_clamped_before_observe(
        #[case] seconds: f64,
        #[case] expect_count: u64,
    ) {
        // The post counter always ticks; the histogram sample count reflects the clamp
        // (a hostile value is replaced by 0.0, still a valid sample — never NaN/inf into
        // the bucket sums, which would poison the series).
        let m = Metrics::new();
        m.record_transport_post("slack", "ok", seconds);
        let text = m.encode_text();
        let count = text
            .lines()
            .find(|l| l.starts_with("agent_transport_post_seconds_count"))
            .and_then(|l| l.rsplit(' ').next())
            .and_then(|v| v.parse::<f64>().ok())
            .map(|f| f as u64)
            .unwrap_or(0);
        assert_eq!(count, expect_count, "sample count mismatch for {seconds}");
        // A poisoned sum shows as NaN/inf in the exposition; assert the sum stayed finite.
        let sum = text
            .lines()
            .find(|l| l.starts_with("agent_transport_post_seconds_sum"))
            .and_then(|l| l.rsplit(' ').next())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NAN);
        assert!(sum.is_finite(), "post-latency sum was poisoned: {sum}");
    }

    #[test]
    // desc (negative_seam_health_families_stay_label_less): the transport families carry NO
    // tenant/repo label — a shared channel is not per-tenant attributable; per-repo triage
    // rides the span. Only `kind` + bounded `outcome`/`decision` are labels.
    fn negative_transport_families_stay_tenant_repo_less() {
        let m = Metrics::new();
        m.record_transport_post("slack", "ok", 0.01);
        m.record_transport_ratelimit("slack", "admit");
        for line in m
            .encode_text()
            .lines()
            .filter(|l| l.starts_with("agent_transport_"))
        {
            assert!(
                !line.contains("user=") && !line.contains("repo=") && !line.contains("session="),
                "a transport health metric leaked a tenant/repo label: {line}"
            );
        }
    }

    // --- Phase 4: config-plane observability (config-store + auth/authz + rpc) ------
    //
    // Unlike the fleet recorders (validation-agnostic), the config-plane recorders are a
    // funnel: they re-validate the attacker-influenced `tenant`/`collection` labels with
    // `safe_segment` (defense in depth), so the adversarial *rejection* rows live here.

    /// The one exposition line for `family` whose labels all match `wants` (`(k, v)`
    /// pairs), or `None` if absent.
    fn line_with<'a>(text: &'a str, family: &str, wants: &[(&str, &str)]) -> Option<&'a str> {
        text.lines().filter(|l| l.starts_with(family)).find(|l| {
            wants
                .iter()
                .all(|(k, v)| l.contains(&format!("{k}=\"{v}\"")))
        })
    }

    /// Distinct `tenant="…"` label values present for `family` in the exposition.
    fn tenants_for(text: &str, family: &str) -> std::collections::BTreeSet<String> {
        text.lines()
            .filter(|l| l.starts_with(family))
            .filter_map(|l| {
                let rest = &l[l.find("tenant=\"")? + 8..];
                Some(rest[..rest.find('"')?].to_string())
            })
            .collect()
    }

    /// Outcome a `record_config_store_op` call is expected to produce.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum CfgExpect {
        /// The `(collection,op,outcome,tenant)` series is present.
        Ticks,
        /// Nothing is recorded (a hostile label was dropped at the funnel).
        Rejected,
    }

    #[rstest]
    // desc: a well-formed put records the (collection,op,outcome,tenant) series → expect it ticks.
    #[case::positive_put_ticks("transport", "put", "ok", "acme", CfgExpect::Ticks)]
    // desc: a backend error is recorded as outcome=error → expect the error series ticks.
    #[case::negative_error_outcome("fleet", "get", "error", "acme", CfgExpect::Ticks)]
    // desc (corner): the count op records its op label distinctly → expect op="count" series ticks.
    #[case::corner_count_op("role", "count", "ok", "acme", CfgExpect::Ticks)]
    // desc (boundary): an empty tenant is NOT a safe segment → the whole op is dropped.
    #[case::boundary_empty_tenant("transport", "put", "ok", "", CfgExpect::Rejected)]
    // desc (adversarial): a hostile tenant segment (traversal) reaches the funnel → dropped, no series.
    #[case::adversarial_hostile_tenant("transport", "put", "ok", "../../etc", CfgExpect::Rejected)]
    // desc (adversarial): a hostile collection segment (separator) is dropped at the funnel.
    #[case::adversarial_hostile_collection("a/b", "put", "ok", "acme", CfgExpect::Rejected)]
    fn config_store_recorder(
        #[case] collection: &str,
        #[case] op: &str,
        #[case] outcome: &str,
        #[case] tenant: &str,
        #[case] expect: CfgExpect,
    ) {
        let m = Metrics::new();
        m.record_config_store_op(collection, op, outcome, tenant);
        let text = m.encode_text();
        let found = line_with(
            &text,
            "agent_config_store_ops_total",
            &[
                ("collection", collection),
                ("op", op),
                ("outcome", outcome),
                ("tenant", tenant),
            ],
        )
        .is_some();
        match expect {
            CfgExpect::Ticks => assert!(found, "expected a series for {collection}/{op}:\n{text}"),
            CfgExpect::Rejected => {
                assert!(
                    !text.contains("agent_config_store_ops_total{"),
                    "hostile label was recorded:\n{text}"
                );
            }
        }
    }

    #[rstest]
    // desc: a finite positive latency is recorded on the un-tenanted op histogram → one sample.
    #[case::positive_finite("get", 0.5, 1)]
    // desc (boundary): zero is a valid non-negative latency → recorded.
    #[case::boundary_zero("list", 0.0, 1)]
    // desc (adversarial): NaN is clamped to 0.0 before observe → one sample, finite sum.
    #[case::adversarial_nan("get", f64::NAN, 1)]
    // desc (adversarial): a negative latency is clamped to 0.0 → recorded, sum stays finite.
    #[case::adversarial_negative("apply", -3.0, 1)]
    // desc (adversarial): +inf is clamped to 0.0 → recorded, sum stays finite.
    #[case::adversarial_inf("count", f64::INFINITY, 1)]
    fn config_store_latency_clamped(
        #[case] op: &str,
        #[case] seconds: f64,
        #[case] expect_count: u64,
    ) {
        let m = Metrics::new();
        m.record_config_store_latency(op, seconds);
        let text = m.encode_text();
        let count = text
            .lines()
            .find(|l| l.starts_with("agent_config_store_op_seconds_count"))
            .and_then(|l| l.rsplit(' ').next())
            .and_then(|v| v.parse::<f64>().ok())
            .map(|f| f as u64)
            .unwrap_or(0);
        assert_eq!(count, expect_count, "sample count mismatch for {seconds}");
        let sum = text
            .lines()
            .find(|l| l.starts_with("agent_config_store_op_seconds_sum"))
            .and_then(|l| l.rsplit(' ').next())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::NAN);
        assert!(sum.is_finite(), "op-latency sum was poisoned: {sum}");
    }

    #[rstest]
    // desc: under the cap every distinct tenant keeps its config-plane series → all present.
    #[case::positive_under_cap(3, &["t1", "t2", "t3"], &["t1", "t2", "t3"])]
    // desc (boundary_tenant_lru_capped): a tenant past the cap evicts the least-recently-used.
    #[case::boundary_cap_evicts_oldest(2, &["t1", "t2", "t3"], &["t2", "t3"])]
    // desc (corner): re-touching a live tenant refreshes it, so a different one is evicted.
    #[case::corner_readmit_keeps_recent(2, &["t1", "t2", "t1", "t3"], &["t1", "t3"])]
    fn boundary_tenant_lru_capped(
        #[case] cap: usize,
        #[case] seq: &[&str],
        #[case] expected: &[&str],
    ) {
        let m = Metrics::new();
        m.set_config_plane_tenant_cap(cap);
        for t in seq {
            m.record_config_store_op("transport", "put", "ok", t);
        }
        let got = tenants_for(&m.encode_text(), "agent_config_store_ops_total");
        let want: std::collections::BTreeSet<String> =
            expected.iter().map(|&s| s.to_string()).collect();
        assert_eq!(got, want, "LRU tenant set mismatch");
    }

    #[test]
    // desc: an evicted tenant loses series across BOTH tenant-labelled families (open-ended
    // discriminators are removed precisely from the remembered tuples, not enumerated).
    fn boundary_tenant_eviction_clears_both_families() {
        let m = Metrics::new();
        m.set_config_plane_tenant_cap(1);
        // t1 records into both config-store and rpc families…
        m.record_config_store_op("transport", "put", "ok", "t1");
        m.record_grpc_rpc("/pkg.Svc/M", "ok", "t1", 0.01);
        assert_eq!(
            tenants_for(&m.encode_text(), "agent_config_store_ops_total"),
            ["t1".to_string()].into()
        );
        // …then t2 overflows the cap and evicts t1 from both.
        m.record_config_store_op("transport", "put", "ok", "t2");
        let text = m.encode_text();
        assert!(
            !text.contains("tenant=\"t1\""),
            "evicted tenant t1 left a stale series:\n{text}"
        );
    }

    #[rstest]
    // desc: an OK request records {rpc,outcome=ok,tenant} and its latency → both tick.
    #[case::positive_ok("/pkg.Svc/M", "ok", "acme", true)]
    // desc (negative): a non-ok grpc status maps to the outcome label → series present.
    #[case::negative_non_ok("/pkg.Svc/M", "permission_denied", "acme", true)]
    // desc (corner): an unauthenticated request has an empty tenant — still a valid bounded label.
    #[case::corner_empty_tenant("/pkg.Svc/M", "ok", "", true)]
    // desc (adversarial): a hostile (spoofed) tenant segment is dropped to empty, never recorded verbatim.
    #[case::adversarial_hostile_tenant("/pkg.Svc/M", "ok", "../../etc", false)]
    fn grpc_rpc_recorder(
        #[case] rpc: &str,
        #[case] outcome: &str,
        #[case] tenant: &str,
        #[case] tenant_kept: bool,
    ) {
        let m = Metrics::new();
        m.record_grpc_rpc(rpc, outcome, tenant, 0.02);
        let text = m.encode_text();
        // The counter always ticks (rpc+outcome are trusted); only the tenant label differs.
        assert!(
            line_with(
                &text,
                "agent_grpc_server_rpc_total",
                &[("rpc", rpc), ("outcome", outcome)]
            )
            .is_some(),
            "no rpc series:\n{text}"
        );
        let recorded_tenant = if tenant_kept { tenant } else { "" };
        assert!(
            line_with(
                &text,
                "agent_grpc_server_rpc_total",
                &[("tenant", recorded_tenant)]
            )
            .is_some(),
            "expected tenant={recorded_tenant:?}:\n{text}"
        );
        // The latency histogram is rpc-only (un-tenanted seam health).
        for line in text
            .lines()
            .filter(|l| l.starts_with("agent_grpc_server_rpc_seconds"))
        {
            assert!(
                !line.contains("tenant="),
                "rpc latency leaked a tenant label: {line}"
            );
        }
    }

    #[test]
    // adversarial: the `rpc` label is attacker-controllable — a client can spray junk
    // paths that still reach the tower layer before tonic routes them to Unimplemented —
    // so it is bounded by a high-water cap: once full, an unknown path collapses to the
    // `other` sentinel instead of growing the dimension without limit. A known path keeps
    // recording as itself.
    fn adversarial_rpc_label_bounded_to_other() {
        let m = Metrics::new();
        m.set_rpc_label_cap(2);
        m.record_grpc_rpc("/pkg.Svc/A", "ok", "acme", 0.01);
        m.record_grpc_rpc("/pkg.Svc/B", "ok", "acme", 0.01);
        // A known path re-records as itself (no new label consumed).
        m.record_grpc_rpc("/pkg.Svc/A", "ok", "acme", 0.01);
        // Distinct paths past the cap overflow → fold to `other`, both counter + latency.
        m.record_grpc_rpc("/pkg.Svc/C", "ok", "acme", 0.01);
        m.record_grpc_rpc("/pkg.Svc/D", "ok", "acme", 0.01);
        let text = m.encode_text();
        for kept in ["/pkg.Svc/A", "/pkg.Svc/B"] {
            assert!(
                line_with(&text, "agent_grpc_server_rpc_total", &[("rpc", kept)]).is_some(),
                "known path {kept} kept:\n{text}"
            );
        }
        assert!(
            line_with(&text, "agent_grpc_server_rpc_total", &[("rpc", "other")]).is_some(),
            "overflow folded to `other`:\n{text}"
        );
        for overflow in ["/pkg.Svc/C", "/pkg.Svc/D"] {
            assert!(
                !text.contains(&format!("rpc=\"{overflow}\"")),
                "overflow path {overflow} must not become its own label (counter or latency):\n{text}"
            );
        }
    }

    #[rstest]
    // desc: an allow decision ticks {action,resource_type,decision=allow}.
    #[case::positive_allow("write", "registry", "allow")]
    // desc (negative): a deny decision ticks decision=deny.
    #[case::negative_deny("delete", "config", "deny")]
    // desc (corner): the approve action maps to its own bounded label.
    #[case::corner_approve("approve", "fleet", "allow")]
    fn authz_and_verify_recorders(
        #[case] action: &str,
        #[case] resource_type: &str,
        #[case] decision: &str,
    ) {
        let m = Metrics::new();
        m.record_authz_decision(action, resource_type, decision);
        m.record_auth_verify(if decision == "allow" { "ok" } else { "error" });
        let text = m.encode_text();
        assert!(
            line_with(
                &text,
                "agent_authz_decisions_total",
                &[
                    ("action", action),
                    ("resource_type", resource_type),
                    ("decision", decision)
                ]
            )
            .is_some(),
            "no authz series:\n{text}"
        );
        // Neither security counter carries a tenant label (it rides the grpc.server span).
        for fam in ["agent_authz_decisions_total", "agent_auth_verify_total"] {
            for line in text.lines().filter(|l| l.starts_with(fam)) {
                assert!(
                    !line.contains("tenant="),
                    "{fam} leaked a tenant label: {line}"
                );
            }
        }
    }

    #[test]
    // desc (negative_seam_health_families_stay_label_less): the config-plane latency histograms
    // are op/rpc-only — no tenant/collection label ever appears on them.
    fn negative_config_plane_latency_stays_tenant_less() {
        let m = Metrics::new();
        m.record_config_store_latency("get", 0.01);
        m.record_grpc_rpc("/pkg.Svc/M", "ok", "acme", 0.01);
        let text = m.encode_text();
        for fam in [
            "agent_config_store_op_seconds",
            "agent_grpc_server_rpc_seconds",
        ] {
            for line in text.lines().filter(|l| l.starts_with(fam)) {
                assert!(
                    !line.contains("tenant=") && !line.contains("collection="),
                    "{fam} leaked a high-cardinality label: {line}"
                );
            }
        }
    }

    #[test]
    // desc (corner_labels_have_no_pr): PR is never a config-plane metric label.
    fn corner_config_plane_labels_have_no_pr() {
        let m = Metrics::new();
        m.record_config_store_op("transport", "put", "ok", "acme");
        m.record_grpc_rpc("/pkg.Svc/M", "ok", "acme", 0.01);
        m.record_authz_decision("write", "registry", "allow");
        for line in m.encode_text().lines().filter(|l| {
            l.starts_with("agent_config_store_")
                || l.starts_with("agent_grpc_server_")
                || l.starts_with("agent_authz_")
        }) {
            assert!(
                !line.contains("pr=\""),
                "PR leaked into a config-plane label: {line}"
            );
        }
    }
}
