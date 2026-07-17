//! `agent` — run one goal through the agent loop.
//!
//! Usage:
//!   agent [--config PATH] <goal words...>
//!
//! Example:
//!   agent --config config/agent.toml "list the files in this repo"

use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .init();

    let Args { config_path, goal } = parse_args()?;
    if goal.trim().is_empty() {
        anyhow::bail!("no goal given.\nusage: agent [--config PATH] <goal words...>");
    }

    let toml_str = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading config `{}`", config_path.display()))?;
    let config = agent_runtime::parse_config(&toml_str).context("parsing config")?;

    let agent = agent_runtime::build_agent(config).await.context("building agent")?;

    tracing::info!(goal = %goal, "starting agent run");
    let answer = agent.run(&goal).await?;

    println!("\n=== ANSWER ===\n{answer}");
    Ok(())
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
                config_path = PathBuf::from(
                    args.next().context("--config requires a path argument")?,
                );
            }
            "--help" | "-h" => {
                println!("usage: agent [--config PATH] <goal words...>");
                std::process::exit(0);
            }
            _ => goal_parts.push(arg),
        }
    }

    Ok(Args { config_path, goal: goal_parts.join(" ") })
}
