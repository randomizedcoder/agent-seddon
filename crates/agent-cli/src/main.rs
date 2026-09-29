//! `agent` — a coding agent.
//!
//! Usage:
//!   agent [--config PATH] [--continue | --resume ID] [<goal words...>]
//!
//! With a goal, runs it once (one-shot). With no goal, enters an interactive
//! multi-turn REPL (see `repl.rs`). `--continue` resumes the most recent saved
//! session; `--resume ID` resumes a specific one.

mod campaign_cli;
mod grpc_server;
mod mcp_server;
mod metrics_server;
mod reload;
mod repl;
mod shutdown;

use agent_runtime::{session_store, Metrics};
use agent_telemetry::{ClickHouseLayer, OtelConfig, OtelGuard, TelemetryConfig, TelemetryHandle};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter, Registry};

#[tokio::main]
async fn main() -> Result<()> {
    let Args {
        config_path,
        mode,
        resume,
        cognition_graph,
        model_router_config,
    } = parse_args()?;

    // `agent --run-task --tenant T --task <id>` (docs/design/campaigns, CP-06b): the
    // worker subprocess the campaign driver dispatches. The owner token comes
    // through `AGENT_CAMPAIGN_OWNER` (never an argument) and is checked BEFORE the
    // config is read or any store is opened: missing / unsafe ⇒ `lease lost`
    // (exit 3), nothing touched. The token is never printed. With a valid owner
    // the run continues: config, store, agent build, then the worker protocol in
    // the `Mode::RunTask` arm below.
    let run_task_owner = match &mode {
        Mode::RunTask { tenant, task } => {
            let owner = std::env::var(agent_core::campaign::CAMPAIGN_OWNER_ENV).ok();
            match campaign_cli::run_task_owner(owner.as_deref()) {
                Ok(o) => Some(o),
                Err((code, msg)) => {
                    eprintln!("{msg} (tenant {tenant}, task #{task})");
                    std::process::exit(code);
                }
            }
        }
        _ => None,
    };

    // `agent token` (security-hardening S23): print the stored login's token for
    // another program (the native portal). Before the config is required: without
    // a config file the only stored login is used.
    #[cfg(feature = "auth")]
    if let Mode::Token { issuer, json } = &mode {
        let config = match std::fs::read_to_string(&config_path) {
            Ok(toml) => Some(
                agent_runtime::parse_config_reporting_unknown(&toml)
                    .context("parsing config")?
                    .0,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("reading config `{}`", config_path.display()))
            }
        };
        match agent_runtime::login::token(config.as_ref(), issuer.as_deref(), *json).await {
            Ok(out) => {
                println!("{out}");
                return Ok(());
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(e.exit_code());
            }
        }
    }

    let toml_str = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading config `{}`", config_path.display()))?;
    // Parse with the ignored-key list in hand rather than letting `parse_config`
    // log it: this runs BEFORE `init_tracing` below, so a `tracing::warn!` here
    // would have no subscriber and be swallowed. The warnings are emitted once
    // tracing is up, further down.
    let (mut config, unknown_config_keys) =
        agent_runtime::parse_config_reporting_unknown(&toml_str).context("parsing config")?;
    // Record where the config came from so the `ConfigStore` seam (portal
    // settings) can write edits back to the same file.
    config.source_path = Some(config_path.clone());
    // A worker child (`--run-task`) re-reads the driver's config, and the driver
    // holds the writer lock on the shared search index: give the child its own
    // disposable index dirs before the build (removed after the leaf).
    let run_task_index_dirs = match &mode {
        Mode::RunTask { task, .. } => {
            agent_runtime::campaign_worker::isolate_indexes(&mut config, *task)
        }
        _ => Vec::new(),
    };
    // `agent login` / `logout` / `whoami` (security-hardening S12): talk to the IdP
    // and the agent's `AuthService`, then exit. Before the egress proxy (the IdP is
    // not on its allow-list), telemetry, and any server.
    #[cfg(feature = "auth")]
    match &mode {
        Mode::Login {
            issuer,
            endpoint,
            browser: false,
        } => {
            return agent_runtime::login::login(&config, issuer.as_deref(), endpoint.as_deref())
                .await;
        }
        Mode::Login {
            issuer,
            endpoint,
            browser: true,
        } => {
            return agent_runtime::login::login_browser(
                &config,
                issuer.as_deref(),
                endpoint.as_deref(),
            )
            .await;
        }
        Mode::Logout { issuer } => {
            return agent_runtime::login::logout(&config, issuer.as_deref()).await;
        }
        Mode::WhoAmI { issuer } => {
            return agent_runtime::login::whoami(&config, issuer.as_deref()).await;
        }
        _ => {}
    }
    #[cfg(not(feature = "auth"))]
    if matches!(
        mode,
        Mode::Login { .. } | Mode::Logout { .. } | Mode::WhoAmI { .. } | Mode::Token { .. }
    ) {
        anyhow::bail!("`agent login` needs the agent built with the `auth` feature");
    }
    // `--cognition-graph FILE` = `[graph] store = "file", file = FILE` — the
    // scenario-file form (config/cognition/*.textproto).
    if let Some(file) = cognition_graph {
        config.graph.store = "file".to_string();
        config.graph.file = file;
    }
    // `--model-router-config FILE` > `AGENT_MODEL_ROUTER_CONFIG` > the
    // `[agent] model_router_config` key — the model-router textproto scenario
    // file (model-router 03, config/model-router/*.textproto). The builder
    // loads it fail-closed.
    if let Some(file) = model_router_config {
        config.agent.model_router_config = file;
    } else if let Ok(file) = std::env::var("AGENT_MODEL_ROUTER_CONFIG") {
        if !file.is_empty() {
            config.agent.model_router_config = file;
        }
    }
    // Captured before `config` is consumed by the builder.
    let cfg_tick_secs = config.scheduler.tick_secs;
    // One-shot exit drain deadline for background distillation (clamped in
    // `drain_timeout()`); captured for the same reason.
    let digest_drain_timeout = config.digest.drain_timeout();
    // Captured before `config` moves into `build_agent` (see the metrics note below).
    let review_budget = config.review.context_budget_bytes;
    // The fleet Preflight RPC's operational probe set (docs/design/doctor/), built
    // from `config` before the builder consumes it. Only the full `--serve-fleet`
    // process wires it; every other mode leaves it `None`.
    let fleet_preflight: Option<std::sync::Arc<dyn agent_core::PreflightProvider>> =
        matches!(mode, Mode::ServeFleet(_)).then(|| {
            std::sync::Arc::new(agent_runtime::doctor::DoctorProbes::from_config(&config))
                as std::sync::Arc<dyn agent_core::PreflightProvider>
        });

    // C23-3c: agent-process egress allow-list. When enabled, start a loopback CONNECT
    // filtering proxy and pin this process's `reqwest` egress to it via
    // `HTTPS_PROXY`/`HTTP_PROXY`, so the agent reaches only allow-listed hosts (providers +
    // forge + `[web]` + operator extras). Done HERE — before telemetry, `agent doctor`, and
    // `build_agent` construct any `reqwest` client, so the env is in place when they read
    // it. Fail-closed: a bind failure refuses to start the agent (it would otherwise run
    // with egress unfiltered while the operator believes it is on). The confirmation is
    // logged after `init_tracing` below (no subscriber exists yet here).
    let egress_addr = if config.sandbox.egress.enabled {
        let rules = agent_runtime::derive_egress_allowlist(&config);
        let matcher = std::sync::Arc::new(agent_egress::HostMatcher::new(rules));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("egress allow-list: binding the loopback proxy")?;
        let addr = listener
            .local_addr()
            .context("egress allow-list: proxy local addr")?;
        for (k, v) in agent_egress::proxy_env(addr) {
            std::env::set_var(k, v);
        }
        tokio::spawn(agent_egress::serve(listener, matcher.clone()));
        Some((addr, matcher.len()))
    } else {
        None
    };

    // Telemetry (opt-in). Build the writer before installing tracing so the
    // ClickHouse layer can stream logs from the very first event.
    let (telemetry, session_id) = if config.telemetry.enabled {
        let session_id = uuid::Uuid::new_v4().to_string();
        let handle = TelemetryHandle::spawn(
            TelemetryConfig {
                addr: config.telemetry.clickhouse_url.clone(),
                database: config.telemetry.database.clone(),
                user: config.telemetry.user.clone(),
                password: config
                    .telemetry
                    .writer_password()
                    .map_err(anyhow::Error::msg)?,
                batch_max_rows: config.telemetry.batch_max_rows,
                flush_interval: Duration::from_millis(config.telemetry.flush_interval_ms),
            },
            session_id.clone(),
        );
        // Every auth/authz event (security-hardening S11) goes to
        // `agent_auth_events` through the same writer.
        let audit = handle.clone();
        agent_core::set_auth_audit(std::sync::Arc::new(move |event| {
            audit.record_auth_event(event);
        }));
        (Some(handle), session_id)
    } else {
        (None, String::new())
    };
    // The campaign stores' event mirror (docs/design/campaigns/04-executor.md
    // §Observability, CP-08): every committed `task_events` row this process
    // writes — a CLI verb's, the driver's, an `--run-task` child's — becomes an
    // `agent_events` row when telemetry is on; off, the stores mirror nothing.
    let campaign_sink: Option<std::sync::Arc<dyn agent_core::campaign::EventSink>> =
        telemetry.clone().map(|handle| {
            std::sync::Arc::new(handle) as std::sync::Arc<dyn agent_core::campaign::EventSink>
        });

    // OTLP tracing (opt-in, independent of the ClickHouse sink): enabled by a
    // non-empty `otlp_endpoint`. Tag spans with the run's session id when we have one.
    let otel_cfg = (!config.telemetry.otlp_endpoint.is_empty()).then(|| OtelConfig {
        endpoint: config.telemetry.otlp_endpoint.clone(),
        service_name: config.telemetry.otel_service_name.clone(),
        instance_id: (!session_id.is_empty()).then(|| session_id.clone()),
        headers: config.telemetry.otlp_headers.clone(),
    });
    let otel_guard = init_tracing(&telemetry, config.telemetry.stream_logs, otel_cfg);

    // Confirm the C23-3c egress allow-list now that a subscriber exists.
    if let Some((addr, rules)) = egress_addr {
        tracing::info!(
            proxy = %addr,
            allow_hosts = rules,
            "C23-3c egress allow-list active — process egress restricted to allow-listed hosts"
        );
        if rules == 0 {
            tracing::warn!(
                "egress allow-list is enabled but empty — ALL process egress is blocked \
                 (no provider/forge/web hosts derived; add hosts under [sandbox.egress])"
            );
        }
    }

    // Now that there is a subscriber, report anything in the config that nothing
    // reads. Ignored keys are not fatal, but they must not be silent: this file
    // selects which implementation each seam uses, so a misplaced key means the
    // agent quietly runs something other than what was asked for.
    for key in &unknown_config_keys {
        tracing::warn!(
            key = %key,
            "unknown config key — it is being IGNORED, so anything it was meant to \
             configure is running its default. Check the spelling and the section \
             it belongs in (see config/agent.toml)"
        );
    }

    // `agent doctor`: the agent determines its own operational state and prints a
    // report, then exits — before any server/metrics machinery starts. Unlike
    // `--check-config` this dials the network (ClickHouse liveness). Exit is
    // non-zero iff a required probe failed (warnings/skips do not fail the gate).
    // Placed here so it borrows `config` before the builder consumes it, and never
    // starts a metrics or seam server. See docs/design/doctor/.
    if matches!(mode, Mode::Doctor) {
        let report = agent_runtime::doctor::diagnose(&config).await;
        for p in &report.probes {
            let latency = if p.latency_ms > 0 {
                format!(" ({}ms)", p.latency_ms)
            } else {
                String::new()
            };
            println!(
                "  [{:>7}] {:<13} {}{}",
                p.status.as_str(),
                p.name,
                p.detail,
                latency
            );
        }
        println!(
            "doctor: {} ({} ok, {} warn, {} fail, {} skipped)",
            if report.ok() { "OK" } else { "FAIL" },
            report.count(agent_core::ProbeStatus::Ok),
            report.count(agent_core::ProbeStatus::Warn),
            report.count(agent_core::ProbeStatus::Fail),
            report.count(agent_core::ProbeStatus::Skipped),
        );
        if !report.ok() {
            anyhow::bail!("doctor: one or more probes failed");
        }
        return Ok(());
    }

    // `agent campaign …` (docs/design/campaigns, CP-04): the store-only verbs run
    // here, before any metrics or seam machinery starts — like `doctor`. The store
    // is opened lazily and migrated on this first use when `[config_store]
    // migrate_on_start` allows it; `--tenant` only selects the tenant view. The
    // verbs run as `user:local` under a fresh local session scope. `plan`, `run
    // --once` and `run` need the planner (a provider), so they keep the opened
    // store — and, for the driver verbs, the multi-tenant backend (CP-05) — and
    // fall through to the built agent's `scope` arm below.
    let mut campaign_run: Option<CampaignRun> = None;
    if let Mode::Campaign(args) = &mode {
        // The resident driver is gated by `[campaign] enabled` (an explicit
        // opt-in, since it claims and dispatches on its own); refused here, after
        // the config load and before anything opens. `run --once` is an explicit
        // human action and ignores the key.
        if args.cmd == campaign_cli::CampaignCmd::Run && !config.campaign.enabled {
            anyhow::bail!(
                "agent campaign run: the resident driver is off — set `[campaign] enabled = true` \
                 (or use `run --once` for a single tick)"
            );
        }
        let store = agent_runtime::campaign::open_campaign_store(
            &config,
            agent_runtime::campaign::CampaignOpen {
                tenant: args.tenant.as_deref(),
                apply_migrations: true,
                sink: campaign_sink.clone(),
            },
        )
        .await
        .context("[campaign] store")?
        .context(
            "agent campaign: no campaign store is configured — set `[campaign] store = \
             \"postgres\"` (the DSN comes from `[config_store] dsn_ref`)",
        )?;
        let ctx = campaign_cli::CampaignCtx {
            store,
            repos: config.campaign.repos.clone(),
        };
        if !args.cmd.needs_planner() {
            let identity = agent_core::SessionKey::local(uuid::Uuid::new_v4().to_string());
            return agent_core::scope(identity, async {
                let stdout = std::io::stdout();
                let mut out = stdout.lock();
                campaign_cli::run(&ctx, &args.cmd, &mut out).await
            })
            .await;
        }
        // The driver verbs also need the backend (every tenant's view); the store
        // above already applied the schema, so this open never migrates.
        let backend = if args.cmd.needs_driver() {
            Some(
                agent_runtime::campaign::open_campaign_backend(
                    &config,
                    false,
                    campaign_sink.clone(),
                )
                .await
                .context("[campaign] store")?
                .context("agent campaign run: no campaign store is configured")?,
            )
        } else {
            None
        };
        // `config` moves into the builder below; keep what the planner needs.
        let working_dir = if config.agent.working_dir.is_empty() {
            std::env::current_dir().context("resolving the working directory")?
        } else {
            PathBuf::from(&config.agent.working_dir)
        };
        campaign_run = Some(CampaignRun {
            ctx,
            cfg: config.campaign.clone(),
            repo_root: config.campaign.repo_root_or(&working_dir),
            main_model: config.provider.model.clone(),
            backend,
            tenants: agent_runtime::campaign_driver::tenants_for(&config, args.tenant.as_deref()),
            worker: agent_runtime::campaign_worker::WorkerCfg::from_config(&config),
            config_path: config.source_path.clone(),
        });
    }

    // `agent --run-task` (CP-06b): the worker opens its tenant's store view now —
    // lazily, no migration (the driver's own open applied the schema) — and keeps
    // the worker knobs before `config` moves into the builder. No store ⇒ exit 1
    // naming the key; the driver then fails the leaf with the exit.
    let mut run_task: Option<RunTaskRun> = None;
    if let Mode::RunTask { tenant, .. } = &mode {
        let store = agent_runtime::campaign::open_campaign_store(
            &config,
            agent_runtime::campaign::CampaignOpen {
                tenant: Some(tenant),
                apply_migrations: false,
                sink: campaign_sink.clone(),
            },
        )
        .await
        .context("[campaign] store")?
        .context(
            "agent --run-task: no campaign store is configured — set `[campaign] store = \
             \"postgres\"` (the DSN comes from `[config_store] dsn_ref`)",
        )?;
        run_task = Some(RunTaskRun {
            store,
            worker: agent_runtime::campaign_worker::WorkerCfg::from_config(&config),
            owner: run_task_owner
                .clone()
                .expect("the owner is checked before the config is read"),
            index_dirs: run_task_index_dirs.clone(),
        });
    }

    // Metrics (opt-in). Instrumentation always runs into this registry; serving
    // the /metrics endpoint and pushing are gated by config.
    let metrics = Metrics::new();
    if config.metrics.enabled {
        // A `--serve-<seam>` process serves `/metrics` on that seam's dedicated
        // port so several co-located seam servers don't collide on `:9600`.
        let listen = match &mode {
            Mode::ServeGrpc(seam, _) => format!("127.0.0.1:{}", seam.metrics_port()),
            Mode::ServeGrpcAll(_) => {
                format!("127.0.0.1:{}", agent_grpc::constants::GATEWAY.metrics_port)
            }
            Mode::ServeSessions(_) => {
                format!("127.0.0.1:{}", agent_grpc::constants::SESSIONS.metrics_port)
            }
            Mode::ServeFleet(_) => {
                format!("127.0.0.1:{}", agent_grpc::constants::FLEET.metrics_port)
            }
            _ => config.metrics.listen.clone(),
        };
        metrics_server::serve(metrics.clone(), &listen);
    }
    let metrics_cfg = MetricsRun {
        enabled: config.metrics.enabled,
        pushgateway: config.metrics.pushgateway.clone(),
        job: config.metrics.job.clone(),
    };

    // Resolve the gRPC serve target (which needs `config.grpc`) before `config` is
    // moved into `build_agent`.
    let serve_grpc: Option<(grpc_server::Seam, agent_grpc::Endpoint)> = match &mode {
        Mode::ServeGrpc(seam, listen) => Some((
            *seam,
            grpc_server::resolve_listen(*seam, &config, listen.as_deref()),
        )),
        _ => None,
    };
    let serve_grpc_all_listen: Option<agent_grpc::Endpoint> = match &mode {
        Mode::ServeGrpcAll(listen) => Some(grpc_server::resolve_gateway_listen(
            &config,
            listen.as_deref(),
        )),
        _ => None,
    };
    let serve_sessions_listen: Option<agent_grpc::Endpoint> = match &mode {
        Mode::ServeSessions(listen) => Some(grpc_server::resolve_sessions_listen(
            &config,
            listen.as_deref(),
        )),
        _ => None,
    };
    // The full fleet process binds the Fleet seam's endpoint (`[grpc.fleet] listen`,
    // else the generated FLEET port) — the same endpoint the bare seam uses.
    let serve_fleet_listen: Option<agent_grpc::Endpoint> = match &mode {
        Mode::ServeFleet(listen) => Some(grpc_server::resolve_listen(
            grpc_server::Seam::Fleet,
            &config,
            listen.as_deref(),
        )),
        _ => None,
    };

    // A non-empty `[grpc.session_stream] listen` makes a *running* agent observable:
    // the OneShot/REPL run also hosts `AgentSessionService` on this endpoint, sharing
    // the live session source, so a portal sees the loop's events in real time
    // (docs/design/portal). Empty ⇒ no observe server (the default).
    let observe_listen: Option<agent_grpc::Endpoint> = {
        let l = &config.grpc.session_stream.listen;
        (!l.is_empty()).then(|| agent_grpc::Endpoint::parse(l))
    };

    let sessions_dir = session_store::default_dir();
    tracing::info!(session_id = %session_id, "starting agent");

    // `--check-config`: capture the selected impls before `config` is consumed by
    // the builder, so we can report them once the build proves every selector
    // resolves to a registered factory.
    let check_config = matches!(mode, Mode::CheckConfig);
    if check_config {
        // Dry-open the `[campaign] store` (docs/design/campaigns, CP-04): proves
        // the selected arm is linked and its DSN reference resolves, LAZILY —
        // no dial, no migration — so the check stays hermetic.
        agent_runtime::campaign::open_campaign_store(
            &config,
            agent_runtime::campaign::CampaignOpen {
                tenant: None,
                apply_migrations: false,
                sink: None,
            },
        )
        .await
        .context("[campaign] store")?;
    }
    let selections = check_config.then(|| ConfigSelections {
        provider: config.agent.provider.clone(),
        context: config.agent.context.clone(),
        policy: config.agent.policy.clone(),
        memory: config.memory.backend.clone(),
        tokenizer: config.tokenizer.backend.clone(),
        search: config.search.backend_names().join(","),
        tools: config.tools.enabled.len(),
        campaign: agent_runtime::campaign::backend_label(&config),
        role: role_label(&config.role.store),
    });

    // `--check-config` builds in `CheckConfig` mode: every seam still resolves
    // through the real factory chain, but deferrable startup reads (the RBAC
    // catalog from a `[role] store = "postgres"`) are skipped, so the dry run of a
    // Postgres profile opens no socket.
    let agent = agent_runtime::build_agent_mode(
        config,
        telemetry.clone(),
        session_id.clone(),
        metrics.clone(),
        if check_config {
            agent_runtime::BuildMode::CheckConfig
        } else {
            agent_runtime::BuildMode::Run
        },
    )
    .await
    .context("building agent")?;

    // A validated config is the whole job for `--check-config`: the build above
    // resolved every seam, so print the selections and exit before running.
    if check_config {
        let s = selections.expect("selections are captured whenever check_config is set");
        println!("config: OK ({})", config_path.display());
        println!("  provider  = {}", s.provider);
        println!("  context   = {}", s.context);
        println!("  policy    = {}", s.policy);
        println!("  memory    = {}", s.memory);
        println!("  tokenizer = {}", s.tokenizer);
        println!("  search    = {}", s.search);
        println!("  tools     = {} enabled", s.tools);
        println!("  campaign  = {}", s.campaign);
        println!("  role      = {}", s.role);
        return Ok(());
    }

    // Resolve an optional resume target to (id, transcript).
    let resumed = resolve_resume(&resume, &sessions_dir);

    // Multi-session identity origin (docs/design/multi-session/01-identity.md). This
    // process is a single (local) user; its session id is the telemetry session id
    // when present, else a freshly-minted one so a `= "grpc"` seam call still carries
    // a well-formed identity. Scoping the run means any remote seam the loop dials
    // receives `(user, session)` in its metadata; server handler tasks are spawned by
    // tonic and do *not* inherit this scope, so a hosted seam still uses its caller's
    // identity, never this process's.
    let run_session = if session_id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        session_id.clone()
    };
    // A scheduled-job child (scheduler S2b) scopes the whole run to its owning tenant
    // — validated fail-closed, never falling back to `local`, since a wrong-tenant run
    // would cross the isolation boundary. Every other mode is this (local) process.
    let identity = match &mode {
        Mode::RunScheduledJob { tenant, .. } => agent_core::SessionKey::parse(tenant, "scheduler")
            .with_context(|| format!("--tenant `{tenant}` is not a valid tenant segment"))?,
        // A campaign worker child (CP-06b) likewise: the tenant it was dispatched
        // for, the leaf's own session id.
        Mode::RunTask { tenant, task } => agent_core::SessionKey::parse(
            tenant,
            &agent_runtime::campaign_worker::leaf_session(*task),
        )
        .with_context(|| format!("--tenant `{tenant}` is not a valid tenant segment"))?,
        _ => agent_core::SessionKey::local(run_session),
    };

    // `--run-task`'s protocol exit (0 / 1 / 3), surfaced after the cleanup below.
    let mut run_task_exit: Option<i32> = None;

    // Run either one-shot or the REPL, capturing the answer (one-shot only).
    let outcome: Result<Option<String>> = agent_core::scope(identity, async {
        match mode {
            Mode::OneShot(goal) => {
                let mut session = agent.session();
                let id = match resumed {
                    Some((rid, msgs)) => {
                        session.load(msgs);
                        rid
                    }
                    None => repl::new_id(),
                };
                // Race the run against Ctrl-C / SIGTERM: an interrupt should save the
                // (partial) transcript and clean up rather than killing the process and
                // orphaning state. Cancelling `send` drops its future; the working set it mutated
                // in place is still readable via `messages()`.
                let outcome: Result<Option<String>> = tokio::select! {
                    r = session.send(&goal) => r.map(Some),
                    sig = shutdown::signal() => {
                        eprintln!("\n{sig} — interrupted; saving session and cleaning up…");
                        Ok(None)
                    }
                    // Runs concurrently, sharing the live session source; resolves only
                    // if its bind fails. Dropped (server stops) when the run finishes.
                    res = run_observe(&agent, observe_listen) => res.map(|()| None),
                };
                // Always persist the transcript (success, error, or interrupt) so the
                // run is resumable with `--resume {id}` / `--continue`.
                if let Err(e) = session_store::save(&sessions_dir, &id, session.messages()) {
                    tracing::warn!("could not save session `{id}`: {e}");
                } else {
                    tracing::info!("session saved as `{id}`");
                }
                // One-shot exit would kill the background distiller mid-job —
                // bounded drain so the delivered turn's digest rows land
                // (cognition-graph 02; no-op when nothing is pending). Deadline
                // from `[digest] drain_timeout_s` — slow reasoning distillers
                // need more than the old fixed 60s (live-observed drop).
                session.drain_background(digest_drain_timeout).await;
                outcome
            }
            Mode::Repl => tokio::select! {
                r = repl::run(&agent, &sessions_dir, resumed) => r.map(|()| None),
                res = run_observe(&agent, observe_listen) => res.map(|()| None),
            },
            Mode::Scheduler => {
                // Tick until interrupted. Each due job runs as a fresh headless turn,
                // and the scheduler's own overlap guard is what stops a slow job
                // stacking copies of itself.
                let every = std::time::Duration::from_secs(cfg_tick_secs.max(1));
                eprintln!("scheduler: ticking every {}s — ^C to stop", every.as_secs());
                loop {
                    tokio::select! {
                        () = tokio::time::sleep(every) => {
                            let n = agent.tick_scheduler().await;
                            if n > 0 {
                                tracing::info!(jobs = n, "scheduler fired due jobs");
                            }
                        }
                        sig = shutdown::signal() => {
                            eprintln!("\n{sig} — stopping the scheduler");
                            break;
                        }
                    }
                }
                Ok(None)
            }
            Mode::RunScheduledJob { goal, .. } => {
                // One headless turn, scoped (above) to the owning tenant — the exact
                // shape the in-process driver fires, now as a spawnable subprocess the
                // sandboxed driver dispatches (scheduler S2b). The answer goes to
                // stdout; a run error propagates so the process exits non-zero and the
                // driver records the job Failed.
                let answer = agent
                    .run(&goal)
                    .await
                    .context("running the scheduled job")?;
                Ok(Some(answer))
            }
            Mode::Review(target, gate) => match agent.review_collector() {
                Some(collector) => {
                    let facts = collector
                        .collect(&target)
                        .await
                        .context("collecting grounded review facts")?;
                    // Record the run (best-effort telemetry → agent_reviews / episodic).
                    agent
                        .record_review(agent_core::ReviewRecord::from_facts(&facts, "explicit"))
                        .await;
                    println!("{}", agent_review::render_facts_with(&facts, review_budget));
                    // `--gate`: a changed-files-only CI gate — fail the build (non-zero
                    // exit) when the synthesized risk crosses the configured threshold.
                    if gate && facts.risk.gate_failed {
                        anyhow::bail!(
                            "review gate FAILED: {} at risk {:.2} ≥ threshold {:.2}",
                            facts
                                .risk
                                .files
                                .first()
                                .map(|f| f.file.as_str())
                                .unwrap_or("(unknown)"),
                            facts.risk.max_score,
                            facts.risk.gate_threshold,
                        );
                    }
                    Ok(None)
                }
                None => anyhow::bail!(
                "the review flow is not enabled — set `[review] backend = \"local\"` in the config"
            ),
            },
            Mode::Detect(prompt) => match agent.task_classifier() {
                Some(classifier) => {
                    let v = classifier
                        .classify(&agent_core::ClassifyCtx {
                            prompt: &prompt,
                            history: &[],
                        })
                        .await;
                    println!(
                        "mode={} confidence={:.2} reason={}",
                        v.mode.as_str(),
                        v.confidence,
                        v.reason
                    );
                    Ok(None)
                }
                None => anyhow::bail!(
                "mode detection is not enabled — set `[mode] classifier = \"hybrid\"` in the config"
            ),
            },
            Mode::ServeMcp => mcp_server::serve(&agent).await.map(|()| None),
            Mode::ServeGrpc(..) => {
                let (seam, listen) = serve_grpc.expect("serve target resolved above");
                grpc_server::serve(&agent, seam, listen)
                    .await
                    .map(|()| None)
            }
            Mode::ServeGrpcAll(..) => {
                let listen = serve_grpc_all_listen.expect("gateway target resolved above");
                grpc_server::serve_all(&agent, listen).await.map(|()| None)
            }
            Mode::ServeSessions(..) => {
                let listen = serve_sessions_listen.expect("sessions target resolved above");
                grpc_server::serve_sessions(agent.clone(), listen)
                    .await
                    .map(|()| None)
            }
            Mode::ServeFleet(..) => {
                let listen = serve_fleet_listen.expect("fleet target resolved above");
                let preflight = fleet_preflight.expect("preflight built for ServeFleet above");
                grpc_server::serve_fleet(agent.clone(), listen, preflight)
                    .await
                    .map(|()| None)
            }
            // Handled before the run: the build above validated the config and we
            // already returned.
            Mode::CheckConfig => unreachable!("--check-config returns before the run"),
            Mode::Doctor => unreachable!("doctor returns before the run"),
            // `plan` / `run --once` / `run`: the planner over the store opened above
            // and the agent's planner provider (`[campaign] planner_model`, else the
            // main one), with the worktree brief and touch resolver rooted at
            // `[campaign] repo_root` (else `[agent] working_dir`). The driver verbs
            // build one planner per tenant per tick from the same parts.
            Mode::Campaign(args) => {
                let CampaignRun {
                    ctx,
                    cfg,
                    repo_root,
                    main_model,
                    backend,
                    tenants,
                    worker,
                    config_path,
                } = campaign_run.expect("the campaign store is opened before the build");
                let provider = agent.campaign_planner_provider();
                let brief: std::sync::Arc<dyn agent_campaign::BriefSource> =
                    std::sync::Arc::new(agent_campaign::FallbackBrief::new(&repo_root));
                let touches: std::sync::Arc<dyn agent_campaign::TouchResolver> =
                    std::sync::Arc::new(agent_campaign::WorktreeTouches::new(&repo_root));
                let label = cfg.planner_label(&main_model).to_string();
                let max_repairs = cfg.max_repairs;
                let stdout = std::io::stdout();
                match &args.cmd {
                    campaign_cli::CampaignCmd::RunOnce | campaign_cli::CampaignCmd::Run => {
                        let factory: agent_campaign::PlannerFactory =
                            std::sync::Arc::new(move |store| {
                                agent_campaign::Planner::draft07(
                                    store,
                                    provider.clone(),
                                    brief.clone(),
                                    touches.clone(),
                                    label.clone(),
                                )
                                .with_max_repairs(max_repairs)
                            });
                        let backend =
                            backend.expect("the backend is opened for the driver verbs");
                        let planner = std::sync::Arc::new(agent_campaign::FactoryPlanner(factory));
                        if args.cmd == campaign_cli::CampaignCmd::RunOnce {
                            // One tick over the one tenant `ctx.store` is bound to,
                            // `enabled` forced on (an explicit human action).
                            let once = agent_runtime::CampaignCfg {
                                enabled: true,
                                ..cfg
                            };
                            let driver = agent_runtime::campaign_driver::build_driver(
                                &once,
                                backend,
                                agent_campaign::Tenants::Fixed(vec![ctx
                                    .store
                                    .tenant()
                                    .to_string()]),
                                planner,
                                agent.forge(),
                                agent_runtime::campaign_driver::WorkerDeps {
                                    agent: agent.clone(),
                                    agent_bin: std::env::current_exe().ok(),
                                    config_path,
                                    worker,
                                },
                            )?;
                            let mut out = stdout.lock();
                            return campaign_cli::run_driver_once(&ctx, &driver, &mut out)
                                .await
                                .map(|()| None);
                        }
                        // The resident loop, mirroring `--scheduler`: tick every
                        // `tick_secs` until interrupted, then drain the workers.
                        let driver = agent_runtime::campaign_driver::build_driver(
                            &cfg,
                            backend,
                            tenants,
                            planner,
                            agent.forge(),
                            agent_runtime::campaign_driver::WorkerDeps {
                                agent: agent.clone(),
                                agent_bin: std::env::current_exe().ok(),
                                config_path,
                                worker,
                            },
                        )?;
                        let every = Duration::from_secs(cfg.tick_secs.max(1));
                        eprintln!(
                            "campaign: ticking every {}s — ^C to stop",
                            every.as_secs()
                        );
                        loop {
                            tokio::select! {
                                () = tokio::time::sleep(every) => {
                                    let report = driver.tick().await;
                                    let mut out = stdout.lock();
                                    campaign_cli::render_resident_tick(&report, &mut out)?;
                                }
                                sig = shutdown::signal() => {
                                    eprintln!("\n{sig} — stopping the campaign driver");
                                    break;
                                }
                            }
                        }
                        let drained = driver
                            .drain(Duration::from_secs(cfg.worker_timeout_secs))
                            .await;
                        if drained.aborted > 0 {
                            eprintln!(
                                "campaign: {} worker(s) aborted; their leases expire and reap returns the leaves",
                                drained.aborted
                            );
                        }
                        Ok(None)
                    }
                    _ => {
                        let planner = agent_campaign::Planner::draft07(
                            ctx.store.clone(),
                            provider,
                            brief,
                            touches,
                            label,
                        )
                        .with_max_repairs(max_repairs);
                        let mut out = stdout.lock();
                        campaign_cli::run_plan(
                            &ctx,
                            &planner,
                            cfg.plan_per_tick,
                            &args.cmd,
                            &mut out,
                        )
                        .await
                        .map(|()| None)
                    }
                }
            }
            // The campaign worker (CP-06b, `04-executor.md` "Worker protocol"): one
            // leaf, claimed for this owner by the dispatching driver, run to its own
            // terminal state on this tenant's store view. The exit code is the
            // protocol's (0 completed, 1 failed, 3 lease lost); nothing is printed.
            Mode::RunTask { tenant, task } => {
                let RunTaskRun {
                    store,
                    worker,
                    owner,
                    index_dirs,
                } = run_task.expect("the campaign store is opened before the build");
                let exit = agent_runtime::campaign_worker::run_leaf(
                    &agent, store, &tenant, task, &owner, &worker,
                )
                .await;
                // The child's disposable index dirs (best effort; a leftover is
                // harmless and overwritten by the next attempt).
                for dir in index_dirs {
                    let _ = std::fs::remove_dir_all(dir);
                }
                run_task_exit = Some(exit.code());
                Ok(None)
            }
            Mode::Login { .. } | Mode::Logout { .. } | Mode::WhoAmI { .. } | Mode::Token { .. } => {
                unreachable!("login/logout/whoami/token return before the run")
            }
        }
    })
    .await;

    // Remove this session's disposable worktrees on every exit path (best-effort),
    // so an aborted or finished run doesn't leave them orphaned on disk.
    agent.cleanup().await;

    // Flush telemetry + push metrics before surfacing success/failure.
    if let Some(handle) = &telemetry {
        handle.shutdown().await;
    }
    if let Some(guard) = otel_guard {
        guard.shutdown(); // flush any pending OTLP spans to the collector
    }
    if metrics_cfg.enabled && !metrics_cfg.pushgateway.is_empty() {
        metrics_server::push(&metrics, &metrics_cfg.pushgateway, &metrics_cfg.job).await;
    }

    if let Some(code) = run_task_exit {
        // The worker's exit code is the protocol; flushed above, exit now.
        outcome?;
        std::process::exit(code);
    }
    if let Some(answer) = outcome? {
        println!("\n=== ANSWER ===\n{answer}");
        if !session_id.is_empty() {
            println!("\n(telemetry session_id: {session_id})");
        }
    }
    Ok(())
}

