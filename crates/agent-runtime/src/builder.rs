//! Turn a `Config` into a wired `Agent` via the plugin [`Registry`].
//!
//! The registry (`registry.rs`) owns the config-string → factory maps; this
//! module drives it: resolve each seam by its config string, wrap memory with
//! telemetry when enabled, and assemble `Settings`. It also hosts the built-in
//! factory functions that need more than a one-liner (the OpenAI-compatible
//! provider and the file memory backend).

use crate::agent::{Agent, Settings};
use crate::config::Config;

/// Default overload admission cap when `[grpc] max_in_flight` is absent — high enough
/// not to touch normal use, low enough to shed a runaway burst (docs/design/loadtest).
const DEFAULT_GRPC_MAX_IN_FLIGHT: usize = 1024;
#[cfg(any(feature = "provider-openai-compat", feature = "provider-anthropic"))]
use crate::config::ProviderCfg;
use crate::context_files;
use crate::registry::Registry;
use agent_core::{MemoryStore, ToolRegistry};
use agent_metrics::Metrics;
use agent_telemetry::{CompositeMemory, TelemetryHandle};
use anyhow::Context;
use std::sync::Arc;

/// Build the agent with the feature-gated built-in modules. When `telemetry` is
/// `Some`, the episodic store is wrapped in a `CompositeMemory` that mirrors
/// events into ClickHouse, and `session_id` is stamped on every recorded event.
pub async fn build_agent(
    cfg: Config,
    telemetry: Option<TelemetryHandle>,
    session_id: String,
    metrics: Metrics,
) -> anyhow::Result<Arc<Agent>> {
    let registry = Registry::with_builtins();
    build_agent_with(&registry, cfg, telemetry, session_id, metrics).await
}

