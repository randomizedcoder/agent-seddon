//! `tool-session-recall` — the `session_recall` tool over the [`SearchBackend`]
//! seam (parity spec 20, cross-session recall).
//!
//! Where `search` queries the *code* index, this tool queries an index over the
//! agent's own **past sessions** (saved transcripts), so it can recall how a
//! problem was handled before instead of starting cold. It holds an
//! `Arc<dyn SearchBackend>` rooted at the sessions corpus, wired by the runtime
//! builder; each hit's `path` is a session id and its `snippet` a matching line.

use crate::truncate;
use agent_core::{
    Observation, Result, SearchBackend, SearchHit, SearchMode, SearchQuery, Tool, ToolContext,
    ToolSchema,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// The `session_recall` tool. Construct with the wired corpus backend.
pub struct SessionRecallTool {
    backend: Arc<dyn SearchBackend>,
}

impl SessionRecallTool {
    pub fn new(backend: Arc<dyn SearchBackend>) -> Self {
        Self { backend }
    }
}

#[async_trait]
impl Tool for SessionRecallTool {
    fn name(&self) -> &str {
        "session_recall"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "session_recall".into(),
            description: "Search your OWN PAST sessions (saved transcripts) by keyword — recall \
                          how a problem was solved before instead of starting from scratch. \
                          Returns the best-matching past sessions, each as a session id and a \
                          snippet of the matching turn. All keywords must appear."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Keywords to recall past sessions by (all must match)."
                    },
                    "limit": { "type": "integer", "description": "Max sessions (default 10, max 50)." }
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
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, 50) as usize;
        let q = SearchQuery {
            text,
            mode: SearchMode::Literal,
            path_globs: vec![],
            lang: None,
            limit,
            fuzzy_distance: None,
        };
        match self.backend.query(&q).await {
            Ok(hits) => Ok(Observation::ok(format_sessions(&hits))),
            Err(e) => Ok(Observation::error(e.to_string())),
        }
    }
}

/// Render ranked hits as `session <id>` records, each with its matching snippet.
fn format_sessions(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "(no matching sessions)".into();
    }
    let mut out = String::new();
    for h in hits {
        out.push_str(&format!(
            "session {}\n    {}\n",
            h.path.display(),
            h.snippet
        ));
    }
    truncate(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{IndexState, IndexStatus, ProgressFn, SearchCapabilities};
    use std::path::PathBuf;

    /// A backend that echoes a fixed hit list; errors on the query text "boom".
    struct StubBackend {
        hits: Vec<SearchHit>,
    }
    #[async_trait]
    impl SearchBackend for StubBackend {
        fn capabilities(&self) -> SearchCapabilities {
            SearchCapabilities {
                backend: "stub".into(),
                modes: vec![SearchMode::Literal],
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

    fn tool(hits: Vec<SearchHit>) -> SessionRecallTool {
        SessionRecallTool::new(Arc::new(StubBackend { hits }))
    }

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("/repo"),
        }
    }

    fn hit(session_id: &str, snippet: &str) -> SearchHit {
        SearchHit {
            path: PathBuf::from(session_id),
            line: 3,
            col_start: 0,
            col_end: 0,
            score: 1.0,
            snippet: snippet.into(),
        }
    }

    #[tokio::test]
    async fn positive_formats_session_id_and_snippet() {
        let t = tool(vec![hit("s_fix", "compact the tantivy segments")]);
        let obs = t
            .execute(json!({"query": "segment"}), &ctx())
            .await
            .unwrap();
        assert!(!obs.is_error);
        assert!(obs.content.contains("session s_fix"), "{}", obs.content);
        assert!(
            obs.content.contains("compact the tantivy segments"),
            "{}",
            obs.content
        );
    }

    #[tokio::test]
    async fn boundary_no_hits_reports_no_sessions() {
        let obs = tool(vec![])
            .execute(json!({"query": "x"}), &ctx())
            .await
            .unwrap();
        assert_eq!(obs.content, "(no matching sessions)");
    }

    #[tokio::test]
    async fn negative_missing_query_is_error() {
        let obs = tool(vec![]).execute(json!({}), &ctx()).await.unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("query"));
    }

    #[tokio::test]
    async fn negative_empty_query_is_error() {
        let obs = tool(vec![])
            .execute(json!({"query": ""}), &ctx())
            .await
            .unwrap();
        assert!(obs.is_error);
    }

    #[tokio::test]
    async fn negative_backend_error_becomes_error_observation() {
        let obs = tool(vec![])
            .execute(json!({"query": "boom"}), &ctx())
            .await
            .unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("kaboom"));
    }

    // The limit is clamped into 1..=50 before it reaches the backend.
    #[rstest::rstest]
    #[case::default(json!({"query": "x"}), 10)]
    #[case::clamps_high(json!({"query": "x", "limit": 999}), 50)]
    #[case::clamps_zero(json!({"query": "x", "limit": 0}), 1)]
    #[tokio::test]
    async fn boundary_limit_is_clamped(#[case] args: Value, #[case] expected: usize) {
        // A backend that records the limit it was asked for.
        use std::sync::Mutex;
        struct RecordLimit(Arc<Mutex<usize>>);
        #[async_trait]
        impl SearchBackend for RecordLimit {
            fn capabilities(&self) -> SearchCapabilities {
                SearchCapabilities {
                    backend: "rec".into(),
                    modes: vec![SearchMode::Literal],
                    content_search: true,
                    scored: true,
                    incremental: true,
                    max_concurrent_queries: 0,
                }
            }
            async fn status(&self) -> Result<IndexStatus> {
                Ok(IndexStatus {
                    state: IndexState::Fresh,
                    indexed_files: 0,
                    last_indexed_ms: 0,
                    manifest_digest: String::new(),
                })
            }
            async fn reindex(&self, _p: ProgressFn<'_>) -> Result<IndexStatus> {
                self.status().await
            }
            async fn query(&self, q: &SearchQuery) -> Result<Vec<SearchHit>> {
                *self.0.lock().unwrap() = q.limit;
                Ok(vec![])
            }
        }
        let seen = Arc::new(Mutex::new(0));
        let t = SessionRecallTool::new(Arc::new(RecordLimit(seen.clone())));
        t.execute(args, &ctx()).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), expected);
    }
}