/// Turn the parsed resume flag into a loaded `(id, transcript)`, if any.
fn resolve_resume(
    resume: &Option<ResumeArg>,
    dir: &std::path::Path,
) -> Option<(String, Vec<agent_core::Message>)> {
    let id = match resume {
        Some(ResumeArg::Continue) => session_store::most_recent(dir)?,
        Some(ResumeArg::Id(id)) => id.clone(),
        None => return None,
    };
    match session_store::load(dir, &id) {
        Ok(msgs) => Some((id, msgs)),
        Err(e) => {
            eprintln!("could not resume session `{id}`: {e}");
            None
        }
    }
}

/// Host the `AgentSessionService` alongside a running loop when a `listen` is set,
/// else a future that never resolves (docs/design/portal). Meant as a
/// `tokio::select!` branch next to the run: it shares `&agent` (a second shared
/// borrow, alongside the session's), and is dropped — stopping the server — when
/// the run's branch wins.
async fn run_observe(
    agent: &agent_runtime::Agent,
    listen: Option<agent_grpc::Endpoint>,
) -> Result<()> {
    match listen {
        Some(ep) => grpc_server::serve_session_observe(agent, ep).await,
        None => std::future::pending().await,
    }
}

/// Metrics settings captured before `config` is moved into `build_agent`.
struct MetricsRun {
    enabled: bool,
    pushgateway: String,
    job: String,
}

