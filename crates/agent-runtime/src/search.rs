//! Search seam wiring: compose the configured backends into a single metered
//! [`DispatchSearch`], and a background task that keeps the index fresh on start.
//!
//! Each backend is wrapped in its own metrics decorator (so `tantivy` vs. another
//! backend read distinctly) *before* being composed, matching how the other seams
//! attribute a `= "grpc"` client separately from a local impl.

use crate::config::Config;
use crate::registry::Registry;
use agent_core::IndexState;
use agent_metrics::Metrics;
use agent_search::DispatchSearch;
use anyhow::Context;
use std::sync::Arc;

/// Build the composed search backend from `[search] backends` (empty ⇒ the single
/// default). The result presents one interface to the loop's `search` tool and to
/// `--serve-search`, while retaining every backend for head-to-head comparison.
pub fn build_search(
    registry: &Registry,
    cfg: &Config,
    metrics: &Metrics,
) -> anyhow::Result<Arc<DispatchSearch>> {
    let mut backends = Vec::new();
    // Every backend now comes from the registry: since factories receive
    // `Metrics` via `FactoryCtx`, the `vector` backend no longer needs to be
    // special-cased here to get its Embedder metered (parity spec 24 follow-up).
    let ctx = crate::registry::FactoryCtx::new(cfg, metrics);
    for name in cfg.search.backend_names() {
        let inner = registry
            .build_search(&name, &ctx)
            .with_context(|| format!("building search backend `{name}`"))?;
        let metered = crate::metered::search(inner, metrics.clone(), &name);
        backends.push((name, metered));
    }
    Ok(Arc::new(DispatchSearch::new(backends)?))
}

/// Build the semantic `VectorBackend` over the config-selected, metered Embedder
/// (parity spec 15).
#[cfg(feature = "semantic-search")]
pub(crate) fn build_vector(
    ctx: &crate::registry::FactoryCtx<'_>,
) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>> {
    let (cfg, metrics) = (ctx.cfg, ctx.metrics);
    // Prefer the embedder the builder already made (and metered), so the vector
    // index and `agent --serve-embed` share one instance — a real embedder loads
    // a model, and building it twice would load it twice.
    let embedder = match ctx.built_embedder {
        Some(e) => e.clone(),
        None => {
            let e = build_embedder(cfg)?;
            crate::metered::embedder(e, metrics.clone(), &cfg.embedder.backend)
        }
    };
    let start = if cfg.agent.working_dir.is_empty() {
        std::path::PathBuf::from(".")
    } else {
        std::path::PathBuf::from(&cfg.agent.working_dir)
    };
    let root = agent_search::repo_root(&start);
    let index_dir = if cfg.search.index_dir.is_empty() {
        agent_search::default_index_dir(&root, "vector")
    } else {
        std::path::PathBuf::from(&cfg.search.index_dir).join("vector")
    };
    Ok(Arc::new(agent_search::VectorBackend::new(
        root, index_dir, embedder,
    )))
}

/// Build the config-selected embedder (`[embedder] backend`).
#[cfg(feature = "semantic-search")]
pub(crate) fn build_embedder(cfg: &Config) -> anyhow::Result<Arc<dyn agent_core::Embedder>> {
    match cfg.embedder.backend.as_str() {
        "local" => Ok(Arc::new(agent_embed::LocalEmbedder::new(
            cfg.embedder.dimensions,
        ))),
        // A remote embedder: a GPU host serving a fleet. Dimensions are VERIFIED
        // against config at build time — see `GrpcEmbed::verify_dimensions`.
        #[cfg(feature = "grpc")]
        "grpc" => {
            let ep = crate::registry::grpc_client_endpoint(
                &cfg.grpc.embed.endpoint,
                agent_grpc::constants::EMBED,
            );
            Ok(Arc::new(agent_grpc::client::GrpcEmbed::connect(
                &ep,
                cfg.embedder.dimensions,
            )?))
        }
        other => anyhow::bail!("unknown [embedder] backend `{other}` (built in: `local`, `grpc`)"),
    }
}

