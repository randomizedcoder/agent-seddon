//! Composition + lifecycle for the `AstBackend` seam (mirrors `search.rs`).
//!
//! `build_ast` composes the configured engines (`go` via the `agent-go-graph` helper
//! through the shared `Sandbox`; `grpc` a remote `AstService` client) into one
//! [`DispatchAst`], which presents a single interface to the `find_*` tools and to
//! `agent --serve-ast`. `spawn_freshness` warms the graph in the background on start
//! so the first query is fast. Each engine is wrapped in [`crate::metered::ast`] so
//! every verb records `agent_ast_*` metrics + an `ast.<verb>` span.

use crate::config::Config;
use agent_ast::DispatchAst;
use agent_core::AstBackend;
use agent_metrics::Metrics;
use std::sync::Arc;

/// Compose the configured `[ast] backends` into one dispatcher. Returns `None` when
/// no backend could be built (e.g. `go` selected but no sandbox available), so the
/// caller simply skips wiring the seam rather than failing the whole agent.
pub fn build_ast(
    cfg: &Config,
    sandbox: Option<&Arc<dyn agent_core::Sandbox>>,
    metrics: &Metrics,
) -> anyhow::Result<Option<Arc<DispatchAst>>> {
    let root = repo_root();
    let mut backends: Vec<(String, Arc<dyn AstBackend>)> = Vec::new();

    for name in cfg.ast.backend_names() {
        match name.as_str() {
            "go" => {
                let Some(sandbox) = sandbox else {
                    tracing::warn!("ast backend `go` needs a sandbox; skipping");
                    continue;
                };
                let go = agent_ast::GoAst::new(sandbox.clone(), root.clone())
                    .with_timeout(cfg.ast.helper_timeout_secs);
                backends.push((
                    "go".to_string(),
                    crate::metered::ast(Arc::new(go), metrics.clone(), "go"),
                ));
            }
            #[cfg(feature = "ast-scip")]
            "scip" => {
                let Some(sandbox) = sandbox else {
                    tracing::warn!("ast backend `scip` needs a sandbox; skipping");
                    continue;
                };
                let indexers: Vec<agent_ast::ScipIndexer> = cfg
                    .ast
                    .scip_langs
                    .iter()
                    .filter_map(|l| agent_ast::ScipIndexer::builtin(l))
                    .collect();
                if indexers.is_empty() {
                    tracing::warn!(
                        "ast backend `scip` has no known languages in [ast] scip_langs; skipping"
                    );
                    continue;
                }
                let scip = agent_ast::ScipAst::new(sandbox.clone(), root.clone(), indexers);
                backends.push((
                    "scip".to_string(),
                    crate::metered::ast(Arc::new(scip), metrics.clone(), "scip"),
                ));
            }
            #[cfg(feature = "grpc")]
            "grpc" => {
                let ep = crate::registry::grpc_client_endpoint(
                    &cfg.grpc.ast.endpoint,
                    agent_grpc::constants::AST,
                );
                let client = agent_grpc::client::GrpcAst::connect(&ep)?;
                backends.push((
                    "grpc".to_string(),
                    crate::metered::ast(Arc::new(client), metrics.clone(), "grpc"),
                ));
            }
            other => tracing::warn!("unknown ast backend `{other}`, skipping"),
        }
    }

    if backends.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Arc::new(DispatchAst::new(backends))))
    }
}

/// Kick off a background graph build so the first query is warm (serve-stale until
/// then). Detached — a build failure is logged, never fatal.
pub fn spawn_freshness(dispatch: Arc<DispatchAst>) {
    tokio::spawn(async move {
        let noop = |_p: agent_core::ReindexProgress| {};
        if let Err(e) = dispatch.reindex(&noop).await {
            tracing::debug!("ast background reindex skipped: {e}");
        }
    });
}

/// Discover the repo root by walking up from the cwd for a `.git`; fall back to cwd.
fn repo_root() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut cur = cwd.as_path();
    loop {
        if cur.join(".git").exists() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return cwd,
        }
    }
}