/// Install the fmt layer, plus (each opt-in) the ClickHouse log layer when
/// telemetry + `stream_logs` are on, and the OTLP trace layer when `otel` is set.
/// Returns the OTLP guard (if any) so the caller can flush spans at shutdown.
fn init_tracing(
    telemetry: &Option<TelemetryHandle>,
    stream_logs: bool,
    otel: Option<OtelConfig>,
) -> Option<OtelGuard> {
    // A fresh `RUST_LOG`-derived filter per call site (EnvFilter isn't `Clone`).
    let env_filter =
        || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let ch_layer = telemetry
        .as_ref()
        .filter(|_| stream_logs)
        .map(|h| ClickHouseLayer::new(h.clone()));
    // Build the OTLP layer (and its guard) up front; on exporter-build failure we
    // log and carry on without it rather than abort the run.
    let (otel_layer, otel_guard) = match otel {
        Some(cfg) => match agent_telemetry::otlp_layer(&cfg) {
            Ok((layer, guard)) => (Some(layer), Some(guard)),
            Err(e) => {
                eprintln!("OTLP exporter init failed ({e}); continuing without OTLP tracing");
                (None, None)
            }
        },
        None => (None, None),
    };
    // Per-layer filters so each sink is independent. Console (stderr, to keep stdout
    // clean for the answer / the `--serve-mcp` JSON-RPC channel) and the ClickHouse
    // log layer respect `RUST_LOG`; the OTLP trace layer always captures `INFO`+
    // spans — otherwise `RUST_LOG=warn` (a common way to quiet the console) would
    // silently drop every span and disable distributed tracing.
    Registry::default()
        .with(
            fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr)
                .with_filter(env_filter()),
        )
        .with(ch_layer.map(|l| l.with_filter(env_filter())))
        .with(otel_layer.map(|l| l.with_filter(tracing_subscriber::filter::LevelFilter::INFO)))
        .init();
    otel_guard
}