/// Spawn a detached task that brings each backend's index up to date if it is
/// stale/missing. Non-blocking: the agent starts immediately, and queries serve
/// the last committed snapshot until a background reindex commits.
pub fn spawn_freshness(dispatch: Arc<DispatchSearch>, metrics: Metrics) {
    tokio::spawn(async move {
        for (name, backend) in dispatch.all() {
            match backend.status().await {
                Ok(st) if st.state == IndexState::Fresh => {
                    tracing::debug!(backend = %name, files = st.indexed_files, "search index fresh");
                    metrics.set_search_fresh(name, true);
                }
                Ok(st) => {
                    tracing::info!(
                        backend = %name, state = ?st.state,
                        "search index not fresh — reindexing in the background"
                    );
                    metrics.set_search_fresh(name, false);
                    metrics.on_search_reindex(name, "startup");
                    match backend.reindex(&|_p| {}).await {
                        Ok(done) => tracing::info!(
                            backend = %name, files = done.indexed_files,
                            "search index rebuilt"
                        ),
                        Err(e) => {
                            tracing::warn!(backend = %name, error = %e, "search reindex failed");
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(backend = %name, error = %e, "search status check failed");
                }
            }
        }
    });
}

/// Kick off a one-shot background freshness check for a single freshly-built
/// backend — the per-tenant code index (multi-tenancy C28-3d), which
/// [`spawn_freshness`] cannot reach because it only warms the `local` view at
/// startup (there is no ambient identity then). Reindex if the tenant's index is
/// stale/missing so its first `search` serves real hits; queries serve the last
/// committed snapshot meanwhile (serve-stale). Tracing only — the label-less
/// search-health *gauges* are deliberately not touched here, so many tenants
/// warming concurrently cannot make one gauge flap across tenants.
pub(crate) fn spawn_reindex_if_stale(backend: Arc<dyn agent_core::SearchBackend>) {
    tokio::spawn(async move {
        match backend.status().await {
            Ok(st) if st.state == IndexState::Fresh => {
                tracing::debug!(files = st.indexed_files, "per-tenant code index fresh");
            }
            Ok(st) => {
                tracing::info!(state = ?st.state, "per-tenant code index not fresh — reindexing");
                if let Err(e) = backend.reindex(&|_p| {}).await {
                    tracing::warn!(error = %e, "per-tenant code index reindex failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "per-tenant code index status check failed"),
        }
    });
}

/// A fail-closed, empty [`SearchBackend`]: it holds no documents and every query
/// returns nothing. Used as the per-tenant fallback (multi-tenancy C28-3d) when a
/// tenant's own on-disk index cannot be opened — so that tenant sees an **empty**
/// index, never another tenant's, mirroring the sqlite prompt arm's isolated
/// in-memory fallback. Never selected by config; purely a defensive fallback.
pub(crate) struct EmptySearch;

#[async_trait::async_trait]
impl agent_core::SearchBackend for EmptySearch {
    fn capabilities(&self) -> agent_core::SearchCapabilities {
        agent_core::SearchCapabilities {
            backend: "empty".into(),
            modes: vec![],
            content_search: false,
            scored: false,
            incremental: false,
            max_concurrent_queries: 0,
        }
    }
    async fn status(&self) -> agent_core::Result<agent_core::IndexStatus> {
        Ok(agent_core::IndexStatus {
            state: IndexState::Missing,
            indexed_files: 0,
            last_indexed_ms: 0,
            manifest_digest: String::new(),
        })
    }
    async fn reindex(
        &self,
        _progress: agent_core::ProgressFn<'_>,
    ) -> agent_core::Result<agent_core::IndexStatus> {
        // Nothing to index; report the same empty, missing status.
        self.status().await
    }
    async fn query(
        &self,
        _q: &agent_core::SearchQuery,
    ) -> agent_core::Result<Vec<agent_core::SearchHit>> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{IndexStatus, SearchBackend};
    use agent_testkit::FixtureSearch;
    use std::time::Duration;

    // ---- build_embedder: `[embedder] backend` resolution -------------------

    #[cfg(feature = "semantic-search")]
    #[test]
    fn positive_build_embedder_local_backend() {
        // `minimal_for_test` defaults `embedder.backend = "local"`.
        let cfg = crate::config::Config::minimal_for_test();
        assert!(build_embedder(&cfg).is_ok());
    }

    // Untrusted config string: an unknown backend must fail closed with a legible,
    // built-in-listing error — never panic or silently fall back to a default.
    #[cfg(feature = "semantic-search")]
    #[rstest::rstest]
    #[case::garbage("bogus")]
    #[case::empty("")]
    #[case::path_traversal("../../etc/passwd")]
    #[case::uppercased_known("LOCAL")]
    fn adversarial_unknown_embedder_backend_bails(#[case] backend: &str) {
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.embedder.backend = backend.into();
        // `Arc<dyn Embedder>` isn't Debug, so match rather than `expect_err`.
        let err = match build_embedder(&cfg) {
            Ok(_) => panic!("an unknown embedder backend `{backend}` must fail, not build"),
            Err(e) => e.to_string(),
        };
        assert!(
            err.contains("unknown [embedder] backend"),
            "expected a legible unknown-backend error, got: {err}"
        );
    }

    // ---- build_vector: composes a metered local embedder + index dir -------

    #[cfg(feature = "semantic-search")]
    #[test]
    fn positive_build_vector_builds_local_embedder_when_none_prebuilt() {
        let dir = agent_testkit::tempdir();
        let mut cfg = crate::config::Config::minimal_for_test();
        cfg.agent.working_dir = dir.display().to_string();
        cfg.search.index_dir = dir.join("idx").display().to_string();
        let metrics = Metrics::new();
        let ctx = crate::registry::FactoryCtx::new(&cfg, &metrics);
        assert!(build_vector(&ctx).is_ok());
    }

    // ---- spawn_freshness: fresh short-circuits; stale reindexes ------------

    fn dispatch_of(fx: Arc<FixtureSearch>) -> Arc<DispatchSearch> {
        Arc::new(
            DispatchSearch::new(vec![("fixture".into(), fx as Arc<dyn SearchBackend>)]).unwrap(),
        )
    }

    async fn wait_until(pred: impl Fn() -> bool) -> bool {
        for _ in 0..50 {
            if pred() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        pred()
    }

    // A fresh index reports fresh and is left alone — no background reindex.
    #[tokio::test]
    async fn positive_fresh_index_is_not_reindexed() {
        let fx = Arc::new(FixtureSearch::new()); // Default status = Fresh.
        spawn_freshness(dispatch_of(fx.clone()), Metrics::new());
        // Let the detached task run; a fresh index must never reindex.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(fx.reindex_count(), 0, "a fresh index must not be reindexed");
    }

    // A stale index triggers exactly one background reindex.
    #[tokio::test]
    async fn positive_stale_index_triggers_background_reindex() {
        let stale = IndexStatus {
            state: IndexState::Stale,
            indexed_files: 0,
            last_indexed_ms: 0,
            manifest_digest: "stale".into(),
        };
        let fx = Arc::new(FixtureSearch::new().with_status(stale));
        spawn_freshness(dispatch_of(fx.clone()), Metrics::new());
        assert!(
            wait_until(|| fx.reindex_count() > 0).await,
            "a stale index must trigger a background reindex"
        );
        assert_eq!(fx.reindex_count(), 1);
    }

    // ---- EmptySearch: the fail-closed per-tenant fallback (C28-3d) ---------

    // A tenant whose own index can't open must see NOTHING — never another tenant's
    // rows. The empty backend reports a missing index and returns no hits.
    #[tokio::test]
    async fn empty_search_returns_no_hits_and_missing_status() {
        use agent_core::{IndexState, SearchMode, SearchQuery};
        let backend = EmptySearch;
        let st = backend.status().await.unwrap();
        assert_eq!(st.state, IndexState::Missing);
        assert_eq!(st.indexed_files, 0);
        let hits = backend
            .query(&SearchQuery {
                text: "anything".into(),
                mode: SearchMode::Literal,
                path_globs: vec![],
                lang: None,
                limit: 10,
                fuzzy_distance: None,
            })
            .await
            .unwrap();
        assert!(hits.is_empty(), "the fail-closed fallback returns nothing");
        // reindex is a no-op that stays empty (never rebuilds from a shared source).
        let after = backend.reindex(&|_p| {}).await.unwrap();
        assert_eq!(after.state, IndexState::Missing);
    }
}