/// Build the agent from a caller-supplied [`Registry`]. Out-of-tree binaries use
/// this to register their own provider/tool/memory/etc. factories (see
/// `docs/extending.md`) before wiring the loop — no fork required.
pub async fn build_agent_with(
    registry: &Registry,
    cfg: Config,
    telemetry: Option<TelemetryHandle>,
    session_id: String,
    metrics: Metrics,
) -> anyhow::Result<Arc<Agent>> {
    // Wrap the provider in its metrics decorator up front, so every downstream
    // user (the loop, the summarizing context strategy, distillation) is
    // attributed the same way — including a remote `= "grpc"` client.
    // The factory context. `provider`/`tokenizer` are filled in as they become
    // available — see `FactoryCtx` on why those two are optional.
    let base_ctx = crate::registry::FactoryCtx::new(&cfg, &metrics).with_registry(registry);
    let provider = crate::metered::provider(
        registry
            .build_provider(&cfg.agent.provider, &base_ctx)
            .context("building provider")?,
        metrics.clone(),
        &cfg.agent.provider,
    );
    // Set when the LSP backend is built below; held for serving.
    #[allow(unused_assignments, unused_mut)]
    let mut shared_lsp: Option<Arc<dyn agent_core::LspBackend>> = None;

    // Set when the forge / tasks backends are built below; held for serving.
    #[allow(unused_assignments, unused_mut)]
    let mut shared_forge: Option<Arc<dyn agent_core::Forge>> = None;
    #[allow(unused_assignments, unused_mut)]
    let mut shared_tasks: Option<Arc<dyn agent_core::TaskTracker>> = None;

    // Set when the sandbox / pty backends are built below; held for serving.
    #[allow(unused_assignments, unused_mut)]
    let mut shared_sandbox: Option<Arc<dyn agent_core::Sandbox>> = None;
    #[allow(unused_assignments, unused_mut)]
    let mut shared_pty: Option<Arc<dyn agent_core::Pty>> = None;

    // Set when the web_search dispatch is composed below; held for serving.
    #[allow(unused_assignments, unused_mut)]
    let mut shared_web_search: Option<Arc<dyn agent_core::WebSearch>> = None;

    // The web backend is built ONCE here and shared by every consumer — the
    // `web_fetch` tool, the `@url` reference route, and `agent --serve-web`. It
    // used to be constructed separately per consumer, so the SSRF screen was
    // configured (identically) two or three times over.
    #[cfg(feature = "web")]
    let shared_web: Option<Arc<dyn agent_core::WebBackend>> = {
        let backend: Arc<dyn agent_core::WebBackend> = match cfg.web.backend.as_str() {
            "local" => Arc::new(
                agent_web::LocalWebBackend::new()
                    .with_ssrf(cfg.web.allow_private, cfg.web.allow_hosts.clone()),
            ),
            // A remote egress host: every outbound request leaves from one
            // process, so the SSRF screen is a property of network position
            // rather than of each agent's config.
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.web.endpoint,
                    agent_grpc::constants::WEB,
                );
                Arc::new(agent_grpc::client::GrpcWeb::connect(&ep)?)
            }
            other => anyhow::bail!("unknown [web] backend `{other}` (built in: `local`, `grpc`)"),
        };
        Some(crate::metered::web(backend, metrics.clone()))
    };
    #[cfg(not(feature = "web"))]
    let shared_web: Option<Arc<dyn agent_core::WebBackend>> = None;

    // The content scanner (parity spec 18) is built ONCE here and shared by every
    // consumer — the `skill_write` and `session_export` tools, the policy guard,
    // and `agent --serve-scanner`. It used to be built per consumer, which made
    // three independent `DispatchScanner`s with identical rules.
    #[cfg(feature = "scanner")]
    let scanner_pair = build_scanner(&cfg, &metrics)?;
    #[cfg(not(feature = "scanner"))]
    let scanner_pair: Option<(Arc<dyn agent_core::Scanner>, agent_core::Severity)> = None;
    let shared_scanner = scanner_pair.as_ref().map(|(s, _)| s.clone());
    #[allow(unused_mut)]
    let mut tools = build_tools(registry, &cfg, &metrics)?;
    #[cfg(feature = "mcp")]
    register_mcp_tools(&mut tools, &cfg, registry, &metrics).await;
    #[cfg(feature = "grpc")]
    register_grpc_tools(&mut tools, &cfg, &metrics).await;

    // `bash` routes through the config-selected Sandbox backend (local unconfined
    // spawn, or the `nix` pinned-closure backend), metered per-backend. Wired here
    // (not a registry factory) so it gets the backend + metrics.
    #[cfg(feature = "tool-core")]
    {
        let backend: Arc<dyn agent_core::Sandbox> = match cfg.sandbox.backend.as_str() {
            "local" => Arc::new(agent_sandbox::LocalSandbox),
            "nix" => {
                let flake = if cfg.agent.working_dir.is_empty() {
                    ".".to_string()
                } else {
                    cfg.agent.working_dir.clone()
                };
                Arc::new(agent_sandbox::NixSandbox::new(flake))
            }
            // A remote executor: run on a host built for it (the toolchain, or
            // deliberate isolation) while this process stays thin.
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.sandbox.endpoint,
                    agent_grpc::constants::SANDBOX,
                );
                let mut c = agent_grpc::client::GrpcSandbox::connect(&ep)?;
                // Learn the remote's real isolation up front, so the runtime
                // picks or degrades on facts rather than on a placeholder.
                c.probe().await?;
                Arc::new(c)
            }
            other => anyhow::bail!("unknown [sandbox] backend `{other}` (local|nix|grpc)"),
        };
        let backend = crate::metered::sandbox(backend, metrics.clone());
        shared_sandbox = Some(backend.clone());
        let tool = Arc::new(agent_tools::BashTool::new(backend));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // The `metrics` tool reads the shared registry, so the agent can inspect its
    // own performance (see docs/observability.md). Registered before the subagent
    // set is captured so child agents inherit it.
    #[cfg(feature = "tool-metrics")]
    {
        let tool = Arc::new(agent_tools::MetricsTool::new(metrics.clone()));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // Search: compose the configured backends into one metered `DispatchSearch`,
    // expose it to the model as the `search` tool (registered before the subagent
    // set is captured, so child agents inherit it), and keep the handle to host
    // over gRPC / kick off the background freshness check below.
    #[cfg(feature = "search")]
    let search_dispatch = {
        let dispatch = crate::search::build_search(registry, &cfg, &metrics)?;
        let backend = dispatch.clone() as Arc<dyn agent_core::SearchBackend>;
        let tool = Arc::new(agent_tools::SearchTool::new(backend.clone()));
        tools.register(crate::metered::tool(tool, metrics.clone()));
        // `index_ls`: list files straight from the index (fast; no FS walk).
        let ls = Arc::new(agent_tools::IndexLsTool::new(backend));
        tools.register(crate::metered::tool(ls, metrics.clone()));
        dispatch
    };

    // Cross-session recall (parity spec 20): a tantivy index over the agent's own
    // past session transcripts, exposed as the `session_recall` tool. Opt-in — a
    // fresh checkout has no history worth searching. The backend is metered under
    // the `recall` label (reusing the search families) and its startup freshness
    // reindex is kicked off below. Registered before the subagent set is captured.
    #[cfg(feature = "recall")]
    if cfg.recall.enabled {
        let backend = crate::recall::build_recall_backend(&cfg.recall, &cfg.agent.working_dir)
            .context("building cross-session recall backend")?;
        let backend = crate::metered::search(backend, metrics.clone(), "recall");
        let tool = Arc::new(agent_tools::SessionRecallTool::new(backend.clone()));
        tools.register(crate::metered::tool(tool, metrics.clone()));
        if cfg.recall.auto_index {
            crate::recall::spawn_freshness(backend);
        }
    }

    // Git: build the configured RepoBackend (scoped to this session's run dir) and
    // expose the multi-branch git tools (git_read/git_diff/git_worktree/…).
    // Registered before the subagent set is captured so child agents inherit them;
    // the handle drives the optional background mirror-fetch below.
    #[cfg(feature = "git")]
    let repo_backend = {
        let backend = crate::git::build_repo(registry, &cfg, &session_id, &metrics)
            .context("building git backend")?;
        let backend = crate::metered::repo(backend, metrics.clone(), cfg.git.backend_name());
        for tool in agent_tools::git_tools(backend.clone()) {
            tools.register(crate::metered::tool(tool, metrics.clone()));
        }
        backend
    };

    // Web: build the WebBackend (local reqwest transport), meter it (per-fetch
    // span + metrics), and expose the `web_fetch` tool. The SSRF/private-IP screen
    // is applied by the Policy guard (`[web] allow_private`), not the tool.
    #[cfg(feature = "web")]
    {
        let backend = shared_web
            .clone()
            .expect("the web backend is built above when the feature is on");
        let default_timeout = cfg
            .web
            .timeout_secs
            .clamp(1, cfg.web.max_timeout_secs.max(1));
        let tool = Arc::new(agent_tools::WebFetchTool::new(
            backend,
            cfg.web.max_bytes,
            default_timeout,
            cfg.web.max_timeout_secs.max(1),
            cfg.web.max_redirects,
        ));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // PTY (parity spec 29): interactive terminal sessions. A live tty held
    // across turns is strictly more powerful than one-shot `bash`, so it is off
    // unless configured, and every call still passes the Policy gate.
    #[cfg(feature = "pty")]
    if cfg.pty.enabled {
        let backend: Arc<dyn agent_core::Pty> = match cfg.pty.backend.as_str() {
            "local" => Arc::new(agent_pty::LocalPty::new().with_max_sessions(cfg.pty.max_sessions)),
            // A remote terminal host: the tty lives there, this process only
            // drives it.
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.pty.endpoint,
                    agent_grpc::constants::PTY,
                );
                Arc::new(agent_grpc::client::GrpcPty::connect(&ep)?)
            }
            other => anyhow::bail!("unknown [pty] backend `{other}` (local|grpc)"),
        };
        let backend = crate::metered::pty(backend, metrics.clone());
        shared_pty = Some(backend.clone());
        let tool = Arc::new(agent_tools::PtyTool::new(backend));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // Scheduler (parity spec 28): recurring unattended runs. The `schedule` tool
    // registers/inspects jobs; they only FIRE while a driver ticks
    // (`agent --scheduler`), so enabling this alone cannot start work silently.
    #[cfg(feature = "scheduler")]
    let scheduler = if cfg.scheduler.enabled {
        let m = metrics.clone();
        let s = Arc::new(
            agent_scheduler::LocalScheduler::new()
                .with_max_jobs(cfg.scheduler.max_jobs)
                .with_claim_ttl_ms(cfg.scheduler.claim_ttl_secs.saturating_mul(1_000))
                .with_observer(Arc::new(move |run: &agent_core::Run| {
                    m.on_scheduled_run(
                        run.outcome.as_str(),
                        run.finished_ms.saturating_sub(run.started_ms) as f64 / 1000.0,
                    );
                    tracing::info!(
                        job = %run.job_id,
                        outcome = run.outcome.as_str(),
                        "scheduled run finished"
                    );
                })),
        );
        let tool = Arc::new(agent_tools::ScheduleTool::new(
            s.clone() as Arc<dyn agent_core::Scheduler>
        ));
        tools.register(crate::metered::tool(tool, metrics.clone()));
        Some(s)
    } else {
        None
    };

    // Forge (parity spec 27): the remote platform. Local git stays with the
    // RepoBackend; this is only the PR/issue/review API. Writes are Policy-gated
    // like any side-effecting tool and default to dry-run.
    #[cfg(feature = "forge")]
    if !cfg.forge.backend.is_empty() {
        let fctx = crate::registry::FactoryCtx::new(&cfg, &metrics);
        let backend = registry
            .build_forge(&cfg.forge.backend, &fctx)
            .with_context(|| format!("building forge backend `{}`", cfg.forge.backend))?;
        let backend = crate::metered::forge(backend, metrics.clone());
        shared_forge = Some(backend.clone());
        if cfg.forge.dry_run {
            tracing::info!("forge is in dry-run: writes will be previewed, not sent");
        }
        let tool = Arc::new(agent_tools::ForgeTool::new(backend, cfg.forge.dry_run));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // Skill authoring (parity spec 30): the agent captures a reusable procedure
    // as a SKILL.md the discovery path picks up next run. OFF by default — a
    // skill is read back into future prompts, so authoring is privileged.
    #[cfg(feature = "skill-write")]
    if cfg.skills.write {
        let root = if cfg.skills.write_dir.is_empty() {
            std::path::PathBuf::from(&cfg.agent.working_dir).join(".agent/skills")
        } else {
            std::path::PathBuf::from(&cfg.skills.write_dir)
        };
        let mut tool = agent_tools::SkillWriteTool::new(root);
        // Prefer the Scanner seam for the injection check when one is wired.
        #[cfg(feature = "scanner")]
        if let Some(s) = shared_scanner.clone() {
            tool = tool.with_scanner(s);
        }
        tools.register(crate::metered::tool(Arc::new(tool), metrics.clone()));
    }

    // Session export: the `session_export` tool renders a saved transcript to a
    // shareable artifact, redacting secrets through the Scanner when one is
    // wired (parity spec 20).
    #[cfg(feature = "session-export")]
    {
        let mut tool = agent_tools::SessionExportTool::new(crate::session_store::dir_for(
            &cfg.agent.working_dir,
        ));
        #[cfg(feature = "scanner")]
        if let Some(s) = shared_scanner.clone() {
            tool = tool.with_scanner(s);
        }
        tools.register(crate::metered::tool(Arc::new(tool), metrics.clone()));
    }

    // Web search: compose the configured backends into one metered, caching
    // `DispatchWebSearch` and expose the `web_search` tool. Each backend comes
    // from an ordinary registry factory.
    #[cfg(feature = "web-search")]
    if !cfg.web_search.backends.is_empty() {
        let ws_ctx = crate::registry::FactoryCtx::new(&cfg, &metrics);
        let mut backends: Vec<(String, Arc<dyn agent_core::WebSearch>)> = Vec::new();
        for name in &cfg.web_search.backends {
            let backend = registry
                .build_web_search(name, &ws_ctx)
                .with_context(|| format!("building web-search backend `{name}`"))?;
            backends.push((
                name.clone(),
                crate::metered::web_search(backend, metrics.clone(), name),
            ));
        }
        let dispatch = agent_web_search::DispatchWebSearch::new(
            backends,
            cfg.web_search.cache_ttl_secs.saturating_mul(1_000),
            cfg.web_search.cache_max_entries,
        )?;
        let dispatch = Arc::new(dispatch) as Arc<dyn agent_core::WebSearch>;
        // Held so `agent --serve-web-search` can host the composed dispatch
        // (cache + fusion included), not just one backend.
        shared_web_search = Some(dispatch.clone());
        let tool = Arc::new(agent_tools::WebSearchTool::new(
            dispatch,
            cfg.web_search.default_limit,
        ));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // Tasks: build the TaskTracker (in-memory plan), meter it (plan-progress
    // gauges + `tasks.*` spans), and expose the `todo_write` tool.
    #[cfg(feature = "tasks")]
    {
        let tracker: Arc<dyn agent_core::TaskTracker> = match cfg.tasks.backend.as_str() {
            "memory" => Arc::new(agent_tasks::MemoryTaskTracker::new()),
            // A shared plan: several agents against one task list.
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.tasks.endpoint,
                    agent_grpc::constants::TASKS,
                );
                Arc::new(agent_grpc::client::GrpcTasks::connect(&ep)?)
            }
            other => anyhow::bail!("unknown [tasks] backend `{other}` (memory|grpc)"),
        };
        let tracker = crate::metered::tasks(tracker, metrics.clone());
        shared_tasks = Some(tracker.clone());
        let tool = Arc::new(agent_tools::TodoWriteTool::new(tracker));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // LSP: only when `[lsp] servers` is configured (no daemons otherwise). Build
    // the manager (real stdio servers, pooled per language), meter it, expose the
    // `lsp` tool. The workspace root is the agent's working directory.
    #[cfg(feature = "lsp")]
    if !cfg.lsp.servers.is_empty() {
        let servers = cfg
            .lsp
            .servers
            .iter()
            .map(|s| agent_lsp::ServerConfig {
                language: s.language.clone(),
                command: s.command.clone(),
                extensions: s.extensions.clone(),
            })
            .collect();
        let root = cfg.agent.working_dir.clone();
        let backend: Arc<dyn agent_core::LspBackend> = match cfg.lsp.backend.as_str() {
            "local" => Arc::new(agent_lsp::LspManager::new(
                Arc::new(agent_lsp::StdioFactory),
                root,
                servers,
            )),
            // A shared host: the warm index lives there and survives this
            // process restarting, instead of every agent paying the cold start.
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.lsp.endpoint,
                    agent_grpc::constants::LSP,
                );
                let languages = cfg.lsp.servers.iter().map(|s| s.language.clone()).collect();
                Arc::new(agent_grpc::client::GrpcLsp::connect(&ep, languages)?)
            }
            other => anyhow::bail!("unknown [lsp] backend `{other}` (local|grpc)"),
        };
        let backend = crate::metered::lsp(backend, metrics.clone());
        shared_lsp = Some(backend.clone());
        let tool = Arc::new(agent_tools::LspTool::new(backend));
        tools.register(crate::metered::tool(tool, metrics.clone()));
    }

    // Memory: either the whole-store backend, or — when `[memory] semantic` is
    // set — the episodic layer of that backend composed with an independently
    // chosen `SemanticStore` (e.g. a vector store) via `LayeredMemory`.
    let provider_ctx = crate::registry::FactoryCtx::new(&cfg, &metrics).with_provider(&provider);
    // The layers are held when composed, so `--serve-episodic` /
    // `--serve-semantic` can host them individually. They exist only on the
    // layered path: an unlayered `MemoryStore` is a single facade, and it is NOT
    // an `EpisodicStore` or a `SemanticStore` — the traits have different
    // methods, so there is nothing to serve.
    let mut layer_episodic: Option<Arc<dyn agent_core::EpisodicStore>> = None;
    let mut layer_semantic: Option<Arc<dyn agent_core::SemanticStore>> = None;
    let inner_memory: Arc<dyn MemoryStore> = if cfg.memory.semantic.is_empty() {
        registry
            .build_memory(&cfg.memory.backend, &provider_ctx)
            .context("building memory")?
    } else {
        let episodic = registry
            .build_episodic(&cfg.memory.backend, &provider_ctx)
            .context("building episodic layer")?;
        let semantic = registry
            .build_semantic(&cfg.memory.semantic, &provider_ctx)
            .context("building semantic layer")?;
        layer_episodic = Some(episodic.clone());
        layer_semantic = Some(semantic.clone());
        Arc::new(agent_core::LayeredMemory::new(episodic, semantic))
    };
    let composed_memory: Arc<dyn MemoryStore> = match telemetry {
        Some(handle) => Arc::new(CompositeMemory::new(inner_memory, handle)),
        None => inner_memory,
    };
    // Metrics wrapper outermost, so it times the whole memory op the loop sees
    // (including the telemetry mirror, when enabled).
    let memory = crate::metered::memory(composed_memory, metrics.clone());

    // The tokenizer the context strategy budgets with — built (and metered, so its
    // `tokenizer.count` spans emit) here, then injected like the provider. `None`
    // when the feature is off or the backend is unknown → heuristic fallback.
    #[cfg(feature = "tokenizer")]
    let tokenizer: Option<Arc<dyn agent_core::Tokenizer>> = registry
        .build_tokenizer(&cfg.tokenizer.backend, &provider_ctx)
        .ok()
        .map(crate::metered::tokenizer);
    #[cfg(not(feature = "tokenizer"))]
    let tokenizer: Option<Arc<dyn agent_core::Tokenizer>> = None;

    // The embedder is built ONCE here — a real one loads a model, so the vector
    // index and `agent --serve-embed` must share an instance rather than each
    // constructing their own. A remote embedder additionally VERIFIES that the
    // server's dimensionality matches `[embedder] dimensions`: a mismatch would
    // silently write wrong-width vectors into the index and corrupt recall.
    #[cfg(feature = "semantic-search")]
    let embedder: Option<Arc<dyn agent_core::Embedder>> = {
        let built = crate::search::build_embedder(&cfg)?;
        #[cfg(feature = "grpc")]
        if cfg.embedder.backend == "grpc" {
            let ep = crate::registry::grpc_client_endpoint(
                &cfg.grpc.embed.endpoint,
                agent_grpc::constants::EMBED,
            );
            let mut probe = agent_grpc::client::GrpcEmbed::connect(&ep, cfg.embedder.dimensions)?;
            probe.verify_dimensions().await?;
        }
        Some(crate::metered::embedder(
            built,
            metrics.clone(),
            &cfg.embedder.backend,
        ))
    };
    #[cfg(not(feature = "semantic-search"))]
    let embedder: Option<Arc<dyn agent_core::Embedder>> = None;

    // The digest ledger (cognition-graph 02), opt-in via `[digest] store`. Built
    // BEFORE the context strategy so `instant-window` can read it via the factory
    // ctx. The `clickhouse` backend reuses the `[telemetry]` connection parameters
    // (one server, two write disciplines — digests are durable, telemetry is lossy).
    #[cfg(feature = "digest")]
    let digest_store: Option<Arc<dyn agent_core::DigestStore>> = match cfg.digest.store.as_str() {
        "" => None,
        "clickhouse" => Some(Arc::new(agent_digest::ClickHouseDigests::new(
            cfg.telemetry.clickhouse_url.clone(),
            cfg.telemetry.database.clone(),
            cfg.telemetry.user.clone(),
            cfg.telemetry.password.clone(),
        ))),
        "sqlite" => Some(Arc::new(agent_digest::SqliteDigests::open(expand_tilde(
            &cfg.digest.path,
        ))?)),
        #[cfg(feature = "grpc")]
        "grpc" => {
            let ep = crate::registry::grpc_client_endpoint(
                &cfg.grpc.digest.endpoint,
                agent_grpc::constants::DIGEST,
            );
            Some(Arc::new(agent_grpc::client::GrpcDigests::connect(&ep)?))
        }
        other => anyhow::bail!("unknown [digest] store `{other}`"),
    };
    #[cfg(not(feature = "digest"))]
    let digest_store: Option<Arc<dyn agent_core::DigestStore>> = None;

    // The cognition-graph document (cognition-graph 04), opt-in via
    // `[graph] store`. The file backend re-validates on every read, so a
    // hand-edited invalid document fails closed here, not at the executor.
    #[cfg(feature = "graph")]
    let graph_store: Option<Arc<dyn agent_core::GraphStore>> = match cfg.graph.store.as_str() {
        "" => None,
        "file" => Some(Arc::new(agent_graph::FileGraphs::new(expand_tilde(
            &cfg.graph.file,
        )))),
        #[cfg(feature = "grpc")]
        "grpc" => {
            let ep = crate::registry::grpc_client_endpoint(
                &cfg.grpc.graph.endpoint,
                agent_grpc::constants::GRAPH,
            );
            Some(Arc::new(agent_grpc::client::GrpcGraphs::connect(&ep)?))
        }
        other => anyhow::bail!("unknown [graph] store `{other}`"),
    };
    #[cfg(not(feature = "graph"))]
    let graph_store: Option<Arc<dyn agent_core::GraphStore>> = None;

    #[allow(unused_mut)]
    let mut full_ctx = crate::registry::FactoryCtx::new(&cfg, &metrics)
        .with_provider(&provider)
        .with_tokenizer(tokenizer.as_ref())
        .with_digests(digest_store.as_ref());
    #[cfg(feature = "semantic-search")]
    {
        full_ctx = full_ctx.with_embedder(embedder.as_ref());
    }
    let full_ctx = full_ctx;
    let context = crate::metered::context(
        registry
            .build_context(&cfg.agent.context, &full_ctx)
            .context("building context strategy")?,
        metrics.clone(),
    );
    // Wrap the selected base policy with the dangerous-command / sensitive-path
    // guard (unless `[policy] guard = "off"`), then meter the composite so metrics
    // see the final decision.
    let base_policy = registry
        .build_policy(&cfg.agent.policy, &full_ctx)
        .context("building policy")?;
    // Tool-call verifier (the `verifier` seam): built here while `full_ctx` (which
    // borrows `cfg`) is still valid; attached to the agent further down. Empty
    // `[verifier] backend` ⇒ off. `mode` captured now, before `cfg` is consumed.
    #[cfg(feature = "verifier")]
    let verifier_enforce = cfg.verifier.mode == "enforce";
    #[cfg(feature = "verifier")]
    let shared_verifier: Option<Arc<dyn agent_core::Verifier>> = if cfg.verifier.backend.is_empty()
    {
        None
    } else {
        Some(
            registry
                .build_verifier(&cfg.verifier.backend, &full_ctx)
                .context("building verifier")?,
        )
    };
    // Content scanner (parity spec 18): compose the configured rules into one
    // metered `DispatchScanner` and hand it to the guard, which maps the worst
    // finding's severity to a `Decision`.
    let scanner = scanner_pair;
    let guarded = crate::policy::guard(
        base_policy,
        crate::policy::GuardMode::parse(&cfg.policy.guard),
        cfg.policy.deny_paths.clone(),
        cfg.policy.allow_paths.clone(),
        cfg.web.allow_private,
        cfg.web.allow_hosts.clone(),
        scanner,
        metrics.clone(),
    );
    let policy = crate::metered::policy(guarded, metrics.clone(), &cfg.agent.policy);

    let (context_prepend, context_append) = context_files::load(&cfg.context_files.dir);
    if !context_prepend.is_empty() || !context_append.is_empty() {
        tracing::info!(
            prepend = context_prepend.len(),
            append = context_append.len(),
            "loaded user context files"
        );
    }

    // LLM pool + review seams (docs/design/code-review/). Built here — after the
    // repo/search/forge seams and while `base_ctx` (which borrows `cfg`) is still
    // usable, i.e. before `settings` moves `cfg.agent.system_prompt`.
    // A `[grpc.llm_pool] endpoint` dials a remote pool; else `[pool] members`
    // builds a local one; else no pool (mode detection degrades to the prefilter).
    #[cfg(all(feature = "provider-pool", feature = "grpc"))]
    let grpc_pool: Option<Arc<dyn agent_core::LlmPool>> = if cfg.grpc.llm_pool.endpoint.is_empty() {
        None
    } else {
        let ep = crate::registry::grpc_client_endpoint(
            &cfg.grpc.llm_pool.endpoint,
            agent_grpc::constants::LLM_POOL,
        );
        Some(Arc::new(agent_grpc::client::GrpcLlmPool::connect(&ep)?))
    };
    #[cfg(all(feature = "provider-pool", not(feature = "grpc")))]
    let grpc_pool: Option<Arc<dyn agent_core::LlmPool>> = None;

    #[cfg(feature = "provider-pool")]
    let llm_pool_seam: Option<Arc<dyn agent_core::LlmPool>> = if grpc_pool.is_some() {
        grpc_pool
    } else if cfg.pool.members.is_empty() {
        None
    } else {
        let mut specs = Vec::new();
        for (i, entry) in cfg.pool.members.iter().enumerate() {
            let name = entry.name();
            if name == "pool" {
                anyhow::bail!("[pool] members must not include `pool` itself");
            }
            // A detailed `[[pool.members]]` block carries per-member routing knobs;
            // a bare name takes its tier from the parallel `tiers` list.
            let detailed = match entry {
                crate::config::PoolMemberEntry::Detailed(c) => Some(c.as_ref()),
                crate::config::PoolMemberEntry::Name(_) => None,
            };
            let tier = detailed
                .filter(|c| !c.tier.is_empty())
                .and_then(|c| agent_core::PoolTier::parse(&c.tier))
                .or_else(|| {
                    cfg.pool
                        .tiers
                        .get(i)
                        .and_then(|t| agent_core::PoolTier::parse(t))
                })
                .unwrap_or(agent_core::PoolTier::Medium);
            let weight = detailed.and_then(|c| c.weight).unwrap_or(1.0);
            let cost = detailed.and_then(|c| c.cost).unwrap_or(0.0);
            let max_concurrency = detailed.and_then(|c| c.max_concurrency).unwrap_or(0);
            // An inline `endpoint` synthesizes an OpenAI-compatible provider (one
            // config block per GPU target); otherwise resolve `name` via the registry.
            let provider: Arc<dyn agent_core::LlmProvider> = match detailed {
                Some(c) if !c.endpoint.is_empty() => {
                    Arc::new(build_inline_pool_member(c, cfg.agent.context_window)?)
                        as Arc<dyn agent_core::LlmProvider>
                }
                _ => registry.build_provider(name, &base_ctx)?,
            };
            specs.push(agent_providers::PoolSpec {
                candidate: agent_providers::Candidate {
                    name: name.to_string(),
                    provider: crate::metered::provider(provider, metrics.clone(), name),
                },
                tier,
                cost,
                weight,
                max_concurrency,
            });
        }
        let m = metrics.clone();
        let pool = agent_providers::PoolProvider::new("pool", specs)?
            .with_breaker(
                cfg.pool.failure_threshold,
                cfg.pool.cooldown_secs.saturating_mul(1_000),
            )
            .with_fanout(cfg.pool.fanout)
            .with_policy(agent_providers::PoolPolicy::parse(&cfg.pool.policy))
            .with_saturation(
                agent_providers::Saturation::parse(&cfg.pool.on_saturation),
                cfg.pool.saturation_wait_ms,
            )
            .with_health_grading(cfg.pool.latency_alpha, cfg.pool.degraded_threshold_ms)
            .with_observer(Arc::new(move |ev| {
                crate::metered::record_pool_event(&m, ev);
            }))
            .with_probe(cfg.pool.probe_interval_secs, cfg.pool.probe_timeout_secs);
        Some(Arc::new(pool) as Arc<dyn agent_core::LlmPool>)
    };
    #[cfg(not(feature = "provider-pool"))]
    let llm_pool_seam: Option<Arc<dyn agent_core::LlmPool>> = None;

    // General task-mode detection (docs/design/adaptive-cognition/01-mode.md).
    // Decoupled from `review`: it runs every turn, and review is one consumer.
    // `hybrid` builds the local classifier; `grpc` dials a remote `ModeService`.
    #[cfg(feature = "mode")]
    let task_classifier_seam: Option<Arc<dyn agent_core::TaskClassifier>> =
        match cfg.mode.classifier.as_str() {
            "hybrid" => Some(Arc::new(agent_mode::HybridClassifier::new(
                llm_pool_seam.clone(),
            ))),
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.mode.endpoint,
                    agent_grpc::constants::MODE,
                );
                Some(Arc::new(agent_grpc::client::GrpcClassifier::connect(&ep)?))
            }
            _ => None,
        };
    #[cfg(not(feature = "mode"))]
    let task_classifier_seam: Option<Arc<dyn agent_core::TaskClassifier>> = None;

    // Dimensional memory (adaptive-cognition 03), opt-in via `[dimensions] store`.
    // `file` writes per-dimension histories under the semantic dir; `grpc` dials a
    // remote `DimensionService`. The summarizer is the main provider (like distill).
    #[cfg(feature = "dimensions")]
    let dimension_store_seam: Option<Arc<dyn agent_core::DimensionStore>> =
        if cfg.dimensions.enabled() {
            match cfg.dimensions.store.as_str() {
                "file" => {
                    // Per-user: each tenant's dimensional histories live under
                    // `<semantic_dir>/<user>/dimensions/` (the `local` user keeps
                    // today's `<semantic_dir>/dimensions/`) — cross-session within a
                    // user, isolated across users
                    // (docs/design/multi-session/04-tenancy.md).
                    Some(Arc::new(agent_memory::PerUserDimensions::new(
                        &cfg.memory.semantic_dir,
                        Some(provider.clone()),
                    )))
                }
                #[cfg(feature = "grpc")]
                "grpc" => {
                    let ep = crate::registry::grpc_client_endpoint(
                        &cfg.grpc.dimension.endpoint,
                        agent_grpc::constants::DIMENSION,
                    );
                    Some(Arc::new(agent_grpc::client::GrpcDimensions::connect(&ep)?))
                }
                other => {
                    tracing::warn!("unknown [dimensions] store `{other}`; disabling");
                    None
                }
            }
        } else {
            None
        };
    #[cfg(not(feature = "dimensions"))]
    let dimension_store_seam: Option<Arc<dyn agent_core::DimensionStore>> = None;

    // Prompt management (docs/design/portal): the `PromptStore` seam over the config
    // system prompt + the context.d/prompts trees. Always built (no `= "grpc"` loop
    // consumer) so it can be hosted via `--serve-prompt` / `--serve-all`. The backend
    // is selected by `[prompts] backend` — the same `match cfg.<x>.backend` template
    // the SessionStore uses (docs/design/prompts/05-storage.md).
    #[cfg(feature = "prompt")]
    let prompt_store_seam: Option<Arc<dyn agent_core::PromptStore>> =
        Some(match cfg.prompts.backend.as_str() {
            "file" => Arc::new(agent_prompt::FilePromptStore::new(
                cfg.context_files.dir.clone(),
                cfg.prompts.dir.clone(),
                cfg.agent.system_prompt.clone(),
            )) as Arc<dyn agent_core::PromptStore>,
            // Embedded SQLite catalog (feature `prompt-sqlite`). Empty `db_path` ⇒
            // `<working_dir>/.agent-seddon/prompts.db` (mirrors the session dir default).
            #[cfg(feature = "prompt-sqlite")]
            "sqlite" => {
                let db = if cfg.prompts.db_path.is_empty() {
                    std::path::PathBuf::from(&cfg.agent.working_dir)
                        .join(".agent-seddon")
                        .join("prompts.db")
                } else {
                    std::path::PathBuf::from(&cfg.prompts.db_path)
                };
                Arc::new(agent_prompt::SqlitePromptStore::open(
                    &db,
                    cfg.agent.system_prompt.clone(),
                )?) as Arc<dyn agent_core::PromptStore>
            }
            // A shared/central catalog dialed over gRPC (the already-shipped client).
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.prompt.endpoint,
                    agent_grpc::constants::PROMPT,
                );
                Arc::new(agent_grpc::client::GrpcPrompts::connect(&ep)?)
                    as Arc<dyn agent_core::PromptStore>
            }
            other => {
                anyhow::bail!(
                    "unknown [prompts] backend `{other}` (built in: `file`, `sqlite`, `grpc`)"
                )
            }
        });
    #[cfg(not(feature = "prompt"))]
    let prompt_store_seam: Option<Arc<dyn agent_core::PromptStore>> = None;

    // Metrics proxy (docs/design/portal): a PromQL→Prometheus proxy so a gRPC-only
    // client reads the same series Grafana does. Always built (no loop consumer) so
    // it can be hosted via `--serve-metrics-proxy` / `--serve-all`. A bad
    // `prometheus_url` is a per-call fail-soft, not a startup failure — but building
    // the HTTP client itself can fail, in which case the seam is simply not offered.
    #[cfg(feature = "metrics-proxy")]
    let metrics_proxy_seam: Option<Arc<dyn agent_core::MetricsProxy>> =
        match agent_metrics_proxy::HttpMetricsProxy::new(&cfg.metrics_proxy.prometheus_url) {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                tracing::warn!("metrics proxy disabled: {e}");
                None
            }
        };
    #[cfg(not(feature = "metrics-proxy"))]
    let metrics_proxy_seam: Option<Arc<dyn agent_core::MetricsProxy>> = None;

    #[cfg(feature = "review")]
    let review_collector_seam: Option<Arc<dyn agent_core::ReviewCollector>> =
        match cfg.review.backend.as_str() {
            "local" => {
                #[cfg(feature = "search")]
                let review_search =
                    Some(search_dispatch.clone() as Arc<dyn agent_core::SearchBackend>);
                #[cfg(not(feature = "search"))]
                let review_search: Option<Arc<dyn agent_core::SearchBackend>> = None;
                #[cfg(feature = "forge")]
                let review_forge = shared_forge.clone();
                #[cfg(not(feature = "forge"))]
                let review_forge: Option<Arc<dyn agent_core::Forge>> = None;
                let m = metrics.clone();
                // Root the collector at the same repo the RepoBackend uses (the git
                // root of the process cwd), so the file set and the diff agree.
                let review_root = crate::git::git_paths(&cfg)?.0;
                let mut orch = agent_review::ReviewOrchestrator::new(
                    review_root,
                    repo_backend.clone(),
                    review_search,
                    review_forge,
                )
                .with_observer(Arc::new(move |ev| {
                    crate::metered::record_review_event(&m, ev);
                }))
                .with_deadline(std::time::Duration::from_secs(cfg.review.deadline_secs));
                // Static analysis runs by default; it shells out to the linters via
                // the shared sandbox (fail-soft without one).
                if cfg.review.analyze {
                    orch =
                        orch.with_analyzer(shared_sandbox.clone(), cfg.review.analyze_timeout_secs);
                }
                // Signature-diff (changed function signatures) runs by default —
                // pure in-process, no external tool.
                if cfg.review.signatures {
                    orch = orch.with_signatures();
                }
                // Call graph (Go blast radius) via the pinned helper — fail-soft.
                if cfg.review.callgraph {
                    orch = orch
                        .with_callgraph(shared_sandbox.clone(), cfg.review.callgraph_timeout_secs);
                }
                // Code-style fingerprint — pure in-process, no external tool.
                if cfg.review.style {
                    orch = orch.with_style(cfg.review.style_commit_sample);
                }
                // Cheap-LLM summaries over the pool — skips fail-soft without one.
                if cfg.review.summaries {
                    orch = orch.with_summaries(llm_pool_seam.clone());
                }
                // Historical co-change — pure git-history mining, no toolchain.
                if cfg.review.cochange {
                    orch = orch.with_cochange(cfg.review.cochange_window);
                }
                // Churn / ownership (bus factor + churn trend) — also git-history.
                if cfg.review.churn {
                    orch = orch.with_churn(cfg.review.churn_window);
                }
                // Risk synthesis threshold (post-fan-out; always computed).
                orch = orch.with_gate_threshold(cfg.review.gate_threshold);
                Some(Arc::new(orch) as Arc<dyn agent_core::ReviewCollector>)
            }
            // A remote fact-collection host (CPU-heavy, worth distributing).
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.review.endpoint,
                    agent_grpc::constants::REVIEW,
                );
                Some(Arc::new(agent_grpc::client::GrpcReview::connect(&ep)?)
                    as Arc<dyn agent_core::ReviewCollector>)
            }
            _ => None,
        };
    #[cfg(not(feature = "review"))]
    let review_collector_seam: Option<Arc<dyn agent_core::ReviewCollector>> = None;

    let settings = Settings {
        max_iterations: cfg.agent.max_iterations,
        max_tokens: cfg.agent.max_tokens,
        temperature: cfg.agent.temperature,
        context_window: cfg.agent.context_window,
        reserve_output: cfg.agent.reserve_output,
        // The effective system prompt is the `<prompts>/system.md` override if
        // present, else the config value (docs/design/portal). A `Put(System)` thus
        // takes effect on the next run.
        #[cfg(feature = "prompt")]
        system_prompt: agent_prompt::resolve_system_prompt(
            &cfg.prompts.dir,
            &cfg.agent.system_prompt,
        ),
        #[cfg(not(feature = "prompt"))]
        system_prompt: cfg.agent.system_prompt.clone(),
        stream: cfg.agent.stream,
        parallel_tools: cfg.agent.parallel_tools,
        tool_timeout_secs: cfg.agent.tool_timeout_secs,
        recall_limit: cfg.memory.recall_limit,
        cwd: if cfg.agent.working_dir.is_empty() {
            std::env::current_dir().context("resolving cwd")?
        } else {
            std::path::PathBuf::from(expand_tilde(&cfg.agent.working_dir))
        },
        model: cfg.provider.model.clone(),
        session_id,
        context_prepend,
        context_append,
        review_in_loop: cfg.review.in_loop,
        review_context_budget: cfg.review.context_budget_bytes,
        mode_confidence_floor: cfg.mode.confidence_floor,
        mode_hysteresis: cfg.mode.hysteresis,
        // Absent ⇒ a protective default cap; `Some(0)` ⇒ explicit opt-out (unbounded).
        grpc_max_in_flight: cfg.grpc.max_in_flight.unwrap_or(DEFAULT_GRPC_MAX_IN_FLIGHT),
    };

    // Subagents: register a `delegate` tool whose children reuse the worker tool
    // set (captured before `delegate` is added, so children can't see it) plus a
    // deeper `delegate` while depth remains.
    #[cfg(feature = "subagents")]
    if cfg.agent.subagents {
        let ctx = Arc::new(crate::subagent::SubagentContext {
            provider: provider.clone(),
            context: context.clone(),
            policy: policy.clone(),
            memory: memory.clone(),
            worker_tools: tools.clone(),
            metrics: metrics.clone(),
            max_depth: cfg.agent.subagent_max_depth.max(1),
            child_settings: settings.clone(),
        });
        tools.register(Arc::new(crate::subagent::DelegateTool::root(ctx)));
    }

    // Kick off the background index-freshness check (unless disabled) before the
    // agent starts its loop; queries serve the last committed snapshot meanwhile.
    #[cfg(feature = "search")]
    if cfg.search.auto_index {
        crate::search::spawn_freshness(search_dispatch.clone(), metrics.clone());
    }

    // Keep the shared git mirror fresh in the background (opt-in via auto_fetch_secs).
    #[cfg(feature = "git")]
    crate::git::spawn_fetch(repo_backend.clone(), cfg.git.auto_fetch_secs);

    let agent = Agent::new(
        provider,
        tools,
        memory,
        context,
        policy,
        metrics.clone(),
        settings,
    );
    #[cfg(feature = "search")]
    let agent = agent.with_search(search_dispatch.clone() as Arc<dyn agent_core::SearchBackend>);
    #[cfg(feature = "git")]
    let agent = agent.with_repo(repo_backend);
    let agent = match llm_pool_seam {
        Some(p) => agent.with_llm_pool(p),
        None => agent,
    };
    let agent = match task_classifier_seam {
        Some(c) => agent.with_task_classifier(c),
        None => agent,
    };
    let agent = match dimension_store_seam {
        Some(d) => agent.with_dimension_store(d),
        None => agent,
    };
    let agent = match &digest_store {
        Some(s) => agent.with_digests(
            s.clone(),
            cfg.digest.summary_max_tokens,
            cfg.digest.facts_max_tokens,
        ),
        None => agent,
    };
    let agent = match &graph_store {
        Some(g) => agent.with_graph(g.clone()),
        None => agent,
    };
    let agent = match prompt_store_seam {
        Some(p) => agent.with_prompt_store(p),
        None => agent,
    };
    // Situational system-prompt fragments (docs/design/prompts/): the loop selects
    // and injects the fragments matching the current mode. Rooted at the `prompts`
    // dir — a missing dir ⇒ a no-op resolver ⇒ byte-identical behaviour.
    let agent = agent.with_system_fragments(agent_context::system_fragments::SystemFragments::new(
        Some(&cfg.prompts.dir),
    ));
    let agent = match metrics_proxy_seam {
        Some(m) => agent.with_metrics_proxy(m),
        None => agent,
    };
    let agent = match review_collector_seam {
        Some(r) => agent.with_review_collector(r),
        None => agent,
    };
    // Hold the shared scanner so `agent --serve-scanner` can host it.
    let agent = match shared_scanner.clone() {
        Some(s) => agent.with_scanner(s),
        None => agent,
    };
    // Attach the verifier built above (before `cfg` was consumed). `enforce`
    // captured before `cfg` is moved.
    #[cfg(feature = "verifier")]
    let agent = match shared_verifier {
        Some(v) => agent.with_verifier(v, verifier_enforce),
        None => agent,
    };
    // …and the tokenizer / embedder, for `--serve-tokenizer` / `--serve-embed`.
    let agent = agent.with_tokenizer_seam(tokenizer.clone());
    let agent = agent
        .with_web(shared_web.clone())
        .with_web_search(shared_web_search.clone())
        .with_sandbox(shared_sandbox.clone())
        .with_pty(shared_pty.clone())
        .with_forge(shared_forge.clone())
        .with_tasks(shared_tasks.clone())
        .with_lsp(shared_lsp.clone())
        .with_memory_layers(layer_episodic.clone(), layer_semantic.clone());
    let agent = agent.with_embedder(embedder.clone());
    // Structured output: build the config-selected validator, meter it, attach it.
    #[cfg(feature = "structured")]
    let agent = {
        let validator = crate::structured::build_validator(&cfg.structured.validator)?;
        agent.with_validator(crate::metered::validator(validator, metrics.clone()))
    };
    // Session history: build the config-selected store, meter it, attach it.
    #[cfg(feature = "session")]
    let agent = {
        let store: Arc<dyn agent_core::SessionStore> = match cfg.session.backend.as_str() {
            "file" => {
                let dir = if cfg.session.dir.is_empty() {
                    std::path::PathBuf::from(&cfg.agent.working_dir)
                        .join(".agent-seddon")
                        .join("session")
                } else {
                    std::path::PathBuf::from(&cfg.session.dir)
                };
                Arc::new(agent_session::FileSessionStore::new(dir))
            }
            // A remote store: several agents sharing one content-addressed
            // history, handing work between them by checkpoint id.
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.session.endpoint,
                    agent_grpc::constants::SESSION,
                );
                Arc::new(agent_grpc::client::GrpcSession::connect(&ep)?)
            }
            other => {
                anyhow::bail!("unknown [session] backend `{other}` (built in: `file`, `grpc`)")
            }
        };
        agent
            .with_session_store(crate::metered::session(store, metrics.clone()))
            .with_auto_checkpoint(cfg.session.auto_checkpoint)
    };
    // `@`-reference expansion: build the config-selected resolver, route it through
    // the Search seam (`@symbol`) and a fresh Web backend (`@url`, reusing the SSRF
    // guard), meter it, attach it with its token budget.
    // Lifecycle hooks (parity spec 22): config-selected, dispatched in the order
    // listed. Empty by default, and every dispatch short-circuits when empty.
    #[cfg(feature = "scheduler")]
    let agent = match scheduler {
        Some(s) => agent.with_scheduler(s),
        None => agent,
    };
    let agent = {
        let hooks = crate::hooks::build(&cfg.hooks.enabled, &metrics)?;
        if hooks.is_empty() {
            agent
        } else {
            tracing::info!(hooks = hooks.len(), "lifecycle hooks enabled");
            agent.with_hooks(hooks)
        }
    };
    #[cfg(feature = "reference")]
    let agent = {
        let resolver: Arc<dyn agent_core::ReferenceResolver> = match cfg.reference.backend.as_str()
        {
            "local" => {
                // `mut` is only needed when a route below is compiled in; without
                // `search`/`web` the resolver is built and used as-is.
                #[allow(unused_mut)]
                let mut r = agent_reference::LocalResolver::new(&cfg.agent.working_dir)
                    .with_max_block_chars(cfg.reference.per_block_max_chars);
                #[cfg(feature = "search")]
                {
                    r = r
                        .with_search(search_dispatch.clone() as Arc<dyn agent_core::SearchBackend>);
                }
                #[cfg(feature = "web")]
                if let Some(web) = shared_web.clone() {
                    r = r.with_web(web);
                }
                Arc::new(r)
            }
            // A remote resolver: the process with the checkout and the index
            // mounted does the reading; this agent keeps only the blocks.
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.reference.endpoint,
                    agent_grpc::constants::REFERENCE,
                );
                Arc::new(agent_grpc::client::GrpcReference::connect(&ep)?)
            }
            other => {
                anyhow::bail!("unknown [reference] backend `{other}` (built in: `local`, `grpc`)")
            }
        };
        agent.with_reference_resolver(
            crate::metered::reference(resolver, metrics.clone()),
            cfg.reference.budget_tokens,
        )
    };
    // Share the built backend behind an `Arc` so many `Session`s can own it
    // concurrently (docs/design/multi-session/02-runtime-split.md).
    Ok(Arc::new(agent))
}

