//! `tool-web-search` — the `web_search` tool over the `WebSearch` seam
//! (parity spec 12).
//!
//! The backend owns transport, caching, and ranking; this tool owns the
//! model-facing surface: argument validation and bounds, and rendering results
//! into compact text. The egress destination screen lives in the `Policy` guard
//! (shared with `web_fetch`), which runs *before* this tool executes.

use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema, WebQuery, WebSearch};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Hard cap on `limit`, regardless of what the model asks for.
const MAX_LIMIT: u32 = 20;
/// Hard cap on `freshness_days` (~5 years) so a hostile value can't become a
/// nonsensical upstream parameter.
const MAX_FRESHNESS_DAYS: u32 = 1_825;

pub struct WebSearchTool {
    backend: Arc<dyn WebSearch>,
    default_limit: u32,
}

impl WebSearchTool {
    pub fn new(backend: Arc<dyn WebSearch>, default_limit: u32) -> Self {
        Self {
            backend,
            default_limit: default_limit.clamp(1, MAX_LIMIT),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_search".into(),
            description: "Search the web for current information. Returns ranked \
                          results with a title, URL, and snippet. Use when the answer \
                          depends on information newer than your training data, or on \
                          a specific live source."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query." },
                    "limit": {
                        "type": "integer",
                        "description": format!("Maximum results (1-{MAX_LIMIT})."),
                    },
                    "freshness_days": {
                        "type": "integer",
                        "description": "Only results newer than this many days (0 = any age).",
                    },
                    "backend": {
                        "type": "string",
                        "description": "Optional backend override; unknown names use the default.",
                    }
                },
                "required": ["query"]
            }),
        }
    }

    /// Read-only and side-effect free, so several searches can run concurrently.
    fn parallel_safe(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let query = match args.get("query").and_then(Value::as_str) {
            Some(q) if !q.trim().is_empty() => q.trim().to_string(),
            _ => return Ok(Observation::error("`query` must be a non-empty string")),
        };
        // Clamp rather than reject: the model supplying 1000 means "lots", and
        // failing the turn over it wastes an iteration.
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| (n as u32).clamp(1, MAX_LIMIT))
            .unwrap_or(self.default_limit);
        let freshness_days = args
            .get("freshness_days")
            .and_then(Value::as_u64)
            .map(|n| (n as u32).min(MAX_FRESHNESS_DAYS))
            .unwrap_or(0);
        let backend = args
            .get("backend")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string());

        let q = WebQuery {
            text: query.clone(),
            limit,
            freshness_days,
            backend,
        };
        match self.backend.search(&q).await {
            Ok(results) if results.is_empty() => {
                Ok(Observation::ok(format!("No results for `{query}`.")))
            }
            Ok(results) => {
                let mut out = format!("{} result(s) for `{query}`:\n", results.len());
                for (i, r) in results.iter().enumerate() {
                    out.push_str(&format!(
                        "\n{}. {}\n   {}\n   {}\n",
                        i + 1,
                        r.title,
                        r.url,
                        r.snippet
                    ));
                }
                Ok(Observation::ok(out))
            }
            // A backend failure is an error observation, not an empty result —
            // an empty result reads to the model as "nothing exists".
            Err(e) => Ok(Observation::error(format!("web search failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::ScriptedWebSearch;
    use rstest::rstest;

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: std::path::PathBuf::from("."),
        }
    }

    async fn run(tool: &WebSearchTool, args: Value) -> Observation {
        tool.execute(args, &ctx()).await.expect("tool runs")
    }

    fn tool(b: Arc<ScriptedWebSearch>) -> WebSearchTool {
        WebSearchTool::new(b, 5)
    }

    #[tokio::test]
    async fn positive_renders_results() {
        let b = Arc::new(ScriptedWebSearch::new("s").with_result("rust", "https://a.test/1"));
        let obs = run(&tool(b), json!({"query": "rust"})).await;
        assert!(!obs.is_error);
        assert!(obs.content.contains("https://a.test/1"), "{}", obs.content);
        assert!(obs.content.contains("1 result(s)"));
    }

    /// No results is a clean, explicit statement — not an error, and not silence.
    #[tokio::test]
    async fn negative_no_results_is_explicit() {
        let b = Arc::new(ScriptedWebSearch::new("s"));
        let obs = run(&tool(b), json!({"query": "nothing"})).await;
        assert!(!obs.is_error);
        assert!(obs.content.contains("No results"), "{}", obs.content);
    }

    /// A backend failure must be distinguishable from "no results".
    #[tokio::test]
    async fn negative_backend_error_is_an_error_observation() {
        let b = Arc::new(ScriptedWebSearch::new("s"));
        b.set_error(Some("no API key configured".into()));
        let obs = run(&tool(b), json!({"query": "x"})).await;
        assert!(
            obs.is_error,
            "a backend failure must not look like no results"
        );
        assert!(obs.content.contains("no API key"), "{}", obs.content);
    }

    #[rstest]
    #[case::negative_missing_query(json!({}))]
    #[case::negative_empty_query(json!({"query": "   "}))]
    #[case::negative_wrong_type(json!({"query": 42}))]
    #[tokio::test]
    async fn negative_bad_query_is_rejected(#[case] args: Value) {
        let b = Arc::new(ScriptedWebSearch::new("s"));
        let obs = run(&tool(b), args).await;
        assert!(obs.is_error);
        assert!(obs.content.contains("non-empty string"));
    }

    /// The model chooses these numbers, so they are clamped rather than trusted —
    /// and clamped rather than rejected, since failing wastes an iteration.
    #[rstest]
    #[case::adversarial_huge_limit(json!({"query":"q","limit": 100000}))]
    #[case::adversarial_zero_limit(json!({"query":"q","limit": 0}))]
    #[case::adversarial_huge_freshness(json!({"query":"q","freshness_days": 99999999}))]
    #[case::corner_negative_limit_ignored(json!({"query":"q","limit": -5}))]
    #[tokio::test]
    async fn adversarial_bounds_are_clamped_not_rejected(#[case] args: Value) {
        let b = Arc::new(ScriptedWebSearch::new("s").with_result("q", "https://a.test/1"));
        let obs = run(&tool(b), args).await;
        assert!(!obs.is_error, "must clamp, not fail: {}", obs.content);
    }

    /// Searches are read-only, so the loop may run them concurrently.
    #[test]
    fn positive_tool_is_parallel_safe() {
        let b = Arc::new(ScriptedWebSearch::new("s"));
        assert!(tool(b).parallel_safe());
    }
}
