//! `agent` — a coding agent.
//!
//! Usage:
//!   agent [--config PATH] [--continue | --resume ID] [<goal words...>]
//!
//! With a goal, runs it once (one-shot). With no goal, enters an interactive
//! multi-turn REPL (see `repl.rs`). `--continue` resumes the most recent saved
//! session; `--resume ID` resumes a specific one.

mod grpc_server;
mod mcp_server;
mod metrics_server;
mod repl;

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
    } = parse_args()?;

    let toml_str = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading config `{}`", config_path.display()))?;
    // Parse with the ignored-key list in hand rather than letting `parse_config`
    // log it: this runs BEFORE `init_tracing` below, so a `tracing::warn!` here
    // would have no subscriber and be swallowed. The warnings are emitted once
    // tracing is up, further down.
    let (config, unknown_config_keys) =
        agent_runtime::parse_config_reporting_unknown(&toml_str).context("parsing config")?;
    // Captured before `config` is consumed by the builder.
    let cfg_tick_secs = config.scheduler.tick_secs;
    // Captured before `config` moves into `build_agent` (see the metrics note below).
    let review_budget = config.review.context_budget_bytes;

    // Telemetry (opt-in). Build the writer before installing tracing so the
    // ClickHouse layer can stream logs from the very first event.
    let (telemetry, session_id) = if config.telemetry.enabled {
        let session_id = uuid::Uuid::new_v4().to_string();
        let handle = TelemetryHandle::spawn(
            TelemetryConfig {
                addr: config.telemetry.clickhouse_url.clone(),
                database: config.telemetry.database.clone(),
                user: config.telemetry.user.clone(),
                password: config.telemetry.password.clone(),
                batch_max_rows: config.telemetry.batch_max_rows,
                flush_interval: Duration::from_millis(config.telemetry.flush_interval_ms),
            },
            session_id.clone(),
        );
        (Some(handle), session_id)
    } else {
        (None, String::new())
    };

    // OTLP tracing (opt-in, independent of the ClickHouse sink): enabled by a
    // non-empty `otlp_endpoint`. Tag spans with the run's session id when we have one.
    let otel_cfg = (!config.telemetry.otlp_endpoint.is_empty()).then(|| OtelConfig {
        endpoint: config.telemetry.otlp_endpoint.clone(),
        service_name: config.telemetry.otel_service_name.clone(),
        instance_id: (!session_id.is_empty()).then(|| session_id.clone()),
        headers: config.telemetry.otlp_headers.clone(),
    });
    let otel_guard = init_tracing(&telemetry, config.telemetry.stream_logs, otel_cfg);

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

    let sessions_dir = session_store::default_dir();
    tracing::info!(session_id = %session_id, "starting agent");
    let agent = agent_runtime::build_agent(
        config,
        telemetry.clone(),
        session_id.clone(),
        metrics.clone(),
    )
    .await
    .context("building agent")?;

    // Resolve an optional resume target to (id, transcript).
    let resumed = resolve_resume(&resume, &sessions_dir);

    // Run either one-shot or the REPL, capturing the answer (one-shot only).
    let outcome: Result<Option<String>> = match mode {
        Mode::OneShot(goal) => {
            let mut session = agent.session();
            let id = match resumed {
                Some((rid, msgs)) => {
                    session.load(msgs);
                    rid
                }
                None => repl::new_id(),
            };
            // Race the run against Ctrl-C: an interrupt should save the (partial)
            // transcript and clean up rather than killing the process and orphaning
            // state. Cancelling `send` drops its future; the working set it mutated
            // in place is still readable via `messages()`.
            let outcome: Result<Option<String>> = tokio::select! {
                r = session.send(&goal) => r.map(Some),
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\n^C — interrupted; saving session and cleaning up…");
                    Ok(None)
                }
            };
            // Always persist the transcript (success, error, or interrupt) so the
            // run is resumable with `--resume {id}` / `--continue`.
            if let Err(e) = session_store::save(&sessions_dir, &id, session.messages()) {
                tracing::warn!("could not save session `{id}`: {e}");
            } else {
                tracing::info!("session saved as `{id}`");
            }
            outcome
        }
        Mode::Repl => repl::run(&agent, &sessions_dir, resumed)
            .await
            .map(|()| None),
        Mode::Scheduler => {
            // Tick until interrupted. Each due job runs as a fresh headless turn,
            // and the scheduler's own overlap guard is what stops a slow job
            // stacking copies of itself.
            let every = std::time::Duration::from_secs(cfg_tick_secs.max(1));
            eprintln!("scheduler: ticking every {}s — ^C to stop", every.as_secs());
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(every) => {
                        let n = agent.tick_scheduler().await;
                        if n > 0 {
                            tracing::info!(jobs = n, "scheduler fired due jobs");
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        eprintln!("\n^C — stopping the scheduler");
                        break;
                    }
                }
            }
            Ok(None)
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
    };

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
    /// Drive the scheduler: tick on an interval, firing due jobs (parity spec 28).
    Scheduler,
    /// Collect grounded review facts for a target and print them
    /// (`agent --review <PR#|branch|.>`). The bool is `--gate`: exit non-zero if the
    /// synthesized risk crosses the configured threshold. See docs/design/code-review/.
    Review(agent_core::ReviewTarget, bool),
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
}

fn parse_args() -> Result<Args> {
    let mut config_path = PathBuf::from("config/agent.toml");
    let mut resume: Option<ResumeArg> = None;
    let mut scheduler_mode = false;
    let mut serve_mcp = false;
    let mut serve_grpc: Option<grpc_server::Seam> = None;
    let mut serve_grpc_all = false;
    let mut listen: Option<String> = None;
    let mut review_target: Option<String> = None;
    let mut review_gate = false;
    let mut goal_parts: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
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
            "--serve-mcp" => serve_mcp = true,
            "--serve-all" => serve_grpc_all = true,
            "--review" => {
                review_target = Some(
                    args.next()
                        .context("--review requires a target (a PR#, a branch, or `.`)")?,
                );
            }
            "--gate" => review_gate = true,
            "--listen" => {
                listen = Some(args.next().context("--listen requires an address")?);
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
                     --review TARGET     collect + print grounded review facts (TARGET = PR#, branch, or `.`)\n  \
                     --gate              with --review: exit non-zero if risk ≥ the configured threshold\n  \
                     --serve-mcp         run as an MCP server over stdio (exposes a `run` tool)\n  \
                     --serve-<seam>      host one seam over gRPC; <seam> = {seams}\n  \
                     --serve-all         host every enabled seam over gRPC from one process\n  \
                     --listen ADDR       override the gRPC listen address (host:port or unix:/path)",
                    seams = grpc_server::Seam::flag_names()
                );
                std::process::exit(0);
            }
            _ => goal_parts.push(arg),
        }
    }

    let goal = goal_parts.join(" ");
    let mode = if scheduler_mode {
        Mode::Scheduler
    } else if serve_grpc_all {
        Mode::ServeGrpcAll(listen)
    } else if let Some(seam) = serve_grpc {
        Mode::ServeGrpc(seam, listen)
    } else if serve_mcp {
        Mode::ServeMcp
    } else if let Some(t) = review_target {
        Mode::Review(parse_review_target(&t), review_gate)
    } else if goal.trim().is_empty() {
        Mode::Repl
    } else {
        Mode::OneShot(goal)
    };
    Ok(Args {
        config_path,
        mode,
        resume,
    })
}
