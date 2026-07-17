//! `agent` — run one goal through the agent loop.
//!
//! Usage:
//!   agent [--config PATH] <goal words...>
//!
//! Example:
//!   agent --config config/agent.toml "list the files in this repo"

mod metrics_server;

use agent_runtime::Metrics;
use agent_telemetry::{ClickHouseLayer, TelemetryConfig, TelemetryHandle};
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter, Registry};

#[tokio::main]
async fn main() -> Result<()> {
    let Args { config_path, goal } = parse_args()?;
    if goal.trim().is_empty() {
        anyhow::bail!("no goal given.\nusage: agent [--config PATH] <goal words...>");
    }

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

    tracing::info!(goal = %goal, session_id = %session_id, "starting agent run");
    let agent = agent_runtime::build_agent(
        config,
        telemetry.clone(),
        session_id.clone(),
        metrics.clone(),
    )
    .await
    .context("building agent")?;
    let result = agent.run(&goal).await;

    // Flush telemetry + push metrics before we surface success/failure or exit.
    if let Some(handle) = &telemetry {
        handle.shutdown().await;
    }
    if metrics_cfg.enabled && !metrics_cfg.pushgateway.is_empty() {
        metrics_server::push(&metrics, &metrics_cfg.pushgateway, &metrics_cfg.job).await;
    }

    let answer = result?;
    println!("\n=== ANSWER ===\n{answer}");
    if !session_id.is_empty() {
        println!("\n(telemetry session_id: {session_id})");
    }
    Ok(())
}

/// Metrics settings captured before `config` is moved into `build_agent`.
struct MetricsRun {
    enabled: bool,
    pushgateway: String,
    job: String,
}

/// Install the fmt layer plus, when telemetry + `stream_logs` are on, the
/// ClickHouse layer. `Option<Layer>` is itself a `Layer`, so the same builder
/// covers both cases.
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

struct Args {
    config_path: PathBuf,
    goal: String,
}

fn parse_args() -> Result<Args> {
    let mut config_path = PathBuf::from("config/agent.toml");
    let mut goal_parts: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" | "-c" => {
                config_path =
                    PathBuf::from(args.next().context("--config requires a path argument")?);
            }
            "--help" | "-h" => {
                println!("usage: agent [--config PATH] <goal words...>");
                std::process::exit(0);
            }
            _ => goal_parts.push(arg),
        }
    }

    Ok(Args {
        config_path,
        goal: goal_parts.join(" "),
    })
}