/// Resolve the enabled tools through the registry. Empty `[tools] enabled` means
/// "every registered tool"; otherwise only the named ones (erroring, with the
/// known names listed, on a typo).
fn build_tools(
    registry: &Registry,
    cfg: &Config,
    metrics: &Metrics,
) -> anyhow::Result<ToolRegistry> {
    let mut tools = ToolRegistry::new();
    let ctx = crate::registry::FactoryCtx::new(cfg, metrics);
    if cfg.tools.enabled.is_empty() {
        for name in registry.tool_names() {
            let tool = registry.build_tool(name, &ctx)?;
            tools.register(crate::metered::tool(tool, metrics.clone()));
        }
    } else {
        for name in &cfg.tools.enabled {
            // Tools wired later by the builder (`search`, `metrics`, the `git_*`
            // set, `delegate`) aren't in the registry — naming one is not a typo,
            // it's added unconditionally below. Skip it here; still error on a
            // genuine unknown name.
            if is_builder_registered_tool(name) {
                continue;
            }
            let tool = registry
                .build_tool(name, &ctx)
                .with_context(|| format!("enabling tool `{name}`"))?;
            tools.register(crate::metered::tool(tool, metrics.clone()));
        }
    }
    Ok(tools)
}

/// Names of tools the builder registers directly (not through the [`Registry`]),
/// so `[tools] enabled` can list them without tripping the typo check. They are
/// added unconditionally when their feature is compiled in, so the allowlist only
/// filters the registry-built tools (see `config/agent.toml`).
fn is_builder_registered_tool(name: &str) -> bool {
    (cfg!(feature = "tool-core") && name == "bash")
        || (cfg!(feature = "tool-metrics") && name == "metrics")
        || (cfg!(feature = "search") && (name == "search" || name == "index_ls"))
        || (cfg!(feature = "recall") && name == "session_recall")
        || (cfg!(feature = "subagents") && name == "delegate")
        || (cfg!(feature = "git") && name.starts_with("git_"))
        || (cfg!(feature = "web") && name == "web_fetch")
        || (cfg!(feature = "tasks") && name == "todo_write")
        || (cfg!(feature = "lsp") && name == "lsp")
}

