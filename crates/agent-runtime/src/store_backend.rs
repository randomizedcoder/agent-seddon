//! Construction of the shared [`agent_config_store::Backend`] from `[config_store]`
//! bootstrap config (config design C41 / increment A3).
//!
//! A2 landed the `[config_store]` block ([`ConfigStoreCfg`](crate::config::ConfigStoreCfg))
//! but nothing consumed it yet. A3 is the first consumer: the registry's
//! `postgres` arm builds a [`PgBackend`] here and wraps it in a
//! [`StoreRegistry`](agent_registry::StoreRegistry). A3b/A3c (fleet, prompt) reuse
//! the same helpers, and E1 shares one backend instance across domains.
//!
//! Two constructors, one decorator: [`pg_backend`] opens the shared **Postgres**
//! pool from a secret `dsn_ref`; [`sqlite_backend`] opens an embedded **SQLite**
//! catalog at a path (PG-10 routed the `sqlite` prompt tier through the shared store,
//! and PG-11 reuses this for the registry/fleet sqlite arms). Both wrap the backend
//! in [`crate::metered::config_store`] so every op counts + spans at the single
//! data-owner choke point.
//!
//! **Secret discipline.** The Postgres DSN is a **reference, never a literal**:
//! `dsn_ref` is `env:NAME` or `file:/path` ([`agent_core::DsnRef`]). Resolution is
//! **fail-closed** — an unset env var or an unreadable file is a hard error (a
//! `postgres` backend cannot start without its DSN), and no error message ever
//! echoes the resolved DSN (it carries a password).

use std::sync::Arc;

use agent_config_store::Backend;

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
#[cfg(any(
    feature = "registry-postgres",
    feature = "fleet-postgres",
    feature = "prompt-postgres",
    feature = "scheduler-postgres",
    feature = "forge-registry-postgres",
    feature = "transport-registry-postgres"
))]
pub(crate) fn pg_backend(
    cfg: &crate::config::ConfigStoreCfg,
    metrics: &agent_metrics::Metrics,
) -> anyhow::Result<Arc<dyn Backend>> {
    let dsn = crate::dsn::resolve_dsn_ref(&cfg.dsn_ref)?;
    // `migrate_on_start` applies the schema on first use: there is no connection
    // yet, and a fresh database has no `cards` table (S15b).
    let backend = agent_config_store::PgBackend::connect_lazy(&dsn, cfg.pool_max)
        .map_err(|e| anyhow::anyhow!("[config_store] postgres backend: {e}"))?
        .migrate_lazily(cfg.migrate_on_start);
    Ok(crate::metered::config_store(
        Arc::new(backend),
        metrics.clone(),
        "postgres",
    ))
}

/// Build an embedded **SQLite** config-store backend at `path`.
///
/// The `cards`/`tenants` schema is created on open (`CREATE TABLE IF NOT EXISTS`,
/// gated by `PRAGMA user_version`), so it is safe on a fresh file or a re-open. Like
/// [`pg_backend`], the result is wrapped in the [`crate::metered::config_store`]
/// decorator, so a `sqlite`-backed domain (prompt/registry/fleet) counts + spans as
/// `backend = sqlite` at the same choke point. The caller resolves the path (e.g.
/// tilde/working-dir expansion); this opens exactly what it is given.
#[cfg(any(
    feature = "prompt-sqlite",
    feature = "registry-sqlite",
    feature = "fleet-sqlite"
))]
pub(crate) fn sqlite_backend(
    path: &std::path::Path,
    metrics: &agent_metrics::Metrics,
) -> anyhow::Result<Arc<dyn Backend>> {
    let backend = agent_config_store::SqliteBackend::open(path)
        .map_err(|e| anyhow::anyhow!("[config_store] sqlite backend at {path:?}: {e}"))?;
    Ok(crate::metered::config_store(
        Arc::new(backend),
        metrics.clone(),
        "sqlite",
    ))
}

#[cfg(all(
    test,
    any(
        feature = "registry-postgres",
        feature = "fleet-postgres",
        feature = "prompt-postgres",
        feature = "scheduler-postgres",
        feature = "forge-registry-postgres",
        feature = "transport-registry-postgres",
        feature = "prompt-sqlite",
        feature = "registry-sqlite",
        feature = "fleet-sqlite"
    )
))]
mod tests {
    use super::*;

    // The `env:`/`file:` resolution + fail-closed/no-echo cases live with the
    // shared resolver in `crate::dsn`; here we prove only the backend build wiring.

    // adversarial: a build helper accepts the resolved DSN only through the ref
    // path — a lazily-built pool over a syntactically-valid DSN constructs (it
    // does not connect), proving the sync resolver path is wired end to end.
    // `connect_lazy` spawns pool maintenance, so it needs a runtime — which the
    // real caller (`build_agent`) always has.
    #[cfg(any(
        feature = "registry-postgres",
        feature = "fleet-postgres",
        feature = "prompt-postgres",
        feature = "scheduler-postgres",
        feature = "forge-registry-postgres",
        feature = "transport-registry-postgres"
    ))]
    #[tokio::test]
    async fn positive_pg_backend_builds_lazily_from_env_ref() {
        use crate::config::ConfigStoreCfg;
        let name = "AGENT_A3_TEST_DSN_LAZY";
        std::env::set_var(name, "postgres://u:p@127.0.0.1:5432/db");
        let cfg = ConfigStoreCfg {
            backend: "postgres".into(),
            dsn_ref: format!("env:{name}"),
            ..Default::default()
        };
        assert!(
            pg_backend(&cfg, &agent_metrics::Metrics::new()).is_ok(),
            "lazy pool must construct"
        );
        std::env::remove_var(name);
    }

    // positive: the sqlite helper opens an on-disk catalog and returns a live,
    // metered backend that round-trips a card (ensure-tenant → put → get). Runs
    // under any `*-sqlite` tier (prompt/registry/fleet) — the helper is shared.
    #[cfg(any(
        feature = "prompt-sqlite",
        feature = "registry-sqlite",
        feature = "fleet-sqlite"
    ))]
    #[tokio::test]
    async fn positive_sqlite_backend_opens_and_roundtrips() {
        use agent_config_store::Write;
        let dir = agent_testkit::tempdir();
        let backend = sqlite_backend(&dir.join("cards.db"), &agent_metrics::Metrics::new())
            .expect("open sqlite backend");
        backend
            .apply(&[
                Write::EnsureTenant {
                    tenant: "local".into(),
                },
                Write::Put {
                    collection: "prompt_system",
                    tenant: "local".into(),
                    id: "system".into(),
                    blob: b"hello".to_vec(),
                },
            ])
            .await
            .expect("apply through the sqlite backend");
        assert_eq!(
            backend
                .get("prompt_system", "local", "system")
                .await
                .unwrap()
                .as_deref(),
            Some(b"hello".as_slice()),
        );
    }
}
