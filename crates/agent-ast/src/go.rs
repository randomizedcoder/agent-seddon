//! `GoAst` — the type-aware Go engine behind the `AstBackend` seam. It runs the
//! pinned `agent-go-graph` helper through the [`Sandbox`] seam (same
//! reproducible-execution rationale as the review call-graph collector), parses its
//! untrusted JSON into a bounded [`Graph`], and answers the seam verbs over it.
//!
//! The graph is built lazily on first query and cached; `reindex` forces a rebuild.
//! Fail-soft like the analyzer: a missing helper (exit 127), a timeout, or an
//! unparseable output surfaces as [`Error::Ast`], never a panic.

use crate::graph::Graph;
use agent_core::{
    AstBackend, AstCallGraph, AstCapabilities, AstVerb, Error, ExecSpec, IndexState, IndexStatus,
    ProgressFn, Result, Sandbox, Symbol, SymbolQuery, SymbolRef,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The pinned helper binary (on PATH via the flake dev shell / check inputs).
const HELPER: &str = "agent-go-graph";
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// The Go code-graph backend. Cheap to clone-share via `Arc`; holds the lazily-built
/// graph behind an `RwLock` so concurrent queries share one build.
pub struct GoAst {
    sandbox: Arc<dyn Sandbox>,
    root: PathBuf,
    timeout_secs: u64,
    graph: RwLock<Option<Arc<Graph>>>,
}

impl GoAst {
    /// A Go engine rooted at `root`, running the helper through `sandbox`.
    pub fn new(sandbox: Arc<dyn Sandbox>, root: impl Into<PathBuf>) -> Self {
        Self {
            sandbox,
            root: root.into(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            graph: RwLock::new(None),
        }
    }

    /// Override the helper timeout (seconds).
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs.max(1);
        self
    }

    /// Return the cached graph, building it on first use. Concurrent first callers
    /// serialize on the write lock so the helper runs once.
    async fn ensure_built(&self) -> Result<Arc<Graph>> {
        if let Some(g) = self.graph.read().await.as_ref() {
            return Ok(g.clone());
        }
        let mut w = self.graph.write().await;
        if let Some(g) = w.as_ref() {
            return Ok(g.clone());
        }
        let g = Arc::new(self.build().await?);
        *w = Some(g.clone());
        Ok(g)
    }

    /// Run the helper and parse its output into a fresh graph. The command is static
    /// (`agent-go-graph --root .`) — no untrusted input reaches the shell; the helper
    /// walks the tree itself.
    async fn build(&self) -> Result<Graph> {
        let spec =
            ExecSpec::sh(format!("{HELPER} --root ."), &self.root).timeout(self.timeout_secs);
        let out =
            self.sandbox.exec(&spec).await.map_err(|e| {
                Error::Ast(format!("helper exec failed: {}", trunc(&e.to_string())))
            })?;
        if out.timed_out {
            return Err(Error::Ast(format!(
                "{HELPER} timed out after {}s",
                self.timeout_secs
            )));
        }
        if out.exit_code == 127 {
            return Err(Error::Ast(format!("{HELPER} not found on PATH")));
        }
        if out.exit_code != 0 {
            return Err(Error::Ast(format!(
                "{HELPER} failed (exit {}): {}",
                out.exit_code,
                trunc(out.stderr.trim())
            )));
        }
        Graph::parse(&out.stdout, &self.root)
            .ok_or_else(|| Error::Ast(format!("{HELPER} output unparseable")))
    }
}

#[async_trait::async_trait]
impl AstBackend for GoAst {
    fn capabilities(&self) -> AstCapabilities {
        AstCapabilities {
            backend: "go".into(),
            languages: vec!["go".into()],
            verbs: vec![
                AstVerb::FindSymbol,
                AstVerb::Implementations,
                AstVerb::InterfaceOf,
                AstVerb::Callers,
                AstVerb::Callees,
                AstVerb::Callchain,
                AstVerb::BlastRadius,
                AstVerb::DependencyPath,
            ],
            incremental: false,
        }
    }

    async fn status(&self) -> Result<IndexStatus> {
        // Cheap probe: report built/not-built without triggering a build.
        let built = self.graph.read().await;
        let (state, files) = match built.as_ref() {
            Some(g) => (IndexState::Fresh, g.packages().len() as u64),
            None => (IndexState::Missing, 0),
        };
        Ok(IndexStatus {
            state,
            indexed_files: files,
            last_indexed_ms: 0,
            manifest_digest: String::new(),
        })
    }

    async fn reindex(&self, progress: ProgressFn<'_>) -> Result<IndexStatus> {
        progress(agent_core::ReindexProgress {
            files_done: 0,
            files_total: 1,
            done: false,
        });
        let g = Arc::new(self.build().await?);
        let files = g.packages().len() as u64;
        *self.graph.write().await = Some(g);
        progress(agent_core::ReindexProgress {
            files_done: 1,
            files_total: 1,
            done: true,
        });
        Ok(IndexStatus {
            state: IndexState::Fresh,
            indexed_files: files,
            last_indexed_ms: 0,
            manifest_digest: String::new(),
        })
    }

    async fn find_symbol(&self, q: &SymbolQuery) -> Result<Vec<Symbol>> {
        Ok(self.ensure_built().await?.find_symbol(q))
    }

    async fn implementations(&self, iface: &SymbolRef) -> Result<Vec<Symbol>> {
        Ok(self.ensure_built().await?.implementations(iface))
    }

    async fn interface_of(&self, ty: &SymbolRef) -> Result<Vec<Symbol>> {
        Ok(self.ensure_built().await?.interface_of(ty))
    }

    async fn callers(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        Ok(self.ensure_built().await?.callers(target, hops))
    }

    async fn callees(&self, target: &SymbolRef, hops: u32) -> Result<AstCallGraph> {
        Ok(self.ensure_built().await?.callees(target, hops))
    }

    async fn callchain(
        &self,
        from: &SymbolRef,
        to: &SymbolRef,
        max_paths: u32,
    ) -> Result<Vec<agent_core::CallPath>> {
        Ok(self.ensure_built().await?.callchain(from, to, max_paths))
    }

    async fn blast_radius(&self, changed: &[String], hops: u32) -> Result<AstCallGraph> {
        let set: HashSet<String> = changed.iter().cloned().collect();
        Ok(self.ensure_built().await?.blast_radius(&set, hops))
    }

    async fn dependency_path(&self, from_pkg: &str, to_pkg: &str) -> Result<Vec<String>> {
        Ok(self.ensure_built().await?.dependency_path(from_pkg, to_pkg))
    }
}

fn trunc(s: &str) -> String {
    s.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ExecOutput, SandboxCapabilities, SymbolQuery};

    /// A `Sandbox` double that returns one canned capture for any command.
    struct FakeSandbox(ExecOutput);

    #[async_trait::async_trait]
    impl Sandbox for FakeSandbox {
        async fn exec(&self, _spec: &ExecSpec) -> Result<ExecOutput> {
            Ok(self.0.clone())
        }
        fn capabilities(&self) -> SandboxCapabilities {
            SandboxCapabilities::default()
        }
    }

    fn engine(out: ExecOutput) -> GoAst {
        GoAst::new(Arc::new(FakeSandbox(out)), agent_testkit::tempdir())
    }

    fn capture(exit: i32, stdout: &str, stderr: &str, timed_out: bool) -> ExecOutput {
        ExecOutput {
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit_code: exit,
            timed_out,
        }
    }

    const SAMPLE: &str = r#"{"symbols":[
        {"id":0,"kind":"interface","name":"Greeter","package":"p","file":"a.go","line":1,"exported":true},
        {"id":1,"kind":"struct","name":"Polite","package":"p","file":"a.go","line":2,"exported":true}
    ],"implements":[{"type_id":1,"interface_id":0}],"edges":[],"imports":[],
    "packages":[{"package":"p","files":1,"exported_fns":0,"types":2}]}"#;

    fn q(name: &str) -> SymbolQuery {
        SymbolQuery {
            name: name.into(),
            kind: None,
            package: None,
            exact: false,
            limit: 10,
        }
    }

    #[tokio::test]
    async fn positive_builds_lazily_and_answers_implementations() {
        let e = engine(capture(0, SAMPLE, "", false));
        let impls = e
            .implementations(&SymbolRef::name("Greeter"))
            .await
            .unwrap();
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].name, "Polite");
    }

    #[tokio::test]
    async fn positive_status_missing_until_first_build_then_fresh() {
        let e = engine(capture(0, SAMPLE, "", false));
        assert_eq!(e.status().await.unwrap().state, IndexState::Missing);
        e.reindex(&|_p| {}).await.unwrap();
        assert_eq!(e.status().await.unwrap().state, IndexState::Fresh);
    }

    #[tokio::test]
    async fn negative_helper_missing_exit_127_is_ast_error() {
        let e = engine(capture(127, "", "", false));
        let err = e.find_symbol(&q("x")).await.unwrap_err();
        assert!(matches!(err, Error::Ast(_)));
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[tokio::test]
    async fn negative_nonzero_exit_surfaces_stderr() {
        let e = engine(capture(1, "", "boom", false));
        let err = e.find_symbol(&q("x")).await.unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
    }

    #[tokio::test]
    async fn negative_timeout_is_ast_error() {
        let e = engine(capture(0, "", "", true));
        let err = e.find_symbol(&q("x")).await.unwrap_err();
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[tokio::test]
    async fn adversarial_unparseable_output_is_ast_error_not_panic() {
        let e = engine(capture(0, "this is not json", "", false));
        let err = e.find_symbol(&q("x")).await.unwrap_err();
        assert!(err.to_string().contains("unparseable"), "{err}");
    }

    #[tokio::test]
    async fn corner_unsupported_verbs_gate_is_not_hit_for_go() {
        // GoAst advertises every verb; a call-graph verb resolves rather than
        // returning the capability-gated default.
        let e = engine(capture(0, SAMPLE, "", false));
        assert!(e.capabilities().supports(AstVerb::Callers));
        // No edges in SAMPLE ⇒ empty but Ok (not an Unsupported error).
        let cg = e.callers(&SymbolRef::name("Polite"), 4).await.unwrap();
        assert_eq!(cg.roots, vec![1]);
    }
}
