//! `agent-runtime` — wires the seams and runs the loop.

mod agent;
#[cfg(feature = "ast")]
mod ast;
#[cfg(feature = "grpc")]
pub mod auth_params;
mod builder;
#[cfg(feature = "graph")]
mod cognition;
mod config;
#[cfg(feature = "config-schema")]
mod config_schema;
#[cfg(feature = "config")]
mod config_store;
mod context_files;
mod distiller;
pub mod doctor;
mod egress;
#[cfg(feature = "review")]
mod fleet_review;
#[cfg(feature = "git")]
mod git;
pub mod hooks;
#[cfg(feature = "auth")]
pub mod login;
mod metered;
mod policy;
#[cfg(all(feature = "fleet", feature = "transport-registry-store"))]
mod progress;
#[cfg(feature = "recall")]
pub mod recall;
mod registry;
#[cfg(feature = "search")]
mod search;
pub mod secrets;
mod session_events;
pub mod session_store;
pub mod skills;
// The fail-closed `env:`/`file:` DSN-reference resolver, shared by every Postgres
// tier (the config-store domains via `store_backend`, the digest ledger via
// the builder's `pg_digests`, and the campaign store via `campaign`). One home
// for the secret-handling (never echo the resolved DSN), so the callers can't
// diverge.
#[cfg(any(
    feature = "auth-postgres",
    feature = "role-postgres",
    feature = "registry-postgres",
    feature = "fleet-postgres",
    feature = "prompt-postgres",
    feature = "scheduler-postgres",
    feature = "forge-registry-postgres",
    feature = "transport-registry-postgres",
    feature = "digest-postgres",
    feature = "campaign-postgres"
))]
mod dsn;
// The `[campaign] store` resolver (docs/design/campaigns, CP-04): opens the
// campaign store the `agent campaign …` verbs run against. Lazy by design.
#[cfg(feature = "campaign")]
pub mod campaign;
#[cfg(any(
    feature = "auth-postgres",
    feature = "role-postgres",
    feature = "registry-postgres",
    feature = "fleet-postgres",
    feature = "prompt-postgres",
    feature = "scheduler-postgres",
    feature = "forge-registry-postgres",
    feature = "transport-registry-postgres",
    // The sqlite arms build their config-store `SqliteBackend` here too (PG-10:
    // prompt; PG-11: registry/fleet reuse `sqlite_backend`).
    feature = "prompt-sqlite",
    feature = "registry-sqlite",
    feature = "fleet-sqlite"
))]
mod store_backend;
#[cfg(feature = "structured")]
pub mod structured;
#[cfg(feature = "subagents")]
mod subagent;
mod tool_provider;
// Per-tenant routing over the multi-tenant seams: the converged shared-store
// control-plane seams (config C35 / C2 — provider-registry, review-fleet, prompt)
// and the file-backed cognition graph (config C2b, path-namespaced per tenant).
// Only meaningful — and only compiled — when one of those seams is built.
#[cfg(any(
    feature = "registry-store",
    feature = "fleet-store",
    feature = "prompt-store",
    feature = "graph",
    feature = "scheduler-store"
))]
mod tenant;
// The tenant-fanning scheduler driver (config C2c-2): the durable half of the
// scheduler seam. Only compiled when the shared-store scheduler is built.
#[cfg(feature = "scheduler-store")]
mod scheduler_driver;

pub use agent::{Agent, GrpcTlsSettings, OpenError, Session, SessionManager, Settings};
pub use agent_metrics::Metrics;
pub use builder::{build_agent, build_agent_with};
/// C29 config-ownership annotation: the (currently empty) set of tenant-writable
/// `agent.toml` sections — every section is operator-global. See the fn's docs.
pub use config::tenant_writable_config_sections;
pub use config::CampaignCfg;
pub use config::Config;
#[cfg(feature = "recall")]
pub use config::RecallCfg;
/// One `[[auth.issuers]]` entry (security-hardening S3), mapped by the serve path.
pub use config::{
    AuthIssuerCfg, AuthMtlsBindingCfg, AuthMtlsCfg, AuthTokenCfg, ClientBearer, GrpcClientCfg,
};
#[cfg(feature = "config-schema")]
pub use config_schema::{build_schema, validate_config};
#[cfg(feature = "config")]
pub use config_store::FileConfigStore;
pub use egress::derive_egress_allowlist;
#[cfg(all(feature = "fleet", feature = "transport-registry-store"))]
pub use progress::{progress_channels, TransportProgressFeed};
#[cfg(feature = "fleet")]
pub use registry::{
    build_session_forge, build_session_forge_from_card, resolve_session_forge,
    resolve_tenant_token_ref, resolve_token_ref,
};
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
    // `[auth]` is checked here, at load, so every entry point (serve, one-shot,
    // doctor) refuses a bad block up front (security-hardening S1).
    cfg.auth.validate().map_err(anyhow::Error::msg)?;
    cfg.grpc.tls.validate().map_err(anyhow::Error::msg)?;
    if let Some(mtls) = &cfg.auth.mtls {
        mtls.validate_client(&cfg.grpc.tls.client)
            .map_err(anyhow::Error::msg)?;
    }
    cfg.grpc
        .client
        .validate(&cfg.auth)
        .map_err(anyhow::Error::msg)?;
    cfg.telemetry.validate().map_err(anyhow::Error::msg)?;
    cfg.campaign.validate().map_err(anyhow::Error::msg)?;
    Ok((cfg, unknown))
}
