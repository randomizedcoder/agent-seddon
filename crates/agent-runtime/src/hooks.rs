//! Built-in lifecycle hooks (parity spec 22).
//!
//! The `Hook` seam exists mainly for out-of-tree code and the (deferred) remote
//! `HookService`, but shipping a seam with no in-tree implementation is how a
//! feature ends up merged-but-unreachable. `TracingHook` is a real, useful,
//! config-selectable hook that exercises the whole path: config → builder →
//! registry → loop dispatch.

use agent_core::{CompactionInfo, Hook, HookOutcome, Message, Observation, ToolCall, WorkingSet};
use agent_metrics::Metrics;
use async_trait::async_trait;

/// Emits a structured log line and a metric at each lifecycle point.
///
/// This is the "observability sink" use case the peers ship as a plugin
/// (hermes' Langfuse hook, pi's event bus): a single place to see turn shape,
/// tool activity, and compaction without instrumenting the loop itself.
pub struct TracingHook {
    metrics: Metrics,
}

impl TracingHook {
    pub fn new(metrics: Metrics) -> Self {
        Self { metrics }
    }
}

#[async_trait]
impl Hook for TracingHook {
    fn name(&self) -> &str {
        "tracing"
    }

    async fn pre_turn(&self, working: &WorkingSet) {
        tracing::debug!(messages = working.messages.len(), "hook: turn starting");
        self.metrics.on_hook("tracing", "pre_turn");
    }

    /// Observation only — this hook never vetoes. A hook that both traces and
    /// blocks would make the trace itself load-bearing, which is the wrong
    /// coupling.
    async fn pre_tool(&self, call: &ToolCall) -> HookOutcome {
        tracing::debug!(tool = %call.name, "hook: tool starting");
        self.metrics.on_hook("tracing", "pre_tool");
        HookOutcome::Continue
    }

    async fn post_tool(&self, call: &ToolCall, obs: &Observation) {
        tracing::debug!(
            tool = %call.name,
            is_error = obs.is_error,
            media = obs.blocks.len(),
            "hook: tool finished"
        );
        self.metrics.on_hook("tracing", "post_tool");
    }

    async fn post_turn(&self, message: &Message) {
        tracing::debug!(
            tool_calls = message.tool_calls.len(),
            chars = message.content_text().len(),
            "hook: turn finished"
        );
        self.metrics.on_hook("tracing", "post_turn");
    }

    async fn on_compact(&self, info: &CompactionInfo) {
        tracing::info!(
            messages_before = info.messages_before,
            messages_after = info.messages_after,
            tokens_before = info.tokens_before,
            tokens_after = info.tokens_after,
            "hook: context compacted"
        );
        self.metrics.on_hook("tracing", "on_compact");
    }
}

/// Build the configured hooks, in config order (dispatch follows that order).
pub(crate) fn build(
    names: &[String],
    metrics: &Metrics,
) -> anyhow::Result<agent_core::HookRegistry> {
    let mut reg = agent_core::HookRegistry::new();
    for name in names {
        match name.as_str() {
            "tracing" => reg.register(std::sync::Arc::new(TracingHook::new(metrics.clone()))),
            other => anyhow::bail!("unknown [hooks] entry `{other}` (built-in: tracing)"),
        }
    }
    Ok(reg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn positive_builds_the_tracing_hook() {
        let reg = build(&["tracing".to_string()], &Metrics::new()).expect("builds");
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.names().collect::<Vec<_>>(), vec!["tracing"]);
    }

    #[test]
    fn boundary_empty_config_is_an_empty_registry() {
        let reg = build(&[], &Metrics::new()).expect("builds");
        assert!(reg.is_empty());
    }

    /// A typo must fail loudly at build time, not silently disable observability.
    #[rstest]
    #[case::negative_unknown("nope")]
    #[case::negative_typo("tracng")]
    fn negative_unknown_hook_is_rejected(#[case] name: &str) {
        let err = match build(&[name.to_string()], &Metrics::new()) {
            Ok(_) => panic!("unknown hook `{name}` must be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("unknown [hooks] entry"), "got: {err}");
    }

    /// Dispatch order is the config order, so a guard can be placed ahead of an
    /// observer and results are reproducible.
    #[tokio::test]
    async fn positive_dispatch_follows_config_order() {
        use std::sync::{Arc, Mutex};
        struct Marker(&'static str, Arc<Mutex<Vec<&'static str>>>);
        #[async_trait]
        impl Hook for Marker {
            fn name(&self) -> &str {
                self.0
            }
            async fn pre_tool(&self, _c: &ToolCall) -> HookOutcome {
                self.1.lock().unwrap().push(self.0);
                HookOutcome::Continue
            }
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut reg = agent_core::HookRegistry::new();
        reg.register(Arc::new(Marker("first", seen.clone())));
        reg.register(Arc::new(Marker("second", seen.clone())));
        let call = ToolCall {
            id: "1".into(),
            name: "t".into(),
            arguments: serde_json::json!({}),
        };
        reg.pre_tool(&call).await;
        assert_eq!(*seen.lock().unwrap(), vec!["first", "second"]);
    }

    /// First denial wins, and later hooks do not run — their side effects would
    /// otherwise assume a call that never happens.
    #[tokio::test]
    async fn positive_first_denial_short_circuits() {
        use std::sync::{Arc, Mutex};
        struct Denier;
        #[async_trait]
        impl Hook for Denier {
            fn name(&self) -> &str {
                "denier"
            }
            async fn pre_tool(&self, _c: &ToolCall) -> HookOutcome {
                HookOutcome::Deny("nope".into())
            }
        }
        struct Tracker(Arc<Mutex<bool>>);
        #[async_trait]
        impl Hook for Tracker {
            fn name(&self) -> &str {
                "tracker"
            }
            async fn pre_tool(&self, _c: &ToolCall) -> HookOutcome {
                *self.0.lock().unwrap() = true;
                HookOutcome::Continue
            }
        }
        let ran = Arc::new(Mutex::new(false));
        let mut reg = agent_core::HookRegistry::new();
        reg.register(Arc::new(Denier));
        reg.register(Arc::new(Tracker(ran.clone())));
        let call = ToolCall {
            id: "1".into(),
            name: "t".into(),
            arguments: serde_json::json!({}),
        };
        assert_eq!(reg.pre_tool(&call).await, HookOutcome::Deny("nope".into()));
        assert!(!*ran.lock().unwrap(), "a later hook ran after a denial");
    }
}
