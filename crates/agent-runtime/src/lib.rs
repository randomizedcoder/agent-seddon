//! `agent-runtime` — wires the seams and runs the loop.

mod agent;
#[cfg(feature = "ast")]
mod ast;
mod builder;
#[cfg(feature = "graph")]
mod cognition;
mod config;
mod context_files;
mod distiller;
#[cfg(feature = "git")]
mod git;
pub mod hooks;
mod metered;
mod policy;
#[cfg(feature = "recall")]
pub mod recall;
mod registry;
#[cfg(feature = "search")]
mod search;
mod session_events;
pub mod session_store;
pub mod skills;
#[cfg(feature = "structured")]
pub mod structured;
#[cfg(feature = "subagents")]
mod subagent;

pub use agent::{Agent, OpenError, Session, SessionManager, Settings};
pub use agent_metrics::Metrics;
pub use builder::{build_agent, build_agent_with};
pub use config::Config;
#[cfg(feature = "recall")]
pub use config::RecallCfg;
pub use registry::{register_builtins, Registry};
pub use session_events::{SessionEvents, SessionEventsRegistry};

/// Parse a TOML config string into a [`Config`], **warning about keys it did not
/// recognise**.
///
/// Unknown keys are a warning rather than an error, deliberately: rejecting them
/// would turn a stale or forward-looking key into a hard startup failure and
/// break configs that work today. But they must not be *silent* — this config
/// selects which implementation each seam uses, so a misplaced key means the
/// agent quietly runs something other than what the operator asked for. A
/// `[agent] memory = "grpc"` (the real key is `[memory] backend`) previously
/// parsed cleanly and used the local store, with nothing to indicate it.
pub fn parse_config(toml_str: &str) -> anyhow::Result<Config> {
    let (cfg, unknown) = parse_config_reporting_unknown(toml_str)?;
    for key in &unknown {
        tracing::warn!(
            key = %key,
            "unknown config key — it is being IGNORED, so anything it was meant to \
             configure is running its default. Check the spelling and the section \
             it belongs in (see config/agent.toml)"
        );
    }
    Ok(cfg)
}

/// Parse, and also return the dotted paths of any keys the deserializer ignored.
///
/// Split out from [`parse_config`] so the set can be asserted on rather than
/// only logged — in particular, that the shipped `config/agent.toml` yields
/// none, since a warning that fires on the reference config is just noise.
pub fn parse_config_reporting_unknown(toml_str: &str) -> anyhow::Result<(Config, Vec<String>)> {
    let de = toml::Deserializer::new(toml_str);
    let mut unknown = Vec::new();
    let cfg: Config = serde_ignored::deserialize(de, |path| unknown.push(path.to_string()))?;
    Ok((cfg, unknown))
}