enum Mode {
    OneShot(String),
    Repl,
    ServeMcp,
    /// Host one seam over gRPC (`--serve-<seam>`), with an optional listen override.
    ServeGrpc(grpc_server::Seam, Option<String>),
    /// Host **every** seam over gRPC from one process (`--serve-all`), with an
    /// optional listen override. Seams whose impl is disabled are skipped.
    ServeGrpcAll(Option<String>),
    /// Host the opt-in **sessions gateway** (`--serve-sessions`): the
    /// `SessionRegistryService` + a *driving* `AgentSessionService` (the `Send` RPC)
    /// + the idle-GC reaper. `--serve-mcp`-class; loopback/UDS-gated (docs/design/portal).
    ServeSessions(Option<String>),
    /// Host the full **review fleet** (`--serve-fleet`, review-fleet C1): the roster
    /// control plane (`ReviewFleetService` incl. `ReviewNow`) + the orchestrator that
    /// reconciles enabled rows into capped sessions and drives queued PRs, plus the
    /// driving `AgentSessionService` + reaper. Distinct from the bare Fleet *seam*
    /// (roster CRUD only) served inside `--serve-all`.
    ServeFleet(Option<String>),
    /// Drive the scheduler: tick on an interval, firing due jobs (parity spec 28).
    Scheduler,
    /// Run **one** scheduled job as a headless per-tenant turn and exit (scheduler
    /// S2b). This is the child the tenant-fanning driver spawns under the `Sandbox`
    /// seam when `[scheduler] sandbox_dispatch` is on: it scopes the run to
    /// `--tenant` (so it reads that tenant's per-tenant seams and resolves only that
    /// tenant's secrets), runs the goal via `agent.run`, prints the answer, and exits
    /// non-zero on failure. The tenant is validated fail-closed — an invalid segment
    /// refuses to run, never falling back to `local`.
    RunScheduledJob {
        tenant: String,
        goal: String,
    },
    /// Collect grounded review facts for a target and print them
    /// (`agent --review <PR#|branch|.>`). The bool is `--gate`: exit non-zero if the
    /// synthesized risk crosses the configured threshold. See docs/design/code-review/.
    Review(agent_core::ReviewTarget, bool),
    /// Classify a prompt's task mode and print the verdict (`agent --detect-mode
    /// "<prompt>"`). A thin, offline debug surface for the general mode detector —
    /// the deterministic prefilter needs no model. See docs/design/adaptive-cognition/.
    Detect(String),
    /// Validate the config end to end without running (`agent --check-config`):
    /// load it through the real loader, build the agent so every selected seam impl
    /// must resolve, print the chosen impls, and exit 0. A dry run for CI /
    /// operators — no model, no network. Backs the `config-roundtrip` check.
    CheckConfig,
    /// Run operational health probes and print a report (`agent doctor`): the agent
    /// determines its own state — config selections, ClickHouse liveness, provider-key
    /// resolvability — and exits non-zero iff a required probe failed. On-demand,
    /// unlike `--check-config` it *does* dial the network. See docs/design/doctor/.
    Doctor,
    /// `agent campaign <verb> …` (docs/design/campaigns, CP-04): the human-facing
    /// verbs over the campaign store. Store-only verbs run before metrics and the
    /// agent build, like `doctor`; `plan` / `run --once` / `run` build the agent
    /// for the planner's provider. The bare word `campaign` selects this only as
    /// the first non-option token — after `--` it is a goal word like any other.
    Campaign(campaign_cli::CampaignArgs),
    /// `agent --run-task --tenant T --task <id>` (campaigns CP-06b): the worker
    /// subprocess the campaign driver dispatches, hidden like
    /// `--run-scheduled-job`. The owner token is checked before the config is read
    /// (`campaign_cli::run_task_owner`); the leaf then runs through
    /// `agent_runtime::campaign_worker::run_leaf` and the process exits with the
    /// protocol's code. The tenant is validated fail-closed at parse time; the
    /// task is an id, never a listing letter.
    RunTask {
        tenant: String,
        task: agent_core::campaign::TaskId,
    },
    /// Sign in (`agent login [--browser] [--issuer NAME] [--endpoint ADDR]`): the
    /// device flow at the login issuer, or with `--browser` the authorization-code
    /// flow with a loopback redirect (S21), then an agent token kept in
    /// `$XDG_CONFIG_HOME/agent-seddon/tokens/<issuer>.json`
    /// (docs/design/security-hardening/01-authentication.md "CLI").
    #[cfg_attr(not(feature = "auth"), allow(dead_code))]
    Login {
        issuer: Option<String>,
        endpoint: Option<String>,
        browser: bool,
    },
    /// Revoke the stored login's session and forget it (`agent logout`).
    #[cfg_attr(not(feature = "auth"), allow(dead_code))]
    Logout {
        issuer: Option<String>,
    },
    /// Print who the stored login is (`agent whoami`).
    #[cfg_attr(not(feature = "auth"), allow(dead_code))]
    WhoAmI {
        issuer: Option<String>,
    },
    /// Print the stored login's agent token, refreshed when stale (`agent token
    /// [--issuer NAME] [--json]`, security-hardening S23).
    #[cfg_attr(not(feature = "auth"), allow(dead_code))]
    Token {
        issuer: Option<String>,
        json: bool,
    },
}

