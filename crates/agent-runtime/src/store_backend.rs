//! Construction of the shared [`agent_config_store::Backend`] from `[config_store]`
//! bootstrap config (config design C41 / increment A3).
//!
//! A2 landed the `[config_store]` block ([`ConfigStoreCfg`](crate::config::ConfigStoreCfg))
//! but nothing consumed it yet. A3 is the first consumer: the registry's
//! `postgres` arm builds a [`PgBackend`] here and wraps it in a
//! [`StoreRegistry`](agent_registry::StoreRegistry). A3b/A3c (fleet, prompt) will
//! reuse the same helper, and E1 will share one backend instance across domains.
//!
//! **Secret discipline.** The Postgres DSN is a **reference, never a literal**:
//! `dsn_ref` is `env:NAME` or `file:/path` ([`agent_core::DsnRef`]). Resolution is
//! **fail-closed** — an unset env var or an unreadable file is a hard error (a
//! `postgres` backend cannot start without its DSN), and no error message ever
//! echoes the resolved DSN (it carries a password).

use std::sync::Arc;

use agent_config_store::{Backend, PgBackend};

use crate::config::ConfigStoreCfg;
use crate::dsn::resolve_dsn_ref;

/// Build the shared **Postgres** config-store backend from `[config_store]`.
///
/// Lazily-connecting (the pool opens on first use), so this composes with the
/// synchronous config resolvers; the schema is assumed present (the shared
/// `cards`/`tenants` tables from the config-store migration), applied out of band
/// or by an eager bootstrap.
///
/// The returned backend is wrapped in the [`crate::metered::config_store`] decorator
/// (config-plane observability, Phase 4), so every op onto this shared Postgres backend
/// — behind any registry/scheduler/prompt domain — counts and spans as `backend =
/// postgres` at the single data-owner choke point.
pub(crate) fn pg_backend(
    cfg: &ConfigStoreCfg,
    metrics: &agent_metrics::Metrics,
) -> anyhow::Result<Arc<dyn Backend>> {
    let dsn = resolve_dsn_ref(&cfg.dsn_ref)?;
    let backend = PgBackend::connect_lazy(&dsn, cfg.pool_max)
        .map_err(|e| anyhow::anyhow!("[config_store] postgres backend: {e}"))?;
    Ok(crate::metered::config_store(
        Arc::new(backend),
        metrics.clone(),
        "postgres",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(dsn_ref: &str) -> ConfigStoreCfg {
        ConfigStoreCfg {
            backend: "postgres".into(),
            dsn_ref: dsn_ref.into(),
            ..Default::default()
        }
    }

    // The `env:`/`file:` resolution + fail-closed/no-echo cases live with the
    // shared resolver in `crate::dsn`; here we prove only the backend build wiring.

    // adversarial: a build helper accepts the resolved DSN only through the ref
    // path — a lazily-built pool over a syntactically-valid DSN constructs (it
    // does not connect), proving the sync resolver path is wired end to end.
    // `connect_lazy` spawns pool maintenance, so it needs a runtime — which the
    // real caller (`build_agent`) always has.
    #[tokio::test]
    async fn positive_pg_backend_builds_lazily_from_env_ref() {
        let name = "AGENT_A3_TEST_DSN_LAZY";
        std::env::set_var(name, "postgres://u:p@127.0.0.1:5432/db");
        let cfg = cfg_with(&format!("env:{name}"));
        assert!(
            pg_backend(&cfg, &agent_metrics::Metrics::new()).is_ok(),
            "lazy pool must construct"
        );
        std::env::remove_var(name);
    }
}
