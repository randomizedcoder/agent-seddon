//! `tool-search-index` — the `search` tool over the [`SearchBackend`] seam.
//!
//! Unlike `grep`/`find` (which walk the tree live), this tool queries a
//! pre-built full-text index, so it stays fast under the high query concurrency
//! of the planning phase. It holds an `Arc<dyn SearchBackend>` wired by the
//! runtime builder (the backend is repo-rooted, so the tool ignores `cwd`).

use crate::truncate;
use agent_core::{
    Observation, Result, SearchBackend, SearchHit, SearchMode, SearchQuery, Tool, ToolContext,
    ToolSchema,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

/// The `search` tool. Construct with the wired backend via [`SearchTool::new`], or for a
/// review over a pre-PR index via [`SearchTool::for_review`].
pub struct SearchTool {
    backend: Arc<dyn SearchBackend>,
    /// Set for a review: the index is of the base branch, and these (repo-relative)
    /// files are ones the PR changes, so their hits are flagged as pre-PR content.
    pre_pr: Option<HashSet<PathBuf>>,
}

/// The flag on a hit in a file the PR changes.
const PRE_PR_FLAG: &str = "  [pre-PR: this PR changes this file]";

impl SearchTool {
    pub fn new(backend: Arc<dyn SearchBackend>) -> Self {
        Self {
            backend,
            pre_pr: None,
        }
    }

    /// The review session's `search` (review-graph 5b-ii): `backend` indexes the repo's
    /// default branch, not the PR, so the description says so and each hit in a
    /// `changed` file is flagged as pre-PR content.
    pub fn for_review(
        backend: Arc<dyn SearchBackend>,
        changed: impl IntoIterator<Item = PathBuf>,
    ) -> Self {
        Self {
            backend,
            pre_pr: Some(changed.into_iter().collect()),
        }
    }
}

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }
    fn schema(&self) -> ToolSchema {
        let description = if self.pre_pr.is_some() {
            "Fast full-text code search over an index of the repository's default branch \
             (pre-PR content: it does not include this PR's changes). Use it to find callers, \
             similar code and conventions elsewhere in the repo; read the PR's own files with \
             read_file/grep. Returns `path:line<TAB>snippet` per hit, best matches first; a \
             hit in a file this PR changes is flagged `[pre-PR]`. Modes: literal (all terms), \
             phrase (ordered), fuzzy (typo-tolerant), regex."
        } else {
            "Fast full-text code search over the repository's search index. \
             Returns `path:line<TAB>snippet` per hit, best matches first. \
             Modes: literal (all terms), phrase (ordered), fuzzy (typo-tolerant), \
             regex. Prefer this over grep for finding code during planning."
        };
        ToolSchema {
            name: "search".into(),
            description: description.into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Text to search for." },
                    "mode": {
                        "type": "string",
                        "enum": ["literal", "phrase", "fuzzy", "regex"],
                        "description": "Query interpretation (default 'literal')."
                    },
                    "limit": { "type": "integer", "description": "Max hits (default 20, max 200)." },
                    "path_globs": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict to matching paths, e.g. [\"**/*.rs\"]."
                    },
                    "lang": { "type": "string", "description": "Restrict to a language, e.g. 'rust'." },
                    "fuzzy_distance": { "type": "integer", "description": "Max edit distance for fuzzy mode (0-2)." }
                },
                "required": ["query"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let text = match args.get("query").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return Ok(Observation::error("missing string argument `query`")),
        };
        let mode = match args
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("literal")
        {
            "literal" => SearchMode::Literal,
            "phrase" => SearchMode::Phrase,
            "fuzzy" => SearchMode::Fuzzy,
            "regex" => SearchMode::Regex,
            other => {
                return Ok(Observation::error(format!(
                    "unknown search mode `{other}` (use literal|phrase|fuzzy|regex)"
                )))
            }
        };
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(20)
            .clamp(1, 200) as usize;
        let path_globs = args
            .get("path_globs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let lang = args.get("lang").and_then(Value::as_str).map(str::to_string);
        let fuzzy_distance = args
            .get("fuzzy_distance")
            .and_then(Value::as_u64)
            .map(|n| n.min(2) as u8);

        let q = SearchQuery {
            text,
            mode,
            path_globs,
            lang,
            limit,
            fuzzy_distance,
        };
        // A query error (bad regex, unsupported mode) is surfaced to the model as
        // an error observation rather than aborting the turn.
        match self.backend.query(&q).await {
            Ok(hits) => Ok(Observation::ok(format_hits(&hits, self.pre_pr.as_ref()))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

fn format_hits(hits: &[SearchHit], pre_pr: Option<&HashSet<PathBuf>>) -> String {
    if hits.is_empty() {
        return "(no matches)".into();
    }
    let mut out = String::new();
    for h in hits {
        if h.line > 0 {
            out.push_str(&format!("{}:{}\t{}", h.path.display(), h.line, h.snippet));
        } else {
            out.push_str(&format!("{}\t{}", h.path.display(), h.snippet));
        }
        if pre_pr.is_some_and(|changed| changed.contains(&h.path)) {
            out.push_str(PRE_PR_FLAG);
        }
        out.push('\n');
    }
    truncate(out)
}

/// The `index_ls` tool: list repository files straight from the search index —
/// fast, no filesystem walk — optionally filtered by path globs. Complements the
/// live `ls`/`find` walk; reflects the last index build (use `ls` for live state).
pub struct IndexLsTool {
    backend: Arc<dyn SearchBackend>,
}

impl IndexLsTool {
    pub fn new(backend: Arc<dyn SearchBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for IndexLsTool {
    fn name(&self) -> &str {
        "index_ls"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "index_ls".into(),
            description:
                "List repository files from the search index (fast — no filesystem walk). \
                          Optionally restrict to `path_globs`, e.g. [\"src/**\", \"**/*.rs\"]. \
                          Reflects the last index build; use `ls` for a live directory listing."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path_globs": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict to matching paths (empty ⇒ every indexed file)."
                    }
                }
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let globs: Vec<String> = args
            .get("path_globs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        match self.backend.list_files(&globs).await {
            Ok(paths) if paths.is_empty() => Ok(Observation::ok("(no files)")),
            Ok(paths) => {
                let mut out = String::new();
                for p in &paths {
                    out.push_str(&p.display().to_string());
                    out.push('\n');
                }
                Ok(Observation::ok(truncate(out)))
            }
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{IndexState, IndexStatus, ProgressFn, SearchCapabilities};
    use serde_json::json;
    use std::path::PathBuf;

    /// A backend that echoes a fixed hit list and records the last query.
    struct StubBackend {
        hits: Vec<SearchHit>,
    }
    #[async_trait]
    impl SearchBackend for StubBackend {
        fn capabilities(&self) -> SearchCapabilities {
            SearchCapabilities {
                backend: "stub".into(),
                modes: vec![SearchMode::Literal, SearchMode::Regex],
                content_search: true,
                scored: true,
                incremental: true,
                max_concurrent_queries: 0,
            }
        }
        async fn status(&self) -> Result<IndexStatus> {
            Ok(IndexStatus {
                state: IndexState::Fresh,
                indexed_files: 1,
                last_indexed_ms: 0,
                manifest_digest: String::new(),
            })
        }
        async fn reindex(&self, _p: ProgressFn<'_>) -> Result<IndexStatus> {
            self.status().await
        }
        async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
            if q.text == "boom" {
                return Err(agent_core::Error::Search("kaboom".into()));
            }
            Ok(self.hits.clone())
        }
    }

    fn tool(hits: Vec<SearchHit>) -> SearchTool {
        SearchTool::new(Arc::new(StubBackend { hits }))
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("/repo"),
        }
    }

    fn hit(path: &str, line: u32, snippet: &str) -> SearchHit {
        SearchHit {
            path: PathBuf::from(path),
            line,
            col_start: 0,
            col_end: 0,
            score: 1.0,
            snippet: snippet.into(),
        }
    }

    // A review's `search` is over the base branch: a hit in a file the PR changes is
    // flagged pre-PR (exact repo-relative path only), others are not, and a plain
    // `search` never flags.
    #[rstest::rstest]
    #[case::positive_changed_file_flagged("src/a.rs", 3, true, true)]
    #[case::negative_unchanged_file_not_flagged("src/b.rs", 3, true, false)]
    #[case::corner_filename_only_hit_flagged("src/a.rs", 0, true, true)]
    #[case::negative_plain_search_never_flags("src/a.rs", 3, false, false)]
    #[case::adversarial_lookalike_path_not_flagged("src/a.rs.orig", 3, true, false)]
    #[case::adversarial_dot_prefixed_path_not_flagged("./src/a.rs", 3, true, false)]
    #[tokio::test]
    async fn review_search_flags_pre_pr_hits(
        #[case] path: &str,
        #[case] line: u32,
        #[case] review: bool,
        #[case] flagged: bool,
    ) {
        let backend = Arc::new(StubBackend {
            hits: vec![hit(path, line, "fn a()")],
        });
        let t = if review {
            SearchTool::for_review(backend, [PathBuf::from("src/a.rs")])
        } else {
            SearchTool::new(backend)
        };
        let obs = t.execute(json!({"query": "a"}), &ctx()).await.unwrap();
        assert!(!obs.is_error);
        assert_eq!(obs.content.contains("[pre-PR"), flagged, "{}", obs.content);
        assert_eq!(obs.content.lines().count(), 1, "one line per hit");
        assert_eq!(
            t.schema().description.contains("pre-PR content"),
            review,
            "the review description says the index is pre-PR"
        );
    }

    #[tokio::test]
    async fn formats_hits_with_line_numbers() {
        let t = tool(vec![hit("src/main.rs", 12, "fn main()")]);
        let obs = t.execute(json!({"query": "main"}), &ctx()).await.unwrap();
        assert!(!obs.is_error);
        assert!(
            obs.content.contains("src/main.rs:12\tfn main()"),
            "{}",
            obs.content
        );
    }

    #[tokio::test]
    async fn filename_only_hit_omits_line() {
        let t = tool(vec![hit("README.md", 0, "# Title")]);
        let obs = t.execute(json!({"query": "title"}), &ctx()).await.unwrap();
        assert!(obs.content.starts_with("README.md\t"), "{}", obs.content);
    }

    #[tokio::test]
    async fn no_hits_reports_no_matches() {
        let obs = tool(vec![])
            .execute(json!({"query": "x"}), &ctx())
            .await
            .unwrap();
        assert_eq!(obs.content, "(no matches)");
    }

    #[tokio::test]
    async fn missing_query_is_error() {
        let obs = tool(vec![]).execute(json!({}), &ctx()).await.unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("query"));
    }

    #[tokio::test]
    async fn unknown_mode_is_error() {
        let obs = tool(vec![])
            .execute(json!({"query": "x", "mode": "semantic"}), &ctx())
            .await
            .unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("unknown search mode"));
    }

    #[tokio::test]
    async fn backend_error_becomes_error_observation() {
        let obs = tool(vec![])
            .execute(json!({"query": "boom"}), &ctx())
            .await
            .unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("kaboom"));
    }

    // --- index_ls ----------------------------------------------------------

    #[tokio::test]
    async fn index_ls_lists_indexed_files() {
        let backend = Arc::new(agent_testkit::FixtureSearch::new().with_files(vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("README.md"),
        ]));
        let obs = IndexLsTool::new(backend)
            .execute(json!({}), &ctx())
            .await
            .unwrap();
        assert!(!obs.is_error, "{}", obs.content);
        assert!(obs.content.contains("src/main.rs"));
        assert!(obs.content.contains("README.md"));
    }

    #[tokio::test]
    async fn index_ls_empty_reports_no_files() {
        let obs = IndexLsTool::new(Arc::new(agent_testkit::FixtureSearch::new()))
            .execute(json!({}), &ctx())
            .await
            .unwrap();
        assert_eq!(obs.content, "(no files)");
    }
}