/// Connect to each configured MCP server, discover its tools, and register them
/// (as `mcp_<server>_<tool>`) into the tool registry. Best-effort: a server that
/// fails to start or handshake is logged and skipped, never aborting the run.
/// MCP tools are always added when their server is configured — the `[tools]
/// enabled` allowlist only filters the built-ins.
#[cfg(feature = "mcp")]
async fn register_mcp_tools(
    tools: &mut ToolRegistry,
    cfg: &Config,
    registry: &Registry,
    metrics: &Metrics,
) {
    for s in &cfg.mcp.servers {
        // A custom `kind` selects an out-of-tree transport registered on the
        // registry; the whole server config rides along as `params`. Otherwise the
        // kind is inferred: `command` → stdio, `url` → http.
        let is_custom_kind = !s.kind.is_empty() && s.kind != "stdio" && s.kind != "http";
        let transport = if is_custom_kind {
            agent_mcp::Transport::Other {
                kind: s.kind.clone(),
                params: serde_json::json!({
                    "command": s.command,
                    "args": s.args,
                    "env": s.env,
                    "url": s.url,
                    "headers": s.headers,
                }),
            }
        } else if !s.command.is_empty() {
            agent_mcp::Transport::Stdio {
                command: s.command.clone(),
                args: s.args.clone(),
                env: s.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            }
        } else if !s.url.is_empty() {
            agent_mcp::Transport::Http {
                url: s.url.clone(),
                headers: s
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            }
        } else {
            tracing::warn!(
                "mcp server `{}` has neither command nor url; skipping",
                s.name
            );
            continue;
        };
        let server = agent_mcp::ServerConfig {
            name: s.name.clone(),
            transport,
        };
        match agent_mcp::connect_tools(registry.transports(), &server).await {
            Ok(mcp_tools) => {
                let n = mcp_tools.len();
                for tool in mcp_tools {
                    tools.register(crate::metered::tool(tool, metrics.clone()));
                }
                tracing::info!("mcp server `{}`: registered {n} tool(s)", s.name);
            }
            Err(e) => tracing::warn!("mcp server `{}` unavailable: {e}", s.name),
        }
    }
}