/// What a planner verb (`plan`, `run --once`, `run`) carries from the config load
/// to the `scope` arm, since `config` is consumed by the builder in between.
struct CampaignRun {
    ctx: campaign_cli::CampaignCtx,
    cfg: agent_runtime::CampaignCfg,
    repo_root: PathBuf,
    main_model: String,
    /// The multi-tenant backend, opened for the driver verbs only.
    backend: Option<std::sync::Arc<dyn agent_core::campaign::CampaignBackend>>,
    /// The tenants the resident driver serves (`--tenant`, discovery, or `local`).
    tenants: agent_campaign::Tenants,
    /// The worker knobs (`[campaign] worker_timeout_secs` / `target_branch`,
    /// `[forge] dry_run`, `[git] push_policy`), captured before `config` moves.
    worker: agent_runtime::campaign_worker::WorkerCfg,
    /// The `--config` path a `sandbox = "subprocess"` worker child re-reads.
    config_path: Option<PathBuf>,
}

/// What `agent --run-task` keeps from the config load for the worker arm (CP-06b).
struct RunTaskRun {
    /// The store bound to the `--tenant` view.
    store: std::sync::Arc<dyn agent_core::campaign::CampaignStore>,
    worker: agent_runtime::campaign_worker::WorkerCfg,
    /// The owner token from `AGENT_CAMPAIGN_OWNER`, already a path-safe segment.
    owner: agent_core::campaign::Owner,
    /// This child's own search index dirs (`campaign_worker::isolate_indexes`),
    /// removed once the leaf has run.
    index_dirs: Vec<PathBuf>,
}

