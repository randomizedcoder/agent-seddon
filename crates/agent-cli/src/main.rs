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
    let config = agent_runtime::parse_config(&toml_str).context("parsing config")?;

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

    // Metrics (opt-in). Instrumentation always runs into this registry; serving
    // the /metrics endpoint and pushing are gated by config.
    let metrics = Metrics::new();
    if config.metrics.enabled {
        // A `--serve-<seam>` process serves `/metrics` on that seam's dedicated
        // port so several co-located seam servers don't collide on `:9600`.
        let listen = match &mode {
            Mode::ServeGrpc(seam, _) => format!("127.0.0.1:{}", seam.metrics_port()),
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
        Mode::ServeMcp => mcp_server::serve(&agent).await.map(|()| None),
        Mode::ServeGrpc(..) => {
            let (seam, listen) = serve_grpc.expect("serve target resolved above");
            grpc_server::serve(&agent, seam, listen)
                .await
                .map(|()| None)
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
    let mut serve_mcp = false;
    let mut serve_grpc: Option<grpc_server::Seam> = None;
    let mut listen: Option<String> = None;
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
            "--serve-mcp" => serve_mcp = true,
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
                     --serve-mcp         run as an MCP server over stdio (exposes a `run` tool)\n  \
                     --serve-<seam>      host one seam over gRPC; <seam> = provider|memory|tools|context|policy|search|repo\n  \
                     --listen ADDR       override the gRPC listen address (host:port or unix:/path)"
                );
                std::process::exit(0);
            }
            _ => goal_parts.push(arg),
        }
    }

    let goal = goal_parts.join(" ");
    let mode = if let Some(seam) = serve_grpc {
        Mode::ServeGrpc(seam, listen)
    } else if serve_mcp {
        Mode::ServeMcp
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