/// Discover and register a remote gRPC tool worker's tools (mirrors
/// `register_mcp_tools`). Only acts when `[grpc.tools] endpoint` is set — there is
/// no implicit default worker. Best-effort: a failing worker is logged and skipped.
#[cfg(feature = "grpc")]
async fn register_grpc_tools(tools: &mut ToolRegistry, cfg: &Config, metrics: &Metrics) {
    let endpoint = &cfg.grpc.tools.endpoint;
    if endpoint.is_empty() {
        return;
    }
    let ep = agent_grpc::Endpoint::parse(endpoint);
    match agent_grpc::client::grpc_tools(&ep).await {
        Ok(remote) => {
            let n = remote.len();
            for tool in remote {
                tools.register(crate::metered::tool(tool, metrics.clone()));
            }
            tracing::info!("grpc tool worker `{endpoint}`: registered {n} tool(s)");
        }
        Err(e) => tracing::warn!("grpc tool worker `{endpoint}` unavailable: {e}"),
    }
}

// --- built-in factory functions (referenced from `register_builtins`) ------

/// Factory for the OpenAI-compatible provider.
#[cfg(feature = "provider-openai-compat")]
pub(crate) fn openai_compat_provider(
    ctx: &crate::registry::FactoryCtx<'_>,
) -> anyhow::Result<Arc<dyn agent_core::LlmProvider>> {
    let cfg = ctx.cfg;
    use agent_providers::{OpenAiCompatConfig, OpenAiCompatProvider};
    if cfg.provider.insecure_tls {
        tracing::warn!(
            "provider.insecure_tls=true: TLS certificate validation is DISABLED \
             (needed for the self-signed GLM dev server). This exposes the API key \
             and traffic to man-in-the-middle attacks — do not use over untrusted networks."
        );
    }
    let api_key = resolve_api_key(&cfg.provider)?;
    let provider = OpenAiCompatProvider::new(OpenAiCompatConfig {
        base_url: cfg.provider.base_url.clone(),
        model: cfg.provider.model.clone(),
        api_key,
        insecure_tls: cfg.provider.insecure_tls,
        context_window: cfg.agent.context_window,
        max_retries: cfg.provider.max_retries,
        supports_vision: cfg.provider.supports_vision,
    })
    .map_err(|e| anyhow::anyhow!("building provider: {e}"))?;
    #[cfg(feature = "cache")]
    let provider = match build_cache_strategy(ctx)? {
        Some(s) => provider.with_cache_strategy(s),
        None => provider,
    };
    Ok(Arc::new(provider))
}

