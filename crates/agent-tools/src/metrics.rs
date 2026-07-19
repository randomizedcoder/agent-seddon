//! `tool-metrics` — the `metrics` tool: let the agent inspect its own performance.
//!
//! The whole harness records into one shared [`agent_metrics::Metrics`] registry
//! (the same series Prometheus scrapes and Grafana charts). This tool holds a
//! clone of that registry and returns its current text exposition, so the model
//! can read its own counters/latencies **in-process** — no HTTP, and it works even
//! when the `/metrics` endpoint and the Grafana/Prometheus stack aren't running.
//! For rates and p95s over time, point a human at Grafana (see docs/observability.md).

use crate::truncate;
use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use agent_metrics::Metrics;
use async_trait::async_trait;
use serde_json::{json, Value};

/// The `metrics` tool. Construct with the shared registry via [`MetricsTool::new`].
pub struct MetricsTool {
    metrics: Metrics,
}

impl MetricsTool {
    pub fn new(metrics: Metrics) -> Self {
        Self { metrics }
    }
}

#[async_trait]
impl Tool for MetricsTool {
    fn name(&self) -> &str {
        "metrics"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "metrics".into(),
            description: "Inspect THIS agent's own live performance metrics — the same Prometheus \
                          series Grafana charts. Covers run/iteration/token counts, per-tool and \
                          per-provider latency, and the search index (freshness, file count, \
                          reindex + query timings). Pass `filter` to narrow by substring (e.g. \
                          \"search\", \"tool\", \"provider\", \"index\"). Counters/gauges are exact; \
                          for a histogram (e.g. `..._seconds`) read the `_count` and `_sum` lines \
                          (average = sum / count). Set `raw` for histogram buckets + HELP/TYPE."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "string",
                        "description": "Only return metric lines containing this substring (e.g. 'search')."
                    },
                    "raw": {
                        "type": "boolean",
                        "description": "Include histogram _bucket lines and HELP/TYPE comments (default false)."
                    }
                }
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let filter = args.get("filter").and_then(Value::as_str);
        let raw = args.get("raw").and_then(Value::as_bool).unwrap_or(false);

        let text = self.metrics.encode_text();
        let mut out = String::new();
        for line in text.lines() {
            if !raw {
                // Drop HELP/TYPE comments and verbose histogram buckets by default;
                // the `_count`/`_sum` lines survive so averages are still derivable.
                if line.starts_with('#') || line.contains("_bucket") {
                    continue;
                }
            }
            if filter.is_some_and(|f| !line.contains(f)) {
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        if out.trim().is_empty() {
            return Ok(Observation::ok(
                "(no matching metrics recorded yet — try without a filter, or run a few turns first)",
            ));
        }
        Ok(Observation::ok(truncate(out)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: PathBuf::from("/repo"),
        }
    }

    /// A registry with a couple of series recorded across components.
    fn recorded() -> Metrics {
        let m = Metrics::new();
        m.on_tool_exec("bash", 0.01);
        m.on_search_query("tantivy", "literal", 0.002, 3);
        m.set_search_fresh("tantivy", true);
        m
    }

    #[tokio::test]
    async fn returns_recorded_series_without_help_or_buckets() {
        let obs = MetricsTool::new(recorded())
            .execute(json!({}), &ctx())
            .await
            .unwrap();
        assert!(!obs.is_error);
        assert!(obs.content.contains("agent_tool_exec_seconds_count"));
        assert!(obs.content.contains("agent_search_query_seconds_sum"));
        // buckets + HELP/TYPE are dropped by default
        assert!(!obs.content.contains("_bucket"));
        assert!(!obs.content.contains("# HELP"));
    }

    #[tokio::test]
    async fn filter_narrows_to_a_component() {
        let obs = MetricsTool::new(recorded())
            .execute(json!({ "filter": "search" }), &ctx())
            .await
            .unwrap();
        assert!(obs.content.contains("agent_search_index_fresh"));
        assert!(
            !obs.content.contains("agent_tool_exec"),
            "filter should exclude non-search lines:\n{}",
            obs.content
        );
    }

    #[tokio::test]
    async fn raw_includes_buckets() {
        let obs = MetricsTool::new(recorded())
            .execute(
                json!({ "filter": "agent_search_query_seconds", "raw": true }),
                &ctx(),
            )
            .await
            .unwrap();
        assert!(
            obs.content.contains("_bucket"),
            "raw keeps histogram buckets"
        );
    }

    #[tokio::test]
    async fn unmatched_filter_reports_nothing_recorded() {
        let obs = MetricsTool::new(Metrics::new())
            .execute(json!({ "filter": "zzz_no_such_metric" }), &ctx())
            .await
            .unwrap();
        assert!(!obs.is_error);
        assert!(obs.content.contains("no matching metrics"));
    }
}
