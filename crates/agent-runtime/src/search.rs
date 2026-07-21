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
    for name in cfg.search.backend_names() {
        // The `vector` (semantic) backend is built here rather than via a registry
        // factory so its Embedder is metered (the factory has no Metrics handle).
        let inner = match name.as_str() {
            #[cfg(feature = "semantic-search")]
            "vector" => build_vector(cfg, metrics)?,
            _ => registry
                .build_search(&name, cfg)
                .with_context(|| format!("building search backend `{name}`"))?,
        };
        let metered = crate::metered::search(inner, metrics.clone(), &name);
        backends.push((name, metered));
    }
    Ok(Arc::new(DispatchSearch::new(backends)?))
}

/// Build the semantic `VectorBackend` over the config-selected, metered Embedder
/// (parity spec 15).
#[cfg(feature = "semantic-search")]
fn build_vector(
    cfg: &Config,
    metrics: &Metrics,
) -> anyhow::Result<Arc<dyn agent_core::SearchBackend>> {
    let embedder = build_embedder(cfg)?;
    let embedder = crate::metered::embedder(embedder, metrics.clone(), &cfg.embedder.backend);
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
fn build_embedder(cfg: &Config) -> anyhow::Result<Arc<dyn agent_core::Embedder>> {
    match cfg.embedder.backend.as_str() {
        "local" => Ok(Arc::new(agent_embed::LocalEmbedder::new(
            cfg.embedder.dimensions,
        ))),
        other => anyhow::bail!("unknown [embedder] backend `{other}` (only `local` is built in)"),
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
                            tracing::warn!(backend = %name, error = %e, "search reindex failed")
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(backend = %name, error = %e, "search status check failed")
                }
            }
        }
    });
}
