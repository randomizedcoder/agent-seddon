//! The `[campaign] store` resolver (docs/design/campaigns, CP-04).
//!
//! Opens the [`CampaignStore`] the `agent campaign …` verbs run against, mirroring
//! the `[digest] store` match in the builder with one deliberate difference:
//! the Postgres arm connects **lazily** (`PgCampaigns::connect_lazy`) and applies
//! the schema only when the caller asks ([`CampaignOpen::apply_migrations`]) *and*
//! `[config_store] migrate_on_start` is set. `--check-config` therefore proves the
//! arm links and the DSN reference resolves without dialing anything, which is
//! what keeps the `config/multi-tenant.toml` round-trip fixture (a dummy
//! `127.0.0.1:1` DSN) hermetic.
//!
//! The DSN is the shared `[config_store] dsn_ref` resolved through
//! [`crate::dsn`], so no error path here ever echoes a connection string.

use std::sync::Arc;

use agent_core::campaign::{CampaignBackend, CampaignStore, EventSink};

use crate::config::Config;

/// How to open the store: the tenant to scope it to (the CLI's `--tenant`,
/// validated as a path-safe segment by the store), whether this open may
/// apply pending migrations (`false` on the hermetic `--check-config` path),
/// and the sink every committed `task_events` row is mirrored into (CP-08: the
/// process's `TelemetryHandle` when `[telemetry]` is on; `None` mirrors nothing).
#[derive(Clone, Default)]
pub struct CampaignOpen<'a> {
    pub tenant: Option<&'a str>,
    pub apply_migrations: bool,
    pub sink: Option<Arc<dyn EventSink>>,
}

impl std::fmt::Debug for CampaignOpen<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CampaignOpen")
            .field("tenant", &self.tenant)
            .field("apply_migrations", &self.apply_migrations)
            .field("sink", &self.sink.is_some())
            .finish()
    }
}

/// The `[campaign] store` selection as `--check-config` reports it.
pub fn backend_label(cfg: &Config) -> &'static str {
    match cfg.campaign.store.trim() {
        "" => "off",
        "postgres" => "postgres",
        _ => "unknown",
    }
}

/// Open the configured campaign store, or `Ok(None)` when `[campaign] store` is
/// empty (the verbs then refuse with a hint). Fails closed on a store the build
/// cannot provide and on an unresolvable DSN reference; never dials on its own.
pub async fn open_campaign_store(
    cfg: &Config,
    open: CampaignOpen<'_>,
) -> anyhow::Result<Option<Arc<dyn CampaignStore>>> {
    // Only the postgres arm has anything to scope or migrate.
    #[cfg(not(feature = "campaign-postgres"))]
    let _ = open;
    match cfg.campaign.store.trim() {
        "" => Ok(None),
        #[cfg(feature = "campaign-postgres")]
        "postgres" => {
            use anyhow::Context as _;
            let dsn = crate::dsn::resolve_dsn_ref(&cfg.config_store.dsn_ref).context(
                "[campaign] store = \"postgres\" (DSN comes from [config_store] dsn_ref)",
            )?;
            let store = agent_campaign::PgCampaigns::connect_lazy(&dsn, cfg.campaign.pool_max)
                .map_err(|e| anyhow::anyhow!("[campaign] postgres store: {e}"))?;
            let store = match open.sink {
                Some(sink) => store.with_sink(sink),
                None => store,
            };
            let store = match open.tenant {
                Some(tenant) => store
                    .with_tenant(tenant)
                    .map_err(|e| anyhow::anyhow!("[campaign] --tenant: {e}"))?,
                None => store,
            };
            if open.apply_migrations && cfg.config_store.migrate_on_start {
                store
                    .ensure_migrated()
                    .await
                    .map_err(|e| anyhow::anyhow!("[campaign] postgres migrations: {e}"))?;
            }
            Ok(Some(Arc::new(store)))
        }
        #[cfg(not(feature = "campaign-postgres"))]
        "postgres" => anyhow::bail!(
            "[campaign] store = \"postgres\" needs the `campaign-postgres` build feature"
        ),
        // `CampaignCfg::validate` already refuses this at load; the arm keeps
        // the resolver fail-closed for a `Config` built in memory.
        other => anyhow::bail!(
            "unknown [campaign] store `{}`",
            agent_core::campaign::truncate_chars(other, 40)
        ),
    }
}

