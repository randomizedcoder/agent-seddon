//! `agent` — a coding agent.
//!
//! Usage:
//!   agent [--config PATH] [--continue | --resume ID] [<goal words...>]
//!
//! With a goal, runs it once (one-shot). With no goal, enters an interactive
//! multi-turn REPL (see `repl.rs`). `--continue` resumes the most recent saved
//! session; `--resume ID` resumes a specific one.

mod metrics_server;
mod repl;

use agent_runtime::{session_store, Metrics};
use agent_telemetry::{ClickHouseLayer, TelemetryConfig, TelemetryHandle};
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

    init_tracing(&telemetry, config.telemetry.stream_logs);

    // Metrics (opt-in). Instrumentation always runs into this registry; serving
    // the /metrics endpoint and pushing are gated by config.
    let metrics = Metrics::new();
    if config.metrics.enabled {
        metrics_server::serve(metrics.clone(), &config.metrics.listen);
    }
    let metrics_cfg = MetricsRun {
        enabled: config.metrics.enabled,
        pushgateway: config.metrics.pushgateway.clone(),
        job: config.metrics.job.clone(),
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
            let result = session.send(&goal).await;
            if result.is_ok() {
                let _ = session_store::save(&sessions_dir, &id, session.messages());
            }
            result.map(Some)
        }
        Mode::Repl => repl::run(&agent, &sessions_dir, resumed)
            .await
            .map(|()| None),
    };

    // Flush telemetry + push metrics before surfacing success/failure.
    if let Some(handle) = &telemetry {
        handle.shutdown().await;
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

/// Install the fmt layer plus, when telemetry + `stream_logs` are on, the
/// ClickHouse layer.
fn init_tracing(telemetry: &Option<TelemetryHandle>, stream_logs: bool) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let ch_layer = telemetry
        .as_ref()
        .filter(|_| stream_logs)
        .map(|h| ClickHouseLayer::new(h.clone()));
    Registry::default()
        .with(env_filter)
        .with(fmt::layer().with_target(false))
        .with(ch_layer)
        .init();
}

enum Mode {
    OneShot(String),
    Repl,
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
            "--help" | "-h" => {
                println!(
                    "usage: agent [--config PATH] [--continue | --resume ID] [<goal words...>]\n\
                     \n\
                     With a goal: run it once. Without a goal: interactive REPL.\n  \
                     --continue     resume the most recent saved session\n  \
                     --resume ID    resume a specific session"
                );
                std::process::exit(0);
            }
            _ => goal_parts.push(arg),
        }
    }

    let goal = goal_parts.join(" ");
    let mode = if goal.trim().is_empty() {
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
