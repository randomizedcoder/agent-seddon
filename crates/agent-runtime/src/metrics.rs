//! Prometheus metrics for a running agent.
//!
//! `Metrics` owns a `prometheus::Registry` and the metric handles. It is cloned
//! into the `Agent` (which increments during the loop) and kept by the CLI
//! (which serves `/metrics` and/or pushes to a Pushgateway). Instrumentation is
//! unconditional and cheap; only serving/pushing is gated by config, so when
//! metrics are disabled the registry simply goes unscraped.

use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge, Opts,
    Registry, TextEncoder,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,
    api_calls: IntCounterVec,
    api_call_seconds: HistogramVec,
    tokens: IntCounterVec,
    context_tokens: IntGauge,
    context_messages: IntGauge,
    tool_calls: IntCounterVec,
    iterations: IntCounter,
    runs: IntCounterVec,
    run_seconds: Histogram,
    active: IntGauge,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let api_calls = IntCounterVec::new(
            Opts::new("agent_api_calls_total", "LLM completion calls"),
            &["model", "finish_reason"],
        )
        .unwrap();
        let api_call_seconds = HistogramVec::new(
            HistogramOpts::new("agent_api_call_duration_seconds", "LLM call latency"),
            &["model"],
        )
        .unwrap();
        let tokens = IntCounterVec::new(
            Opts::new("agent_tokens_total", "Tokens consumed"),
            &["model", "kind"],
        )
        .unwrap();
        let context_tokens = IntGauge::new(
            "agent_context_tokens",
            "Prompt tokens of the last request (context size)",
        )
        .unwrap();
        let context_messages = IntGauge::new(
            "agent_context_messages",
            "Messages in the working set of the last request",
        )
        .unwrap();
        let tool_calls = IntCounterVec::new(
            Opts::new("agent_tool_calls_total", "Tool invocations"),
            &["tool", "status"],
        )
        .unwrap();
        let iterations =
            IntCounter::new("agent_iterations_total", "Agent loop iterations").unwrap();
        let runs = IntCounterVec::new(
            Opts::new("agent_runs_total", "Completed agent runs"),
            &["outcome"],
        )
        .unwrap();
        let run_seconds = Histogram::with_opts(HistogramOpts::new(
            "agent_run_duration_seconds",
            "Wall-clock duration of an agent run",
        ))
        .unwrap();
        let active = IntGauge::new("agent_active", "1 while a run is in progress").unwrap();

        for m in [
            Box::new(api_calls.clone()) as Box<dyn prometheus::core::Collector>,
            Box::new(api_call_seconds.clone()),
            Box::new(tokens.clone()),
            Box::new(context_tokens.clone()),
            Box::new(context_messages.clone()),
            Box::new(tool_calls.clone()),
            Box::new(iterations.clone()),
            Box::new(runs.clone()),
            Box::new(run_seconds.clone()),
            Box::new(active.clone()),
        ] {
            registry.register(m).expect("register metric");
        }

        Self {
            registry: Arc::new(registry),
            api_calls,
            api_call_seconds,
            tokens,
            context_tokens,
            context_messages,
            tool_calls,
            iterations,
            runs,
            run_seconds,
            active,
        }
    }

    /// Encode all metrics in the Prometheus text exposition format.
    pub fn encode_text(&self) -> String {
        let mut buf = Vec::new();
        let encoder = TextEncoder::new();
        let families = self.registry.gather();
        let _ = encoder.encode(&families, &mut buf);
        String::from_utf8(buf).unwrap_or_default()
    }

    // --- instrumentation helpers ------------------------------------------

    pub fn run_started(&self) {
        self.active.set(1);
    }
    pub fn run_finished(&self, outcome: &str, seconds: f64) {
        self.active.set(0);
        self.runs.with_label_values(&[outcome]).inc();
        self.run_seconds.observe(seconds);
    }
    pub fn on_iteration(&self) {
        self.iterations.inc();
    }
    pub fn on_api_call(&self, model: &str, finish_reason: &str, seconds: f64) {
        self.api_calls
            .with_label_values(&[model, finish_reason])
            .inc();
        self.api_call_seconds
            .with_label_values(&[model])
            .observe(seconds);
    }
    pub fn add_tokens(&self, model: &str, prompt: u64, completion: u64) {
        self.tokens
            .with_label_values(&[model, "prompt"])
            .inc_by(prompt);
        self.tokens
            .with_label_values(&[model, "completion"])
            .inc_by(completion);
    }
    pub fn set_context(&self, prompt_tokens: i64, messages: i64) {
        self.context_tokens.set(prompt_tokens);
        self.context_messages.set(messages);
    }
    pub fn on_tool(&self, tool: &str, status: &str) {
        self.tool_calls.with_label_values(&[tool, status]).inc();
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_incremented_metrics() {
        let m = Metrics::new();
        m.on_iteration();
        m.on_api_call("test-model", "stop", 0.5);
        m.add_tokens("test-model", 100, 20);
        m.set_context(100, 4);
        m.on_tool("bash", "ok");
        m.run_finished("success", 1.5);

        let text = m.encode_text();
        for name in [
            "agent_iterations_total",
            "agent_api_calls_total",
            "agent_tokens_total",
            "agent_context_tokens",
            "agent_tool_calls_total",
            "agent_runs_total",
        ] {
            assert!(text.contains(name), "missing metric `{name}` in:\n{text}");
        }
        assert!(text.contains("test-model"));
    }
}
