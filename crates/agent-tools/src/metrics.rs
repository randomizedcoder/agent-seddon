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
    /// Per-tenant scoping of the exposition (multi-tenancy C28-1). Off (default / Tier-0)
    /// returns the whole process registry; on returns only the caller's own
    /// `(session, user)` series plus the shared label-less seam-health families.
    tenant_scoped: bool,
}

impl MetricsTool {
    pub fn new(metrics: Metrics) -> Self {
        Self {
            metrics,
            tenant_scoped: false,
        }
    }

    /// Enable per-tenant scoping of the exposition (multi-tenancy C28-1). When on, the
    /// tool returns only the caller's own `(session, user)` series — resolved from the
    /// **verified** `current_identity()`, never a tool arg — plus the shared label-less
    /// seam-health families; a missing identity fails **closed** (label-less only). Off
    /// (the default / Tier-0) is byte-identical to before: the whole registry.
    #[must_use]
    pub fn tenant_scoped(mut self, yes: bool) -> Self {
        self.tenant_scoped = yes;
        self
    }
}

/// Whether one Prometheus exposition line is visible to the caller `(session, user)`
/// under tenant scoping. A series with **neither** a `session` nor a `user` label is a
/// shared, label-less seam-health family (provider / tool-exec / search latencies) and
/// is always kept; a series carrying either label is kept only when every such label
/// **matches the caller** — so no other tenant's `(session, user)` series can be read.
/// Comment (`#`) lines are not series and are handled by the caller. Label values are
/// `safe_segment` (no quotes / commas / spaces), so splitting the `{…}` block on `,`
/// then `=` is unambiguous.
fn tenant_line_visible(line: &str, session: &str, user: &str) -> bool {
    let (Some(lb), Some(rb)) = (line.find('{'), line.rfind('}')) else {
        return true; // no label block ⇒ label-less seam-health series
    };
    if rb <= lb + 1 {
        return true; // empty `{}` ⇒ no labels
    }
    for pair in line[lb + 1..rb].split(',') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match name.trim() {
            "user" if value != user => return false,
            "session" if value != session => return false,
            _ => {}
        }
    }
    true
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

        // TENANCY (multi-tenancy C28-1, docs/design/multi-tenancy/02-data-scoping-and-rls.md):
        // when scoping is on (`[tenancy] per_tenant`), restrict the exposition to the
        // caller's own `(session, user)` series plus the shared label-less seam-health
        // families — so a prompt-injectable session can't read another tenant's
        // counters. The scope is the VERIFIED ambient identity (`current_identity()`),
        // never a tool arg. A missing identity fails CLOSED: the `("", "")` sentinel
        // matches no real tenant series, leaving only label-less health. Tier-0
        // (per_tenant off) is unchanged — the whole registry, as before.
        let scope = self.tenant_scoped.then(|| {
            agent_core::current_identity()
                .map(|k| (k.session.as_str().to_string(), k.user.as_str().to_string()))
                .unwrap_or_default()
        });

        let text = self.metrics.encode_text();
        let mut out = String::new();
        for line in text.lines() {
            let is_comment = line.starts_with('#');
            if !raw {
                // Drop HELP/TYPE comments and verbose histogram buckets by default;
                // the `_count`/`_sum` lines survive so averages are still derivable.
                if is_comment || line.contains("_bucket") {
                    continue;
                }
            }
            if let Some((session, user)) = &scope {
                // Comment lines are not series (`#` HELP/TYPE) — never tenant-filtered.
                if !is_comment && !tenant_line_visible(line, session, user) {
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

    // --- multi-tenancy C28-1: per-tenant scoping of the exposition ------------

    use agent_core::{scope, SessionKey};

    /// A registry with one tenant-labelled series per tenant (`agent_tool_calls_total`,
    /// which binds `(session, user)`), plus a shared label-less seam-health family
    /// (`agent_tool_exec_seconds`, no `session`/`user` label).
    fn two_tenants() -> Metrics {
        let m = Metrics::new();
        m.for_session("s-alice", "alice").on_tool("bash", "ok");
        m.for_session("s-bob", "bob").on_tool("edit", "ok");
        m.on_tool_exec("bash", 0.01); // label-less health series
        m
    }

    // positive: scoped to alice → alice's own `(session, user)` series present, and the
    // shared label-less health family is kept (the deliberate C28-1 policy).
    #[tokio::test]
    async fn positive_scoped_shows_own_series_and_labelless_health() {
        let tool = MetricsTool::new(two_tenants()).tenant_scoped(true);
        let obs = scope(SessionKey::parse("alice", "s-alice").unwrap(), async {
            tool.execute(json!({}), &ctx()).await.unwrap()
        })
        .await;
        assert!(
            obs.content.contains("user=\"alice\""),
            "alice sees her own series:\n{}",
            obs.content
        );
        assert!(
            obs.content.contains("agent_tool_exec_seconds"),
            "label-less seam-health families stay visible:\n{}",
            obs.content
        );
    }

    // adversarial: a prompt-injectable session scoped to alice must NOT read bob's
    // `(session, user)` series out of the shared process registry.
    #[tokio::test]
    async fn adversarial_scoped_hides_other_tenant_series() {
        let tool = MetricsTool::new(two_tenants()).tenant_scoped(true);
        let obs = scope(SessionKey::parse("alice", "s-alice").unwrap(), async {
            tool.execute(json!({}), &ctx()).await.unwrap()
        })
        .await;
        assert!(
            !obs.content.contains("user=\"bob\""),
            "bob's series must not leak to alice:\n{}",
            obs.content
        );
        assert!(!obs.content.contains("session=\"s-bob\""));
    }

    // adversarial: scoping on but NO ambient identity ⇒ fail CLOSED — no tenant-labelled
    // series at all, only the shared label-less health families.
    #[tokio::test]
    async fn adversarial_scoped_without_identity_fails_closed() {
        let tool = MetricsTool::new(two_tenants()).tenant_scoped(true);
        // No `scope(...)` wrapper ⇒ `current_identity()` is None.
        let obs = tool.execute(json!({}), &ctx()).await.unwrap();
        assert!(
            !obs.content.contains("user=\"alice\"") && !obs.content.contains("user=\"bob\""),
            "no identity ⇒ no tenant series:\n{}",
            obs.content
        );
        assert!(
            obs.content.contains("agent_tool_exec_seconds"),
            "label-less health is still shown:\n{}",
            obs.content
        );
    }

    // corner: scoping OFF (Tier-0 default) is unchanged — the whole registry, every
    // tenant's series, regardless of ambient identity.
    #[tokio::test]
    async fn corner_unscoped_tier0_shows_all_tenants() {
        let tool = MetricsTool::new(two_tenants()); // tenant_scoped == false
        let obs = scope(SessionKey::parse("alice", "s-alice").unwrap(), async {
            tool.execute(json!({}), &ctx()).await.unwrap()
        })
        .await;
        assert!(obs.content.contains("user=\"alice\""));
        assert!(
            obs.content.contains("user=\"bob\""),
            "Tier-0 (unscoped) still shows all tenants:\n{}",
            obs.content
        );
    }

    // --- tenant_line_visible unit cases (four classes + adversarial) ----------

    #[test]
    fn positive_line_visible_keeps_labelless_and_own() {
        // Label-less seam-health line (no `{...}`) is always kept.
        assert!(tenant_line_visible(
            "agent_tool_exec_seconds_count{tool=\"bash\"} 3",
            "s-alice",
            "alice"
        ));
        // Own `(session, user)` line is kept.
        assert!(tenant_line_visible(
            "agent_tool_calls_total{tool=\"bash\",status=\"ok\",session=\"s-alice\",user=\"alice\"} 1",
            "s-alice",
            "alice"
        ));
    }

    #[test]
    fn negative_line_visible_hides_other_tenant() {
        // Wrong user.
        assert!(!tenant_line_visible(
            "agent_tool_calls_total{tool=\"edit\",status=\"ok\",session=\"s-bob\",user=\"bob\"} 1",
            "s-alice",
            "alice"
        ));
        // Right user, wrong session.
        assert!(!tenant_line_visible(
            "agent_active{session=\"s-other\",user=\"alice\"} 1",
            "s-alice",
            "alice"
        ));
    }

    #[test]
    fn boundary_line_visible_empty_label_block_is_labelless() {
        assert!(tenant_line_visible("some_metric{} 0", "s-alice", "alice"));
        // A fleet series carries `user` (org) but no `session` — kept iff the org matches.
        assert!(tenant_line_visible(
            "agent_fleet_reviews_total{status=\"drafted\",user=\"alice\",repo=\"acme__web\"} 1",
            "s-alice",
            "alice"
        ));
        assert!(!tenant_line_visible(
            "agent_fleet_reviews_total{status=\"drafted\",user=\"globex\",repo=\"acme__web\"} 1",
            "s-alice",
            "alice"
        ));
    }

    #[test]
    fn adversarial_line_visible_no_value_prefix_confusion() {
        // A different tenant whose name is a prefix of the caller's must NOT match
        // (the closing quote makes the value exact).
        assert!(!tenant_line_visible(
            "agent_active{session=\"s-alice2\",user=\"alice2\"} 1",
            "s-alice",
            "alice"
        ));
    }
}
