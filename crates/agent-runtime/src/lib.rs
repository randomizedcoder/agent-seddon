//! `agent-runtime` — wires the seams and runs the loop.

mod agent;
mod builder;
mod config;
mod context_files;
mod metrics;
mod policy;
mod registry;
pub mod session_store;
#[cfg(feature = "subagents")]
mod subagent;

pub use agent::{Agent, Session, Settings};
pub use builder::{build_agent, build_agent_with};
pub use config::Config;
pub use metrics::Metrics;
pub use registry::{register_builtins, Registry};

/// Parse a TOML config string into a [`Config`].
pub fn parse_config(toml_str: &str) -> anyhow::Result<Config> {
    toml::from_str(toml_str).map_err(Into::into)
}