/// Factory for the Anthropic-native provider.
#[cfg(feature = "provider-anthropic")]
pub(crate) fn anthropic_provider(
    ctx: &crate::registry::FactoryCtx<'_>,
) -> anyhow::Result<Arc<dyn agent_core::LlmProvider>> {
    let cfg = ctx.cfg;
    use agent_providers::{AnthropicConfig, AnthropicProvider};
    let api_key = resolve_api_key(&cfg.provider)?;
    let base_url = if cfg.provider.base_url.is_empty() {
        "https://api.anthropic.com/v1".to_string()
    } else {
        cfg.provider.base_url.clone()
    };
    let provider = AnthropicProvider::new(AnthropicConfig {
        base_url,
        model: cfg.provider.model.clone(),
        api_key,
        version: cfg.provider.version.clone(),
        context_window: cfg.agent.context_window,
        max_retries: cfg.provider.max_retries,
    })
    .map_err(|e| anyhow::anyhow!("building provider: {e}"))?;
    #[cfg(feature = "cache")]
    let provider = match build_cache_strategy(ctx)? {
        Some(s) => provider.with_cache_strategy(s),
        None => provider,
    };
    Ok(Arc::new(provider))
}

/// Factory for the file-backed whole memory store: the file episodic log paired
/// with the file semantic store, composed via `LayeredMemory`. Distillation is
/// wired only when `[memory] distill = true` (it costs a model call per run).
#[cfg(feature = "memory-file")]
pub(crate) fn file_memory(
    ctx: &crate::registry::FactoryCtx<'_>,
) -> anyhow::Result<Arc<dyn MemoryStore>> {
    let cfg = ctx.cfg;
    file_dir_prep(cfg);
    let distill_provider = match cfg.memory.distill {
        true => Some(ctx.provider()?.clone()),
        false => None,
    };
    // Route by the ambient `(user, session)` identity so each user's episodic +
    // semantic memory is rooted at `<base>/<user>/…` — accumulating across their own
    // sessions but isolated from other users (docs/design/multi-session/04-tenancy.md).
    // The default `local` user maps to the un-namespaced base, so the single-user CLI
    // path is byte-identical.
    Ok(Arc::new(agent_memory::PerUserMemory::new(
        &cfg.memory.episodic_path,
        &cfg.memory.semantic_dir,
        distill_provider,
    )))
}

/// Factory for just the file episodic layer (used when a custom semantic backend
/// is composed against it via `[memory] semantic`).
#[cfg(feature = "memory-file")]
pub(crate) fn file_episodic(
    ctx: &crate::registry::FactoryCtx<'_>,
) -> anyhow::Result<Arc<dyn agent_core::EpisodicStore>> {
    let cfg = ctx.cfg;
    file_dir_prep(cfg);
    Ok(Arc::new(agent_memory::FileEpisodic::new(
        &cfg.memory.episodic_path,
    )))
}

/// Factory for just the file semantic layer.
#[cfg(feature = "memory-file")]
pub(crate) fn file_semantic(
    ctx: &crate::registry::FactoryCtx<'_>,
) -> anyhow::Result<Arc<dyn agent_core::SemanticStore>> {
    let cfg = ctx.cfg;
    file_dir_prep(cfg);
    let mut semantic = agent_memory::FileSemantic::new(&cfg.memory.semantic_dir);
    if cfg.memory.distill {
        semantic = semantic.with_provider(ctx.provider()?.clone());
    }
    Ok(Arc::new(semantic))
}

/// Best-effort directory prep shared by the file memory factories. (The factories
/// are sync; the stores also create dirs on first write.)
#[cfg(feature = "memory-file")]
fn file_dir_prep(cfg: &Config) {
    if let Some(parent) = std::path::Path::new(&cfg.memory.episodic_path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let _ = std::fs::create_dir_all(&cfg.memory.semantic_dir);
}

/// Resolve an API key with the standard precedence — inline > env > file — WITHOUT
/// requiring one: returns `""` when none is configured, so a keyless local pool
/// member (a server that ignores the key) stays valid. Errors only when a
/// configured `api_key_file` can't be read (fail closed on a real misconfig).
#[cfg(any(
    feature = "provider-openai-compat",
    feature = "provider-anthropic",
    feature = "provider-pool"
))]
fn resolve_key_opt(inline: &str, env: &str, file: &str) -> anyhow::Result<String> {
    if !inline.is_empty() {
        return Ok(inline.to_string());
    }
    if !env.is_empty() {
        if let Ok(v) = std::env::var(env) {
            if !v.is_empty() {
                return Ok(v);
            }
        }
    }
    if !file.is_empty() {
        let expanded = expand_tilde(file);
        let v = std::fs::read_to_string(&expanded)
            .with_context(|| format!("reading api_key_file `{expanded}`"))?;
        return Ok(v.trim().to_string());
    }
    Ok(String::new())
}

