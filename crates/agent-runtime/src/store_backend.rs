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
use anyhow::Context;

use crate::config::ConfigStoreCfg;

/// Resolve the `[config_store] dsn_ref` into a connection string. `env:`/`file:`
/// only; the *reference* (var name / path) is operator config and safe to echo,
/// but the resolved DSN never appears in an error.
fn resolve_dsn_ref(dsn_ref: &str) -> anyhow::Result<String> {
    use agent_core::DsnRef;
    match DsnRef::parse(dsn_ref).map_err(|e| anyhow::anyhow!(e))? {
        DsnRef::Env(name) => {
            let v = std::env::var(name)
                .map_err(|_| anyhow::anyhow!("[config_store] dsn_ref env var `{name}` is unset"))?;
            if v.is_empty() {
                anyhow::bail!("[config_store] dsn_ref env var `{name}` is empty");
            }
            Ok(v)
        }
        DsnRef::File(path) => {
            let expanded = crate::builder::expand_tilde(path);
            // Never echo the file *contents*; the path itself is operator config.
            let v = std::fs::read_to_string(&expanded)
                .with_context(|| format!("reading [config_store] dsn_ref file `{expanded}`"))?;
            let v = v.trim().to_string();
            if v.is_empty() {
                anyhow::bail!("[config_store] dsn_ref file `{expanded}` is empty");
            }
            Ok(v)
        }
    }
}

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

    // desc: an `env:` ref resolves to the variable's value.
    #[test]
    fn positive_env_ref_resolves() {
        // A unique var name so the test is order-independent.
        let name = "AGENT_A3_TEST_DSN_POSITIVE";
        std::env::set_var(name, "postgres://u:p@127.0.0.1:5432/db");
        let got = resolve_dsn_ref(&format!("env:{name}")).expect("resolves");
        assert_eq!(got, "postgres://u:p@127.0.0.1:5432/db");
        std::env::remove_var(name);
    }

    // negative: an unset env var is a hard error (fail-closed), not a default.
    #[test]
    fn negative_unset_env_is_fail_closed() {
        let err = resolve_dsn_ref("env:AGENT_A3_TEST_DSN_DEFINITELY_UNSET")
            .expect_err("unset var must fail closed");
        assert!(err.to_string().contains("unset"), "{err}");
    }

    // corner: an empty `dsn_ref` is rejected (a postgres backend needs a DSN).
    #[test]
    fn corner_empty_dsn_ref_rejected() {
        assert!(resolve_dsn_ref("").is_err());
    }

    // adversarial: an inline DSN (the secret-in-config mistake) is rejected, and
    // the error never echoes the connection string / password.
    #[test]
    fn adversarial_inline_dsn_rejected_without_echo() {
        let inline = "postgres://admin:hunter2@db.internal:5432/prod";
        let err = resolve_dsn_ref(inline).expect_err("inline DSN must be rejected");
        let msg = err.to_string();
        assert!(!msg.contains("hunter2"), "leaked password: {msg}");
        assert!(!msg.contains("db.internal"), "leaked host: {msg}");
    }

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