/// The seam impls a config selects — captured before `Config` is consumed by the
/// builder, then printed by `--check-config` once the build proves they resolve.
struct ConfigSelections {
    provider: String,
    context: String,
    policy: String,
    memory: String,
    tokenizer: String,
    search: String,
    tools: usize,
    /// `[campaign] store` as `off` / `postgres` (docs/design/campaigns, CP-04).
    campaign: &'static str,
    /// `[role] store` via [`role_label`]: `off`, or the store name with a note that
    /// the catalog was not loaded (the build ran in `CheckConfig` mode).
    role: String,
}

/// Render the `[role] store` selection for `--check-config`: `""` (no store, the
/// built-in catalog) prints as `off`; any store name prints as-is plus a note
/// that the catalog read was deferred, since `--check-config` builds in
/// [`agent_runtime::BuildMode::CheckConfig`] and never authorizes a request.
fn role_label(store: &str) -> String {
    let store = store.trim();
    if store.is_empty() {
        "off".to_string()
    } else {
        format!("{store} (catalog not loaded: --check-config)")
    }
}

/// Parse a `--review` target: `<base>..<head>` ⇒ an explicit revision range;
/// all-digits ⇒ a PR number; `.`/`worktree`/`HEAD` ⇒ the working tree (current
/// branch vs default); otherwise a branch name.
fn parse_review_target(s: &str) -> agent_core::ReviewTarget {
    let t = s.trim();
    if let Some((base, head)) = t.split_once("..") {
        agent_core::ReviewTarget::Revs {
            base: base.to_string(),
            head: head.to_string(),
        }
    } else if t.is_empty()
        || t == "."
        || t.eq_ignore_ascii_case("worktree")
        || t.eq_ignore_ascii_case("head")
    {
        agent_core::ReviewTarget::WorkingTree
    } else if let Ok(n) = t.parse::<u64>() {
        agent_core::ReviewTarget::Pr(n)
    } else {
        agent_core::ReviewTarget::Branch(t.to_string())
    }
}

enum ResumeArg {
    Continue,
    Id(String),
}

struct Args {
    config_path: PathBuf,
    mode: Mode,
    resume: Option<ResumeArg>,
    /// `--cognition-graph FILE`: run with this cognition-graph document
    /// (equivalent to `[graph] store = "file", file = FILE`).
    cognition_graph: Option<String>,
    /// `--model-router-config FILE`: load the task-router fleet + policy from
    /// this textproto scenario file (equivalent to `[agent] model_router_config`).
    model_router_config: Option<String>,
}

fn parse_args() -> Result<Args> {
    parse_args_from(std::env::args().skip(1))
}