/// Resolve the API key without ever storing it in the repo: inline > env > file.
/// Shared by every provider factory that needs a key; unlike `resolve_key_opt` it
/// **requires** one (the top-level `[provider]` can't be keyless).
#[cfg(any(feature = "provider-openai-compat", feature = "provider-anthropic"))]
fn resolve_api_key(p: &ProviderCfg) -> anyhow::Result<String> {
    let key = resolve_key_opt(&p.api_key, &p.api_key_env, &p.api_key_file)?;
    if key.is_empty() {
        return Err(anyhow::anyhow!(
            "no API key: set provider.api_key, provider.api_key_env, or provider.api_key_file"
        ));
    }
    Ok(key)
}

/// The knobs for synthesizing an OpenAI-compatible upstream from config, borrowed so
/// both `[[pool.members]]` and `[[route.upstreams]]` can share one builder.
#[cfg(any(feature = "provider-pool", feature = "provider-router"))]
struct UpstreamParams<'a> {
    name: &'a str,
    endpoint: &'a str,
    model: &'a str,
    api_key: &'a str,
    api_key_env: &'a str,
    api_key_file: &'a str,
    insecure_tls: bool,
    context_window: u32,
}

/// Build an inline endpoint into an OpenAI-compatible provider — resolving the key
/// (inline > env > file, keyless allowed), honoring per-upstream `insecure_tls`
/// (warns) and `context_window`. Shared by the pool synth and the task-router factory
/// (model-router increments 01/02).
#[cfg(any(feature = "provider-pool", feature = "provider-router"))]
fn build_openai_upstream(
    p: &UpstreamParams<'_>,
) -> anyhow::Result<agent_providers::OpenAiCompatProvider> {
    let name = p.name;
    if p.model.is_empty() {
        anyhow::bail!("upstream `{name}` sets an endpoint but no model");
    }
    if p.insecure_tls {
        tracing::warn!(
            "upstream `{name}` insecure_tls=true: TLS verification is DISABLED (for a \
             self-signed dev endpoint you control). This exposes the API key and \
             traffic to man-in-the-middle attacks — do not use over untrusted networks."
        );
    }
    let api_key = resolve_key_opt(p.api_key, p.api_key_env, p.api_key_file)
        .with_context(|| format!("upstream `{name}` api key"))?;
    agent_providers::OpenAiCompatProvider::new(agent_providers::OpenAiCompatConfig {
        base_url: p.endpoint.to_string(),
        model: p.model.to_string(),
        api_key,
        insecure_tls: p.insecure_tls,
        context_window: p.context_window,
        max_retries: 2,
        supports_vision: false,
    })
    .map_err(|e| anyhow::anyhow!("building upstream `{name}`: {e}"))
}

/// Build an inline `[[pool.members]]` endpoint (unit-testable; model-router 01).
#[cfg(feature = "provider-pool")]
fn build_inline_pool_member(
    c: &crate::config::PoolMemberCfg,
    global_context_window: u32,
) -> anyhow::Result<agent_providers::OpenAiCompatProvider> {
    build_openai_upstream(&UpstreamParams {
        name: &c.name,
        endpoint: &c.endpoint,
        model: &c.model,
        api_key: &c.api_key,
        api_key_env: &c.api_key_env,
        api_key_file: &c.api_key_file,
        insecure_tls: c.insecure_tls,
        context_window: c.context_window.unwrap_or(global_context_window),
    })
}

/// Build one `[[route.upstreams]]` inline endpoint into an OpenAI-compatible provider
/// (model-router increment 02). `pub(crate)` so the registry's task-router factory
/// builds upstreams the same secret-safe way the pool builds members.
#[cfg(feature = "provider-router")]
pub(crate) fn build_route_upstream(
    u: &crate::config::RouteUpstreamCfg,
    global_context_window: u32,
) -> anyhow::Result<agent_providers::OpenAiCompatProvider> {
    build_openai_upstream(&UpstreamParams {
        name: &u.name,
        endpoint: &u.endpoint,
        model: &u.model,
        api_key: &u.api_key,
        api_key_env: &u.api_key_env,
        api_key_file: &u.api_key_file,
        insecure_tls: u.insecure_tls,
        context_window: u.context_window.unwrap_or(global_context_window),
    })
}

/// Map the `[route]` config into the pure `route::Policy` the `TaskRouter` runs
/// (model-router increment 02). Unknown tier/role strings degrade gracefully:
/// an unparseable prefer-tier ⇒ no tier bias; an unknown match-role ⇒ matches any.
#[cfg(feature = "provider-router")]
pub(crate) fn build_route_policy(cfg: &crate::config::RouteCfg) -> agent_providers::route::Policy {
    use agent_providers::route::{Match, Policy, Prefer, Role, Rule};
    let prefer = |p: &crate::config::RoutePreferCfg| Prefer {
        tags: p.tags.clone(),
        tier: agent_core::PoolTier::parse(&p.tier),
        upstreams: p.upstreams.clone(),
    };
    Policy {
        rules: cfg
            .rules
            .iter()
            .map(|r| Rule {
                match_: Match {
                    role: Role::parse(&r.match_.role),
                    min_context: r.match_.min_context,
                },
                prefer: prefer(&r.prefer),
            })
            .collect(),
        default_prefer: prefer(&cfg.default_prefer),
    }
}

#[cfg(any(
    feature = "provider-openai-compat",
    feature = "provider-anthropic",
    feature = "provider-pool",
    feature = "digest",
    feature = "graph"
))]
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

#[cfg(all(
    test,
    any(feature = "provider-openai-compat", feature = "provider-anthropic")
))]
mod tests {
    use super::*;
    use crate::config::ProviderCfg;
    use rstest::rstest;

    fn pcfg(api_key: &str, api_key_env: &str, api_key_file: &str) -> ProviderCfg {
        ProviderCfg {
            base_url: String::new(),
            model: String::new(),
            version: String::new(),
            supports_vision: false,
            api_key: api_key.into(),
            api_key_env: api_key_env.into(),
            api_key_file: api_key_file.into(),
            insecure_tls: false,
            max_retries: 0,
        }
    }

    // --- expand_tilde: `~/` → $HOME expansion ------------------------------
    #[rstest]
    #[case::negative_absolute("/abs/f")]
    #[case::negative_tilde_no_slash("~root/f")]
    #[case::corner_tilde_mid_path("a~/f")]
    #[case::boundary_empty("")]
    fn expand_tilde_passthrough_cases(#[case] input: &str) {
        assert_eq!(expand_tilde(input), input);
    }

    #[test]
    fn expand_tilde_replaces_home_prefix() {
        if let Ok(home) = std::env::var("HOME") {
            assert_eq!(expand_tilde("~/f.txt"), format!("{home}/f.txt"));
        }
    }

    // --- resolve_api_key: precedence inline > env > file -------------------
    #[test]
    fn resolve_api_key_prefers_inline() {
        assert_eq!(
            resolve_api_key(&pcfg("INLINE", "UNSET_ENV_NAME", "")).unwrap(),
            "INLINE"
        );
    }

    #[test]
    fn resolve_api_key_falls_back_to_env() {
        // A unique var name so parallel cases can't race on shared env state.
        let var = "AGENT_SEDDON_TEST_API_KEY_UNIQUE";
        std::env::set_var(var, "FROMENV");
        let got = resolve_api_key(&pcfg("", var, "")).unwrap();
        std::env::remove_var(var);
        assert_eq!(got, "FROMENV");
    }

    #[test]
    fn resolve_api_key_reads_file_trimmed() {
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "  FILEKEY\n").unwrap();
        assert_eq!(
            resolve_api_key(&pcfg("", "", f.to_str().unwrap())).unwrap(),
            "FILEKEY"
        );
    }

    #[test]
    fn resolve_api_key_errors_when_none_configured() {
        assert!(resolve_api_key(&pcfg("", "", "")).is_err());
    }

    // --- resolve_key_opt: same precedence, but keyless is ALLOWED (pool members) --
    #[test]
    fn positive_resolve_key_opt_prefers_inline() {
        assert_eq!(
            resolve_key_opt("INLINE", "UNSET_ENV_NAME", "").unwrap(),
            "INLINE"
        );
    }

    #[test]
    fn positive_resolve_key_opt_reads_file_trimmed() {
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "  FILEKEY\n").unwrap();
        assert_eq!(
            resolve_key_opt("", "", f.to_str().unwrap()).unwrap(),
            "FILEKEY"
        );
    }

    #[test]
    fn positive_resolve_key_opt_env_over_file() {
        // env beats file when both are set (precedence inline > env > file).
        let var = "AGENT_SEDDON_TEST_KEY_OPT_PREC";
        std::env::set_var(var, "FROMENV");
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "FROMFILE").unwrap();
        let got = resolve_key_opt("", var, f.to_str().unwrap()).unwrap();
        std::env::remove_var(var);
        assert_eq!(got, "FROMENV");
    }

    #[test]
    fn negative_resolve_key_opt_unset_env_falls_through_to_file() {
        // An env name whose var is UNSET is skipped gracefully — precedence continues
        // to the file (absent input handled, not treated as the literal name).
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "FROMFILE").unwrap();
        std::env::remove_var("AGENT_SEDDON_TEST_KEY_OPT_UNSET");
        assert_eq!(
            resolve_key_opt("", "AGENT_SEDDON_TEST_KEY_OPT_UNSET", f.to_str().unwrap()).unwrap(),
            "FROMFILE"
        );
    }

    #[test]
    fn corner_resolve_key_opt_empty_env_value_falls_through() {
        // An env var set to the EMPTY string counts as absent (not a real key), so
        // precedence falls through to the file.
        let var = "AGENT_SEDDON_TEST_KEY_OPT_EMPTY";
        std::env::set_var(var, "");
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "FROMFILE").unwrap();
        let got = resolve_key_opt("", var, f.to_str().unwrap()).unwrap();
        std::env::remove_var(var);
        assert_eq!(got, "FROMFILE");
    }

    #[test]
    fn corner_resolve_key_opt_whitespace_file_trims_to_empty() {
        // A present-but-blank key file trims to "" (keyless), not whitespace.
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "  \n\t\n").unwrap();
        assert_eq!(resolve_key_opt("", "", f.to_str().unwrap()).unwrap(), "");
    }

    #[test]
    fn boundary_resolve_key_opt_none_is_empty_not_error() {
        // A keyless local member is valid — unlike the top-level provider, no error.
        assert_eq!(resolve_key_opt("", "", "").unwrap(), "");
    }

    #[test]
    fn adversarial_resolve_key_opt_missing_file_fails_closed() {
        // A configured-but-unreadable key file is a real misconfig → error, not "".
        let err = resolve_key_opt("", "", "/no/such/agent-seddon/key-file").unwrap_err();
        assert!(err.to_string().contains("api_key_file"));
    }
}

