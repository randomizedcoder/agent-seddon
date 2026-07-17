//! `agent-runtime` — wires the seams and runs the loop.

mod agent;
mod builder;
mod config;
mod policy;

pub use agent::{Agent, Settings};
pub use builder::build_agent;
pub use config::Config;

/// Parse a TOML config string into a [`Config`].
pub fn parse_config(toml_str: &str) -> anyhow::Result<Config> {
    toml::from_str(toml_str).map_err(Into::into)
}