/// The arg-parsing core, taking the args explicitly so it is unit-testable (the
/// scheduler-S2b `--` end-of-options handling in particular). `parse_args` calls it
/// with the real process args.
fn parse_args_from(args: impl Iterator<Item = String>) -> Result<Args> {
    let mut config_path = PathBuf::from("config/agent.toml");
    let mut resume: Option<ResumeArg> = None;
    let mut scheduler_mode = false;
    let mut run_scheduled_job = false;
    let mut run_task = false;
    let mut task: Option<String> = None;
    let mut tenant: Option<String> = None;
    let mut serve_mcp = false;
    let mut serve_grpc: Option<grpc_server::Seam> = None;
    let mut serve_grpc_all = false;
    let mut serve_sessions = false;
    let mut serve_fleet = false;
    let mut listen: Option<String> = None;
    let mut review_target: Option<String> = None;
    let mut review_gate = false;
    let mut detect_mode_prompt: Option<String> = None;
    let mut check_config = false;
    let mut doctor = false;
    let mut campaign: Option<campaign_cli::CampaignArgs> = None;
    let mut login = false;
    let mut logout = false;
    let mut whoami = false;
    let mut token = false;
    let mut json = false;
    let mut issuer: Option<String> = None;
    let mut auth_endpoint: Option<String> = None;
    let mut browser = false;
    let mut cognition_graph: Option<String> = None;
    let mut model_router_config: Option<String> = None;
    let mut goal_parts: Vec<String> = Vec::new();
    // Once `--` is seen, every remaining token is a positional goal word, never a
    // flag — so a model-authored (untrusted) scheduled-job goal that looks like a
    // flag (`--serve-mcp`, `doctor`) can never hijack the child's mode (scheduler
    // S2b passes the goal after `--`).
    let mut end_of_opts = false;

    let mut args = args;
    while let Some(arg) = args.next() {
        if end_of_opts {
            goal_parts.push(arg);
            continue;
        }
        match arg.as_str() {
            "--" => end_of_opts = true,
            "--config" | "-c" => {
                config_path =
                    PathBuf::from(args.next().context("--config requires a path argument")?);
            }
            "--continue" => resume = Some(ResumeArg::Continue),
            "--resume" => {
                resume = Some(ResumeArg::Id(
                    args.next().context("--resume requires a session id")?,
                ));
            }
            "--scheduler" => scheduler_mode = true,
            "--run-scheduled-job" => run_scheduled_job = true,
            // The campaign worker mode (CP-05 stub, CP-06 body), hidden like
            // `--run-scheduled-job`; unreachable after `--`.
            "--run-task" => run_task = true,
            "--task" => {
                task = Some(args.next().context("--task requires a task id")?);
            }
            "--tenant" => {
                tenant = Some(args.next().context("--tenant requires a tenant segment")?);
            }
            "--check-config" => check_config = true,
            // Bare `doctor` subcommand (or `--doctor`); the bare word must be an
            // explicit arm so the `_` catch-all below doesn't swallow it as a goal.
            "doctor" | "--doctor" => doctor = true,
            // Bare `campaign` subcommand: everything after the word belongs to the
            // verb parser (so `--title` is never swallowed as a goal word). Unreachable
            // after `--` — the `end_of_opts` branch above runs first — so a
            // scheduled-job goal can never turn a child into a campaign verb.
            "campaign" => {
                let parsed = campaign_cli::parse(&mut args)?;
                if parsed.cmd == campaign_cli::CampaignCmd::Help {
                    println!("{}", campaign_cli::USAGE);
                    std::process::exit(0);
                }
                campaign = Some(parsed);
                break;
            }
            // `agent login` / `logout` / `whoami`: bare words, like `doctor`.
            "login" => login = true,
            "logout" => logout = true,
            "whoami" => whoami = true,
            "token" => token = true,
            "--json" => json = true,
            "--issuer" => {
                issuer = Some(args.next().context("--issuer requires an issuer name")?);
            }
            "--endpoint" => {
                auth_endpoint = Some(args.next().context("--endpoint requires an address")?);
            }
            "--browser" => browser = true,
            "--serve-mcp" => serve_mcp = true,
            "--serve-all" => serve_grpc_all = true,
            "--serve-sessions" => serve_sessions = true,
            // Intercept `--serve-fleet` here so it means the **full fleet process**
            // (orchestrator + reconcile), not the bare roster-CRUD seam the generic
            // `Seam::from_flag` arm below would select. The Fleet seam still serves
            // inside `--serve-all` (docs/design/review-fleet/03-fleet-core.md).
            "--serve-fleet" => serve_fleet = true,
            "--review" => {
                review_target = Some(
                    args.next()
                        .context("--review requires a target (a PR#, a branch, or `.`)")?,
                );
            }
            "--gate" => review_gate = true,
            "--detect-mode" => {
                detect_mode_prompt = Some(
                    args.next()
                        .context("--detect-mode requires a prompt argument")?,
                );
            }
            "--listen" => {
                listen = Some(args.next().context("--listen requires an address")?);
            }
            "--cognition-graph" => {
                cognition_graph = Some(
                    args.next()
                        .context("--cognition-graph requires a textproto file path")?,
                );
            }
            "--model-router-config" => {
                model_router_config = Some(
                    args.next()
                        .context("--model-router-config requires a textproto file path")?,
                );
            }
            flag if grpc_server::Seam::from_flag(flag).is_some() => {
                serve_grpc = grpc_server::Seam::from_flag(flag);
            }
            "--help" | "-h" => {
                println!(
                    "usage: agent [--config PATH] [--continue | --resume ID | --serve-mcp | --serve-<seam>] [<goal words...>]\n\
                     \n\
                     With a goal: run it once. Without a goal: interactive REPL.\n  \
                     --continue          resume the most recent saved session\n  \
                     --resume ID         resume a specific session\n  \
                     --scheduler         drive scheduled jobs (ticks until interrupted)\n  \
                     --run-scheduled-job run one scheduled job as a headless turn scoped to --tenant, then exit\n  \
                     --tenant SEG        with --run-scheduled-job: the owning tenant to scope the run to\n  \
                     --review TARGET     collect + print grounded review facts (TARGET = PR#, branch, or `.`)\n  \
                     --gate              with --review: exit non-zero if risk ≥ the configured threshold\n  \
                     --detect-mode P     classify prompt P's task mode and print the verdict\n  \
                     --check-config      load + validate the config, print the selected impls, and exit\n  \
                     doctor              run operational health probes (config, ClickHouse, provider key) and exit non-zero on failure\n  \
                     campaign <verb> …   manage campaigns — add / plan / list / show / approve / answer … (`agent campaign --help`)\n  \
                     login              sign in at the login issuer (device code) and keep an agent token [--issuer NAME] [--endpoint ADDR]\n  \
                     login --browser    sign in through a browser (loopback redirect) instead of a device code\n  \
                     logout              revoke the stored login's session and forget it [--issuer NAME]\n  \
                     whoami              print the stored login's tenant, roles and permissions [--issuer NAME]\n  \
                     token               print the stored login's agent token, refreshed when stale [--issuer NAME] [--json]\n  \
                     --serve-mcp         run as an MCP server over stdio (exposes a `run` tool)\n  \
                     --serve-<seam>      host one seam over gRPC; <seam> = {seams}\n  \
                     --serve-all         host every enabled seam over gRPC from one process\n  \
                     --serve-sessions    host the sessions gateway (SessionRegistry + driving AgentSession + reaper)\n  \
                     --serve-fleet       host the review fleet (roster control plane + orchestrator + reconcile)\n  \
                     --listen ADDR       override the gRPC listen address (host:port or unix:/path)\n  \
                     --cognition-graph F run with cognition-graph document F (see config/cognition/)\n  \
                     --model-router-config F  load the task-router fleet+policy from textproto F (see config/model-router/)",
                    seams = grpc_server::Seam::flag_names()
                );
                std::process::exit(0);
            }
            _ => goal_parts.push(arg),
        }
    }

    let goal = goal_parts.join(" ");
    if [login, logout, whoami, token]
        .iter()
        .filter(|b| **b)
        .count()
        > 1
    {
        anyhow::bail!("pick one of `login`, `logout`, `whoami`, `token`");
    }
    if auth_endpoint.is_some() && !login {
        anyhow::bail!("--endpoint only applies to `agent login`");
    }
    if browser && !login {
        anyhow::bail!("--browser only applies to `agent login`");
    }
    if issuer.is_some() && !(login || logout || whoami || token) {
        anyhow::bail!("--issuer only applies to `login`, `logout`, `whoami` and `token`");
    }
    if json && !token {
        anyhow::bail!("--json only applies to `agent token`");
    }
    let mode = if login {
        Mode::Login {
            issuer,
            endpoint: auth_endpoint,
            browser,
        }
    } else if logout {
        Mode::Logout { issuer }
    } else if whoami {
        Mode::WhoAmI { issuer }
    } else if token {
        Mode::Token { issuer, json }
    } else if check_config {
        Mode::CheckConfig
    } else if doctor {
        Mode::Doctor
    } else if let Some(mut c) = campaign {
        // `--tenant` before the `campaign` word binds the same as after it; the
        // verb parser validated its own, this one is validated here.
        if let (None, Some(t)) = (&c.tenant, tenant) {
            if !agent_core::safe_segment(&t) {
                anyhow::bail!("--tenant `{}` is not a path-safe segment", {
                    let cut: String = t.chars().take(40).collect();
                    agent_campaign::display::escape_terminal(&cut)
                });
            }
            c.tenant = Some(t);
        }
        Mode::Campaign(c)
    } else if run_task {
        // Fail closed on both arguments: a worker never runs un-scoped, and a task
        // is an id (a letter is a listing position, meaningless to a subprocess).
        let tenant = tenant.context("--run-task requires --tenant <segment>")?;
        if !agent_core::safe_segment(&tenant) {
            anyhow::bail!("--tenant `{}` is not a valid tenant segment", {
                let cut: String = tenant.chars().take(40).collect();
                agent_campaign::display::escape_terminal(&cut)
            });
        }
        let task = campaign_cli::parse_task_id(&task.context("--run-task requires --task <id>")?)?;
        Mode::RunTask { tenant, task }
    } else if task.is_some() {
        anyhow::bail!("--task only applies to `--run-task`");
    } else if scheduler_mode {
        Mode::Scheduler
    } else if run_scheduled_job {
        let tenant = tenant.context("--run-scheduled-job requires --tenant <segment>")?;
        if goal.trim().is_empty() {
            anyhow::bail!("--run-scheduled-job requires a goal");
        }
        Mode::RunScheduledJob { tenant, goal }
    } else if serve_grpc_all {
        Mode::ServeGrpcAll(listen)
    } else if serve_sessions {
        Mode::ServeSessions(listen)
    } else if serve_fleet {
        Mode::ServeFleet(listen)
    } else if let Some(seam) = serve_grpc {
        Mode::ServeGrpc(seam, listen)
    } else if serve_mcp {
        Mode::ServeMcp
    } else if let Some(t) = review_target {
        Mode::Review(parse_review_target(&t), review_gate)
    } else if let Some(p) = detect_mode_prompt {
        Mode::Detect(p)
    } else if goal.trim().is_empty() {
        Mode::Repl
    } else {
        Mode::OneShot(goal)
    };
    Ok(Args {
        config_path,
        mode,
        resume,
        cognition_graph,
        model_router_config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Args> {
        parse_args_from(argv.iter().map(|s| (*s).to_string()))
    }

    // desc: a flag-like scheduled-job goal after `--` is captured as the goal, never
    // interpreted as a flag — the argv-injection guard (scheduler S2b). Without this,
    // a model-authored goal of `--serve-mcp` would hijack the child into a server.
    #[test]
    fn positive_double_dash_makes_flag_like_goal_positional() {
        let args = parse(&[
            "--run-scheduled-job",
            "--tenant",
            "acme",
            "--",
            "--serve-mcp",
        ])
        .unwrap();
        match args.mode {
            Mode::RunScheduledJob { tenant, goal } => {
                assert_eq!(tenant, "acme");
                assert_eq!(goal, "--serve-mcp");
            }
            _ => panic!("expected RunScheduledJob"),
        }
    }

    // desc: every token after `--` joins the goal; flag-looking words among them are
    // inert (the general one-shot path benefits too).
    #[test]
    fn positive_double_dash_collects_following_words_as_goal() {
        let args = parse(&["--", "summarise", "--the", "logs"]).unwrap();
        match args.mode {
            Mode::OneShot(goal) => assert_eq!(goal, "summarise --the logs"),
            _ => panic!("expected OneShot"),
        }
    }

    // desc: the bare word `campaign` as the first non-option token selects the
    // campaign mode and hands every later token to the verb parser — including
    // flag-looking ones the goal catch-all would otherwise swallow.
    #[test]
    fn positive_campaign_word_selects_campaign_mode() {
        let args = parse(&[
            "--config", "x.toml", "campaign", "add", "--repo", "1", "--title", "t", "--goal", "g",
        ])
        .unwrap();
        assert_eq!(args.config_path, PathBuf::from("x.toml"));
        match args.mode {
            Mode::Campaign(c) => match c.cmd {
                campaign_cli::CampaignCmd::Add(add) => assert_eq!(add.title, "t"),
                other => panic!("expected add, got {other:?}"),
            },
            _ => panic!("expected Campaign"),
        }
    }

    // desc: `--tenant` before the `campaign` word binds like `--tenant` after it.
    #[test]
    fn corner_tenant_before_campaign_word_binds() {
        let args = parse(&["--tenant", "acme", "campaign", "list"]).unwrap();
        match args.mode {
            Mode::Campaign(c) => assert_eq!(c.tenant.as_deref(), Some("acme")),
            _ => panic!("expected Campaign"),
        }
    }

    // desc (adversarial): an unsafe `--tenant` before the word is refused, same as
    // the verb parser refuses one after it.
    #[test]
    fn adversarial_tenant_before_campaign_word_is_validated() {
        let err = parse(&["--tenant", "../x", "campaign", "list"])
            .err()
            .expect("refused");
        assert!(err.to_string().contains("not a path-safe segment"), "{err}");
    }

    // desc (adversarial): after `--` the word `campaign` is a goal word, never a
    // mode — a scheduled-job goal cannot reach the campaign verbs.
    #[test]
    fn adversarial_campaign_after_double_dash_is_a_goal() {
        let args = parse(&["--", "campaign", "cancel", "A"]).unwrap();
        match args.mode {
            Mode::OneShot(goal) => assert_eq!(goal, "campaign cancel A"),
            _ => panic!("expected OneShot"),
        }
    }

    // desc (negative): `--check-config` outranks the campaign word, as it does
    // every other mode.
    #[test]
    fn corner_check_config_outranks_campaign() {
        let args = parse(&["--check-config", "campaign", "list"]).unwrap();
        assert!(matches!(args.mode, Mode::CheckConfig));
    }

    // desc (negative): --run-scheduled-job without --tenant is refused (fail-closed —
    // a job must never run un-scoped).
    #[test]
    fn negative_run_scheduled_job_requires_tenant() {
        assert!(parse(&["--run-scheduled-job", "--", "some goal"]).is_err());
    }

    /// The parsed `--run-task` mode, for the table below.
    fn run_task_mode(argv: &[&str]) -> Result<String> {
        parse(argv).map(|a| match a.mode {
            Mode::RunTask { tenant, task } => format!("run-task {tenant} #{task}"),
            Mode::OneShot(goal) => format!("oneshot {goal}"),
            _ => "other".into(),
        })
    }

    // desc: `--run-task --tenant T --task <id>` (campaigns CP-05) parses like
    // `--run-scheduled-job`: both arguments required, the tenant a safe segment,
    // the task an id; after `--` the flags are goal words.
    #[rstest::rstest]
    #[case::positive_run_task(&["--run-task", "--tenant", "acme", "--task", "12"], "run-task acme #12")]
    #[case::positive_run_task_flag_order(&["--task", "7", "--tenant", "t", "--run-task"], "run-task t #7")]
    #[case::adversarial_run_task_after_double_dash_is_a_goal(
        &["--", "--run-task", "--tenant", "t", "--task", "1"],
        "oneshot --run-task --tenant t --task 1"
    )]
    fn run_task_parses(#[case] argv: &[&str], #[case] want: &str) {
        assert_eq!(run_task_mode(argv).expect("parses"), want);
    }

    #[rstest::rstest]
    #[case::negative_run_task_requires_tenant(&["--run-task", "--task", "1"], "requires --tenant")]
    #[case::negative_run_task_requires_task(&["--run-task", "--tenant", "t"], "requires --task")]
    #[case::negative_run_task_bad_task_id_zero(&["--run-task", "--tenant", "t", "--task", "0"], "is not a task ref")]
    #[case::negative_run_task_bad_task_id_word(&["--run-task", "--tenant", "t", "--task", "abc"], "is not a task ref")]
    #[case::negative_run_task_letter_task(&["--run-task", "--tenant", "t", "--task", "A"], "must be a task id")]
    #[case::adversarial_run_task_tenant_traversal(&["--run-task", "--tenant", "../x", "--task", "1"], "not a valid tenant segment")]
    #[case::adversarial_run_task_task_traversal(&["--run-task", "--tenant", "t", "--task", "../1"], "is not a task ref")]
    #[case::negative_task_without_run_task(&["--task", "1", "hello"], "only applies to")]
    #[case::boundary_task_missing_value(&["--run-task", "--tenant", "t", "--task"], "requires a task id")]
    fn run_task_refused(#[case] argv: &[&str], #[case] want: &str) {
        let err = run_task_mode(argv).expect_err("refused");
        let msg = format!("{err:#}");
        assert!(msg.contains(want), "{msg}");
        assert!(
            msg.len() < 300 && !msg.chars().any(char::is_control),
            "{msg:?}"
        );
    }

    // desc (adversarial): a bare flag-like word (no `--`) is still NOT a scheduled job
    // — only the explicit --run-scheduled-job flag selects that mode, so a goal alone
    // can never reach the scheduled-job path.
    #[test]
    fn adversarial_flag_like_goal_without_run_flag_is_not_scheduled_job() {
        let args = parse(&["--", "--run-scheduled-job"]).unwrap();
        match args.mode {
            Mode::OneShot(goal) => assert_eq!(goal, "--run-scheduled-job"),
            _ => panic!("expected OneShot (the token is a goal, not a mode)"),
        }
    }

    /// What the parsed mode is, for the login table below.
    fn login_mode(argv: &[&str]) -> Result<String> {
        parse(argv).map(|a| match a.mode {
            Mode::Login {
                issuer,
                endpoint,
                browser: false,
            } => format!("login {issuer:?} {endpoint:?}"),
            Mode::Login {
                issuer,
                endpoint,
                browser: true,
            } => format!("login --browser {issuer:?} {endpoint:?}"),
            Mode::Logout { issuer } => format!("logout {issuer:?}"),
            Mode::WhoAmI { issuer } => format!("whoami {issuer:?}"),
            Mode::Token { issuer, json } => format!("token {issuer:?} json={json}"),
            Mode::OneShot(goal) => format!("oneshot {goal}"),
            _ => "other".into(),
        })
    }

    // desc: `agent login` / `logout` / `whoami` (security-hardening S12) are bare
    // words like `doctor`; `--issuer` and `--endpoint` only go with them.
    #[rstest::rstest]
    #[case::positive_login(&["login"], "login None None")]
    #[case::positive_login_with_options(&["login", "--issuer", "google", "--endpoint", "https://a:1"], "login Some(\"google\") Some(\"https://a:1\")")]
    #[case::positive_logout(&["logout", "--issuer", "kc"], "logout Some(\"kc\")")]
    #[case::positive_whoami(&["whoami"], "whoami None")]
    #[case::positive_login_browser(&["login", "--browser"], "login --browser None None")]
    #[case::positive_token(&["token"], "token None json=false")]
    #[case::positive_token_json_issuer(&["token", "--json", "--issuer", "google"], "token Some(\"google\") json=true")]
    #[case::corner_browser_before_the_word(&["--browser", "login", "--issuer", "kc"], "login --browser Some(\"kc\") None")]
    #[case::corner_login_after_double_dash_is_a_goal(&["--", "login"], "oneshot login")]
    fn login_words_parse(#[case] argv: &[&str], #[case] want: &str) {
        assert_eq!(login_mode(argv).expect("parses"), want);
    }

    #[rstest::rstest]
    #[case::negative_two_words(&["login", "logout"], "pick one")]
    #[case::negative_endpoint_without_login(&["whoami", "--endpoint", "https://a:1"], "--endpoint only")]
    #[case::negative_issuer_alone(&["--issuer", "google", "hello"], "--issuer only")]
    #[case::negative_browser_without_login(&["whoami", "--browser"], "--browser only")]
    #[case::negative_json_without_token(&["whoami", "--json"], "--json only")]
    #[case::negative_token_and_login(&["token", "login"], "pick one")]
    #[case::boundary_issuer_missing_value(&["login", "--issuer"], "requires an issuer name")]
    fn login_words_refused(#[case] argv: &[&str], #[case] want: &str) {
        let err = login_mode(argv).expect_err("refused");
        assert!(format!("{err:#}").contains(want), "{err:#}");
    }

    // desc: the `role = …` line of `--check-config`: no store prints `off`; a
    // store prints its name plus the deferred-catalog note (the build ran in
    // `CheckConfig` mode, so nothing was read from it). The store name is echoed
    // as configured (it was already validated by the config loader), never
    // interpreted.
    #[rstest::rstest]
    #[case::positive_file("file", "file (catalog not loaded: --check-config)")]
    #[case::positive_postgres("postgres", "postgres (catalog not loaded: --check-config)")]
    #[case::corner_empty_is_off("", "off")]
    #[case::corner_whitespace_is_off("  ", "off")]
    #[case::boundary_trimmed(" postgres ", "postgres (catalog not loaded: --check-config)")]
    fn role_label_rows(#[case] store: &str, #[case] want: &str) {
        assert_eq!(role_label(store), want);
    }
}