/// Integration coverage for the inline `[[pool.members]]` synth: config knobs must
/// actually reach the built provider (per-member context window / key / TLS), not
/// just parse. Gated on `provider-pool` (a default feature) so `nix flake check`'s
/// workspace `cargo test` runs it (model-router increment 01).
#[cfg(all(test, feature = "provider-pool"))]
mod pool_member_synth_tests {
    use super::*;
    use crate::config::PoolMemberCfg;
    use agent_core::LlmProvider;

    fn member(name: &str) -> PoolMemberCfg {
        PoolMemberCfg {
            name: name.into(),
            endpoint: "http://host:11434/v1".into(),
            model: "m".into(),
            ..Default::default()
        }
    }

    #[test]
    fn positive_inline_member_uses_per_member_context_window() {
        let mut c = member("kimi");
        c.api_key = "k".into();
        c.context_window = Some(131_072);
        let p = build_inline_pool_member(&c, 8192).unwrap();
        // The per-member window reaches the provider's capabilities, not the global.
        assert_eq!(p.capabilities().context_window, 131_072);
    }

    #[test]
    fn corner_inline_member_falls_back_to_global_window() {
        let p = build_inline_pool_member(&member("mi50"), 8192).unwrap();
        assert_eq!(p.capabilities().context_window, 8192);
    }

    #[test]
    fn positive_inline_member_self_signed_key_from_file_builds() {
        // A self-signed hosted upstream (GLM shape): key from a file + insecure_tls
        // → the provider builds with no secret in the config.
        let dir = agent_testkit::tempdir();
        let f = dir.join("key");
        std::fs::write(&f, "SECRET\n").unwrap();
        let mut c = member("glm");
        c.api_key_file = f.to_str().unwrap().into();
        c.insecure_tls = true;
        assert!(build_inline_pool_member(&c, 8192).is_ok());
    }

    #[test]
    fn boundary_keyless_inline_member_builds() {
        // No key fields → a keyless local member still builds (no GPU-box regression).
        assert!(build_inline_pool_member(&member("local"), 8192).is_ok());
    }

    #[test]
    fn negative_inline_member_without_model_errors() {
        let mut c = member("x");
        c.model = String::new();
        assert!(build_inline_pool_member(&c, 8192).is_err());
    }

    #[test]
    fn adversarial_inline_member_missing_key_file_fails_closed() {
        let mut c = member("x");
        c.api_key_file = "/no/such/agent-seddon/key-file".into();
        // `.err()` (not `.unwrap_err()`) — the Ok type isn't `Debug`.
        let msg = build_inline_pool_member(&c, 8192)
            .err()
            .expect("missing key file must fail")
            .to_string();
        assert!(msg.contains("api key"));
    }
}

/// The `[route]` config → `route::Policy` mapping (model-router increment 02).
#[cfg(all(test, feature = "provider-router"))]
mod route_policy_tests {
    use super::*;
    use crate::config::{RouteCfg, RouteMatchCfg, RoutePreferCfg, RouteRuleCfg};

    #[test]
    fn positive_maps_roles_tiers_tags_and_default() {
        let cfg = RouteCfg {
            rules: vec![RouteRuleCfg {
                match_: RouteMatchCfg {
                    role: "review".into(),
                    min_context: 32_000,
                },
                prefer: RoutePreferCfg {
                    tags: vec!["reasoning".into()],
                    tier: "heavy".into(),
                    upstreams: vec![],
                },
            }],
            default_prefer: RoutePreferCfg {
                upstreams: vec!["kimi".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let policy = build_route_policy(&cfg);
        assert_eq!(policy.rules.len(), 1);
        assert_eq!(
            policy.rules[0].match_.role,
            Some(agent_providers::route::Role::Review)
        );
        assert_eq!(policy.rules[0].match_.min_context, 32_000);
        assert_eq!(
            policy.rules[0].prefer.tier,
            Some(agent_core::PoolTier::Heavy)
        );
        assert_eq!(policy.default_prefer.upstreams, vec!["kimi"]);
    }

    #[test]
    fn corner_unknown_role_and_tier_degrade_to_none() {
        let cfg = RouteCfg {
            rules: vec![RouteRuleCfg {
                match_: RouteMatchCfg {
                    role: "bogus".into(),
                    min_context: 0,
                },
                prefer: RoutePreferCfg {
                    tier: "huge".into(),
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        let policy = build_route_policy(&cfg);
        // Unknown role ⇒ matches any; unknown tier ⇒ no tier bias (graceful).
        assert_eq!(policy.rules[0].match_.role, None);
        assert_eq!(policy.rules[0].prefer.tier, None);
    }
}

/// Build the config-selected content scanner, or `None` when `[scanner] rules`
/// is empty (scanning off by default — it is a control, not a default-on cost).
#[cfg(feature = "scanner")]
fn build_scanner(
    cfg: &Config,
    metrics: &Metrics,
) -> anyhow::Result<Option<(Arc<dyn agent_core::Scanner>, agent_core::Severity)>> {
    if cfg.scanner.rules.is_empty() {
        return Ok(None);
    }
    let mut subs: Vec<Arc<dyn agent_core::Scanner>> = Vec::new();
    for rule in &cfg.scanner.rules {
        match rule.as_str() {
            "secret" => subs.push(Arc::new(agent_scanner::SecretScanner::new())),
            "threat" => subs.push(Arc::new(agent_scanner::ThreatScanner::new(
                agent_scanner::Scope::parse(&cfg.scanner.scope),
            ))),
            // A remote scanner composes like any other rule, so a deployment can
            // run local rules AND a central one: rules = ["secret", "grpc"].
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.scanner.endpoint,
                    agent_grpc::constants::SCANNER,
                );
                subs.push(Arc::new(agent_grpc::client::GrpcScanner::connect(&ep)?));
            }
            other => anyhow::bail!("unknown [scanner] rule `{other}` (secret|threat|grpc)"),
        }
    }
    let dispatch = agent_scanner::DispatchScanner::new(subs)
        .with_allowlist(cfg.scanner.allow_rules.iter().cloned());
    let scanner = crate::metered::scanner(Arc::new(dispatch), metrics.clone());
    Ok(Some((
        scanner,
        agent_core::Severity::parse(&cfg.scanner.deny_at),
    )))
}

/// Build the config-selected prompt-cache placement strategy (parity spec 24).
/// `"off"` ⇒ `None`, and the request body is byte-identical to a build without
/// this feature.
#[cfg(feature = "cache")]
pub(crate) fn build_cache_strategy(
    ctx: &crate::registry::FactoryCtx<'_>,
) -> anyhow::Result<Option<Arc<dyn agent_core::CacheStrategy>>> {
    let cfg = ctx.cfg;
    let inner: Arc<dyn agent_core::CacheStrategy> = match cfg.cache.strategy.as_str() {
        "off" => return Ok(None),
        "stable-prefix" => Arc::new(agent_cache::StablePrefix),
        "tail-window" => Arc::new(agent_cache::TailWindow::new(cfg.cache.tail_back)),
        other => {
            anyhow::bail!("unknown [cache] strategy `{other}` (stable-prefix|tail-window|off)")
        }
    };
    Ok(Some(crate::metered::cache(inner, ctx.metrics.clone())))
}

#[cfg(test)]
mod seam_builder_tests {
    use super::*;

    // ---- build_scanner: `[scanner] rules` resolution ----------------------

    // Scanning is off by default (a control, not a default-on cost).
    #[cfg(feature = "scanner")]
    #[test]
    fn positive_build_scanner_empty_rules_is_none() {
        let cfg = crate::config::Config::minimal_for_test();
        let metrics = Metrics::new();
        assert!(build_scanner(&cfg, &metrics).unwrap().is_none());
    }

    #[cfg(feature = "scanner")]
    #[rstest::rstest]
    #[case::secret(&["secret"])]
    #[case::threat(&["threat"])]
    #[case::composed(&["secret", "threat"])]
    fn positive_build_scanner_known_rules(#[case] rules: &[&str]) {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.scanner.rules = rules.iter().copied().map(String::from).collect();
        let metrics = Metrics::new();
        assert!(build_scanner(&cfg, &metrics).unwrap().is_some());
    }

    // Untrusted config: an unknown scanner rule fails closed, listing the built-ins.
    #[cfg(feature = "scanner")]
    #[rstest::rstest]
    #[case::garbage("bogus")]
    #[case::empty("")]
    #[case::uppercased_known("SECRET")]
    fn adversarial_unknown_scanner_rule_bails(#[case] rule: &str) {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.scanner.rules = vec![rule.to_string()];
        let metrics = Metrics::new();
        let err = match build_scanner(&cfg, &metrics) {
            Ok(_) => panic!("an unknown scanner rule `{rule}` must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown [scanner] rule"), "got: {err}");
    }

    // ---- build_cache_strategy: `[cache] strategy` resolution --------------

    #[cfg(feature = "cache")]
    #[rstest::rstest]
    #[case::stable_prefix("stable-prefix")]
    #[case::tail_window("tail-window")]
    fn positive_build_cache_strategy_known(#[case] strategy: &str) {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.cache.strategy = strategy.into();
        let metrics = Metrics::new();
        let ctx = crate::registry::FactoryCtx::new(&cfg, &metrics);
        assert!(build_cache_strategy(&ctx).unwrap().is_some());
    }

    #[cfg(feature = "cache")]
    #[test]
    fn positive_build_cache_strategy_off_is_none() {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.cache.strategy = "off".into();
        let metrics = Metrics::new();
        let ctx = crate::registry::FactoryCtx::new(&cfg, &metrics);
        assert!(build_cache_strategy(&ctx).unwrap().is_none());
    }

    #[cfg(feature = "cache")]
    #[rstest::rstest]
    #[case::garbage("bogus")]
    #[case::empty("")]
    fn adversarial_unknown_cache_strategy_bails(#[case] strategy: &str) {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.cache.strategy = strategy.into();
        let metrics = Metrics::new();
        let ctx = crate::registry::FactoryCtx::new(&cfg, &metrics);
        let err = match build_cache_strategy(&ctx) {
            Ok(_) => panic!("an unknown cache strategy `{strategy}` must fail"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown [cache] strategy"), "got: {err}");
    }
}
