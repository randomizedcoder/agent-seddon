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
use std::sync::Arc;

/// The `search` tool. Construct with the wired backend via [`SearchTool::new`].
pub struct SearchTool {
    backend: Arc<dyn SearchBackend>,
}

impl SearchTool {
    pub fn new(backend: Arc<dyn SearchBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &str {
        "search"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "search".into(),
            description: "Fast full-text code search over the repository's search index. \
                          Returns `path:line<TAB>snippet` per hit, best matches first. \
                          Modes: literal (all terms), phrase (ordered), fuzzy (typo-tolerant), \
                          regex. Prefer this over grep for finding code during planning."
                .into(),
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
            Ok(hits) => Ok(Observation::ok(format_hits(&hits))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

fn format_hits(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "(no matches)".into();
    }
    let mut out = String::new();
    for h in hits {
        if h.line > 0 {
            out.push_str(&format!("{}:{}\t{}\n", h.path.display(), h.line, h.snippet));
        } else {
            out.push_str(&format!("{}\t{}\n", h.path.display(), h.snippet));
        }
    }
    truncate(out)
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
}