/// Open the configured campaign store as the driver's multi-tenant
/// [`CampaignBackend`] (`04-executor.md`, CP-05), or `Ok(None)` when `[campaign]
/// store` is empty. The same lazy rules as [`open_campaign_store`]: nothing dials
/// here, and the schema is applied only when `apply_migrations` **and**
/// `[config_store] migrate_on_start`. Every tenant view the backend opens mirrors
/// its committed events into `sink` (CP-08).
pub async fn open_campaign_backend(
    cfg: &Config,
    apply_migrations: bool,
    sink: Option<Arc<dyn EventSink>>,
) -> anyhow::Result<Option<Arc<dyn CampaignBackend>>> {
    #[cfg(not(feature = "campaign-postgres"))]
    let _ = (apply_migrations, sink);
    match cfg.campaign.store.trim() {
        "" => Ok(None),
        #[cfg(feature = "campaign-postgres")]
        "postgres" => {
            use anyhow::Context as _;
            let dsn = crate::dsn::resolve_dsn_ref(&cfg.config_store.dsn_ref).context(
                "[campaign] store = \"postgres\" (DSN comes from [config_store] dsn_ref)",
            )?;
            let store = agent_campaign::PgCampaigns::connect_lazy(&dsn, cfg.campaign.pool_max)
                .map_err(|e| anyhow::anyhow!("[campaign] postgres store: {e}"))?;
            let store = match sink {
                Some(sink) => store.with_sink(sink),
                None => store,
            };
            if apply_migrations && cfg.config_store.migrate_on_start {
                store
                    .ensure_migrated()
                    .await
                    .map_err(|e| anyhow::anyhow!("[campaign] postgres migrations: {e}"))?;
            }
            Ok(Some(Arc::new(store)))
        }
        #[cfg(not(feature = "campaign-postgres"))]
        "postgres" => anyhow::bail!(
            "[campaign] store = \"postgres\" needs the `campaign-postgres` build feature"
        ),
        other => anyhow::bail!(
            "unknown [campaign] store `{}`",
            agent_core::campaign::truncate_chars(other, 40)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn cfg_with_store(store: &str) -> Config {
        let mut cfg = Config::minimal_for_test();
        cfg.campaign.store = store.to_string();
        cfg
    }

    #[rstest]
    #[case::positive_off("", "off")]
    #[case::positive_postgres("postgres", "postgres")]
    #[case::corner_padded(" postgres ", "postgres")]
    #[case::negative_unknown("sqlite", "unknown")]
    #[case::adversarial_huge(&"x".repeat(100_000), "unknown")]
    fn backend_label_rows(#[case] store: &str, #[case] want: &str) {
        assert_eq!(backend_label(&cfg_with_store(store)), want);
    }

    // corner: an empty selector is "no store" — `Ok(None)`, never an error, so the
    // verbs (not the config load) decide how to refuse.
    #[tokio::test]
    async fn corner_store_off_is_none() {
        let got = open_campaign_store(&cfg_with_store(""), CampaignOpen::default())
            .await
            .expect("off is not an error");
        assert!(got.is_none());
    }

    // negative: a store this build cannot provide is a hard error naming the
    // selector — the selection proof `--check-config` relies on.
    #[rstest]
    #[case::negative_unknown_store("sqlite")]
    #[case::adversarial_unknown_store_huge(&"s".repeat(100_000))]
    #[case::adversarial_unknown_store_control_chars("post\u{1b}[31mgres")]
    #[tokio::test]
    async fn negative_unknown_store_bails_bounded(#[case] store: &str) {
        let err = open_campaign_store(&cfg_with_store(store), CampaignOpen::default())
            .await
            .err()
            .expect("unknown store must fail closed");
        let msg = err.to_string();
        assert!(msg.contains("unknown [campaign] store"), "{msg}");
        assert!(msg.len() < 200, "error is unbounded: {} bytes", msg.len());
    }

    #[cfg(not(feature = "campaign-postgres"))]
    #[tokio::test]
    async fn negative_postgres_without_feature_bails() {
        let err = open_campaign_store(&cfg_with_store("postgres"), CampaignOpen::default())
            .await
            .err()
            .expect("postgres arm is not linked");
        assert!(err.to_string().contains("campaign-postgres"), "{err}");
        let err = open_campaign_backend(&cfg_with_store("postgres"), false, None)
            .await
            .err()
            .expect("postgres arm is not linked");
        assert!(err.to_string().contains("campaign-postgres"), "{err}");
    }

    /// The driver's backend resolver mirrors the store resolver row for row.
    mod backend {
        use super::*;

        #[tokio::test]
        async fn corner_store_off_is_none() {
            let got = open_campaign_backend(&cfg_with_store(""), false, None)
                .await
                .expect("off is not an error");
            assert!(got.is_none());
        }

        #[rstest]
        #[case::negative_unknown_store("sqlite")]
        #[case::adversarial_unknown_store_huge(&"s".repeat(100_000))]
        #[tokio::test]
        async fn negative_unknown_store_bails_bounded(#[case] store: &str) {
            let err = open_campaign_backend(&cfg_with_store(store), true, None)
                .await
                .err()
                .expect("unknown store must fail closed");
            let msg = err.to_string();
            assert!(msg.contains("unknown [campaign] store"), "{msg}");
            assert!(msg.len() < 200, "error is unbounded: {} bytes", msg.len());
        }

        #[cfg(feature = "campaign-postgres")]
        #[tokio::test]
        async fn positive_postgres_lazy_open_with_dummy_dsn() {
            let cfg = super::postgres::pg_cfg(
                "AGENT_CAMPAIGN_TEST_DSN_BACKEND_LAZY",
                super::postgres::DUMMY_DSN,
            );
            let got = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                open_campaign_backend(&cfg, false, None),
            )
            .await
            .expect("a lazy open never dials, so it cannot hang")
            .expect("lazy open")
            .expect("postgres is configured");
            // The backend hands out tenant views and refuses unsafe segments
            // before any statement, like the store resolver's `--tenant`.
            assert_eq!(got.with_tenant("acme").unwrap().tenant(), "acme");
            assert!(got.with_tenant("../x").is_err());
        }

        #[cfg(feature = "campaign-postgres")]
        #[tokio::test]
        async fn negative_postgres_missing_dsn_ref_bails() {
            let mut cfg = cfg_with_store("postgres");
            cfg.config_store.dsn_ref = String::new();
            let err = open_campaign_backend(&cfg, false, None)
                .await
                .err()
                .expect("no DSN reference");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("[campaign] store") && msg.contains("dsn_ref"),
                "{msg}"
            );
        }
    }

    #[cfg(feature = "campaign-postgres")]
    mod postgres {
        use super::*;

        pub(super) const DUMMY_DSN: &str = "postgres://agent:unused@127.0.0.1:1/agent";

        /// A postgres config over an `env:` reference; the var is unique per test
        /// so the parallel runner cannot interleave them.
        pub(super) fn pg_cfg(var: &str, dsn: &str) -> Config {
            std::env::set_var(var, dsn);
            let mut cfg = cfg_with_store("postgres");
            cfg.config_store.dsn_ref = format!("env:{var}");
            cfg.config_store.migrate_on_start = true;
            cfg
        }

        // negative: the postgres arm needs a DSN *reference*; an empty
        // `[config_store] dsn_ref` fails closed and the error names the selector.
        #[tokio::test]
        async fn negative_postgres_missing_dsn_ref_bails() {
            let mut cfg = cfg_with_store("postgres");
            cfg.config_store.dsn_ref = String::new();
            let err = open_campaign_store(&cfg, CampaignOpen::default())
                .await
                .err()
                .expect("no DSN reference");
            let msg = format!("{err:#}");
            assert!(msg.contains("[campaign] store"), "{msg}");
            assert!(msg.contains("dsn_ref"), "{msg}");
        }

        // positive: the hermetic `--check-config` path — a dummy DSN on a closed
        // port opens (lazily) without dialing and without migrating.
        #[tokio::test]
        async fn positive_postgres_lazy_open_with_dummy_dsn() {
            let cfg = pg_cfg("AGENT_CAMPAIGN_TEST_DSN_LAZY", DUMMY_DSN);
            let got = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                open_campaign_store(
                    &cfg,
                    CampaignOpen {
                        tenant: None,
                        apply_migrations: false,
                        sink: None,
                    },
                ),
            )
            .await
            .expect("a lazy open never dials, so it cannot hang")
            .expect("lazy open");
            assert!(got.is_some());
        }

        // positive: a tenant scope is applied on the lazy handle, still no dial.
        #[tokio::test]
        async fn positive_postgres_lazy_open_with_tenant() {
            let cfg = pg_cfg("AGENT_CAMPAIGN_TEST_DSN_TENANT", DUMMY_DSN);
            let got = open_campaign_store(
                &cfg,
                CampaignOpen {
                    tenant: Some("acme"),
                    apply_migrations: false,
                    sink: None,
                },
            )
            .await
            .expect("lazy open with tenant");
            assert!(got.is_some());
        }

        // corner: `apply_migrations` without `migrate_on_start` is still a pure
        // lazy open — the operator's opt-out wins, so nothing dials.
        #[tokio::test]
        async fn corner_apply_migrations_without_migrate_on_start_does_not_dial() {
            let mut cfg = pg_cfg("AGENT_CAMPAIGN_TEST_DSN_NO_MIGRATE", DUMMY_DSN);
            cfg.config_store.migrate_on_start = false;
            let got = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                open_campaign_store(
                    &cfg,
                    CampaignOpen {
                        tenant: None,
                        apply_migrations: true,
                        sink: None,
                    },
                ),
            )
            .await
            .expect("must not dial")
            .expect("lazy open");
            assert!(got.is_some());
        }

        // adversarial: a tenant that is not a path-safe segment (traversal, a
        // leading dash, empty) is refused by the store before any query.
        #[rstest]
        #[case::adversarial_tenant_traversal("../other")]
        #[case::adversarial_tenant_leading_dash("-x")]
        #[case::adversarial_tenant_empty("")]
        #[case::adversarial_tenant_separator("a/b")]
        #[tokio::test]
        async fn adversarial_tenant_rejected(#[case] tenant: &str) {
            let cfg = pg_cfg("AGENT_CAMPAIGN_TEST_DSN_BAD_TENANT", DUMMY_DSN);
            let err = open_campaign_store(
                &cfg,
                CampaignOpen {
                    tenant: Some(tenant),
                    apply_migrations: false,
                    sink: None,
                },
            )
            .await
            .err()
            .expect("unsafe tenant");
            assert!(err.to_string().contains("--tenant"), "{err}");
        }

        // adversarial: an unparsable DSN behind the reference fails closed and the
        // error never echoes the connection string (it carries the password).
        #[tokio::test]
        async fn adversarial_bad_dsn_never_echoed() {
            let cfg = pg_cfg(
                "AGENT_CAMPAIGN_TEST_DSN_BAD",
                "not a url at all hunter2 db.internal",
            );
            let err = open_campaign_store(&cfg, CampaignOpen::default())
                .await
                .err()
                .expect("unparsable DSN");
            let msg = format!("{err:#}");
            assert!(msg.contains("[campaign]"), "{msg}");
            assert!(!msg.contains("hunter2"), "leaked secret: {msg}");
            assert!(!msg.contains("db.internal"), "leaked host: {msg}");
        }
    }
}
