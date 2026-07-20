//! Test helpers for asserting **observability**: that an operation moved a metric
//! and/or emitted a span. Every seam is metered + traced (see
//! `agent-runtime/src/metered.rs` and the gRPC span tree), so a feature test can
//! prove its code path is observable, not just correct.

use std::sync::{Arc, Mutex};

use agent_metrics::Metrics;

/// Snapshots a [`Metrics`] registry so a test can assert a specific metric moved
/// across an action. It reads only the public Prometheus **text exposition**
/// ([`Metrics::encode_text`]) — no access to registry internals — so it works for
/// any counter/histogram/gauge by name.
///
/// ```ignore
/// let probe = MetricsProbe::new(&metrics);
/// tool.execute(args, &ctx).await?;            // a metered tool
/// assert!(probe.delta(&metrics, "agent_tool_exec_seconds_count", Some("edit")) >= 1.0);
/// ```
pub struct MetricsProbe {
    before: String,
}

impl MetricsProbe {
    /// Snapshot the registry as it is now.
    pub fn new(metrics: &Metrics) -> Self {
        Self {
            before: metrics.encode_text(),
        }
    }

    /// How much `metric` increased since [`MetricsProbe::new`]. Sums every sample
    /// line whose metric name is exactly `metric` and (when `label` is `Some`)
    /// whose `{…}` label set contains that `key="value"` substring. For a
    /// histogram pass the `_count` (or `_sum`) series name; for a gauge this is the
    /// signed change.
    pub fn delta(&self, metrics: &Metrics, metric: &str, label: Option<&str>) -> f64 {
        let now = metrics.encode_text();
        sum_samples(&now, metric, label) - sum_samples(&self.before, metric, label)
    }
}

/// Sum the values of Prometheus text-exposition sample lines matching `metric`
/// (exact name) and, if given, containing `label` in their `{…}` set.
fn sum_samples(text: &str, metric: &str, label: Option<&str>) -> f64 {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            // `name{labels} value`  or  `name value`
            let (name_and_labels, value) = line.rsplit_once(char::is_whitespace)?;
            let (name, labels) = match name_and_labels.split_once('{') {
                Some((n, rest)) => (n, rest),
                None => (name_and_labels, ""),
            };
            if name != metric {
                return None;
            }
            if let Some(want) = label {
                if !labels.contains(want) {
                    return None;
                }
            }
            value.trim().parse::<f64>().ok()
        })
        .sum()
}

/// Run `f` with a subscriber that records the **name of every span created**, and
/// return those names in creation order. Lets a test assert a code path emitted an
/// expected span (e.g. `skill.load`) without a live OTLP collector.
pub fn captured_spans<F: FnOnce()>(f: F) -> Vec<String> {
    use tracing_subscriber::layer::SubscriberExt;

    let names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(SpanCollector(names.clone()));
    tracing::subscriber::with_default(subscriber, f);
    let collected = names.lock().expect("span collector poisoned").clone();
    collected
}

/// A minimal `tracing` layer that appends each new span's name to a shared vec.
struct SpanCollector(Arc<Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for SpanCollector {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if let Ok(mut v) = self.0.lock() {
            v.push(attrs.metadata().name().to_string());
        }
    }
}

/// One captured span field: `(span_name, field_name, value)`.
pub type SpanField = (String, String, String);

/// Run `f` with a subscriber that records span **fields** — both those set at
/// creation and those attached later with `Span::record(...)` — as
/// `(span_name, field, value)` tuples. Lets a test assert an *attribute* landed
/// on a span (e.g. `policy.authorize` recorded `decision = "deny"`), not just that
/// the span exists.
pub fn captured_span_fields<F: FnOnce()>(f: F) -> Vec<SpanField> {
    use tracing_subscriber::layer::SubscriberExt;

    let fields: Arc<Mutex<Vec<SpanField>>> = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(FieldCollector(fields.clone()));
    tracing::subscriber::with_default(subscriber, f);
    let collected = fields.lock().expect("field collector poisoned").clone();
    collected
}

/// Captures span fields at creation (`on_new_span`) and on later `record`
/// (`on_record`), keyed by span name.
struct FieldCollector(Arc<Mutex<Vec<SpanField>>>);

impl<S> tracing_subscriber::Layer<S> for FieldCollector
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let name = attrs.metadata().name().to_string();
        let mut v = Visitor {
            name,
            out: self.0.clone(),
        };
        attrs.record(&mut v);
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let name = ctx
            .span(id)
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        let mut v = Visitor {
            name,
            out: self.0.clone(),
        };
        values.record(&mut v);
    }
}

/// Records each visited field as `(span, field, value)`; skips `Empty` fields
/// (they surface later via `on_record`).
struct Visitor {
    name: String,
    out: Arc<Mutex<Vec<SpanField>>>,
}

impl Visitor {
    fn push(&mut self, field: &tracing::field::Field, value: String) {
        if let Ok(mut v) = self.out.lock() {
            v.push((self.name.clone(), field.name().to_string(), value));
        }
    }
}

impl tracing::field::Visit for Visitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.push(field, value.to_string());
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.push(field, value.to_string());
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.push(field, value.to_string());
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.push(field, format!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_probe_measures_counter_delta() {
        let m = Metrics::new();
        let probe = MetricsProbe::new(&m);
        m.on_tool_exec("edit", 0.001);
        m.on_tool_exec("edit", 0.002);
        m.on_tool_exec("bash", 0.001);
        // `agent_tool_exec_seconds` is a histogram → assert on its `_count` series.
        assert_eq!(
            probe.delta(&m, "agent_tool_exec_seconds_count", Some("tool=\"edit\"")),
            2.0
        );
        assert_eq!(
            probe.delta(&m, "agent_tool_exec_seconds_count", Some("tool=\"bash\"")),
            1.0
        );
        // No label → sums across every tool label.
        assert_eq!(probe.delta(&m, "agent_tool_exec_seconds_count", None), 3.0);
    }

    #[test]
    fn captured_spans_records_created_spans() {
        let spans = captured_spans(|| {
            let s = tracing::info_span!("skill.load");
            let _e = s.enter();
            tracing::info_span!("inner").in_scope(|| {});
        });
        assert!(spans.contains(&"skill.load".to_string()));
        assert!(spans.contains(&"inner".to_string()));
    }

    // Captures both creation-time fields and fields attached later via `record`.
    // Uses a callsite unique to this test so global interest caching can't be
    // poisoned by a subscriber-less test hitting the same callsite.
    #[test]
    fn captured_span_fields_records_created_and_recorded() {
        let fields = captured_span_fields(|| {
            let s = tracing::info_span!(
                "observe.fieldtest",
                at_create = "yes",
                later = tracing::field::Empty
            );
            let _e = s.enter();
            s.record("later", "recorded");
        });
        assert!(
            fields
                .iter()
                .any(|(sp, f, v)| sp == "observe.fieldtest" && f == "at_create" && v == "yes"),
            "creation field missing: {fields:?}"
        );
        assert!(
            fields
                .iter()
                .any(|(sp, f, v)| sp == "observe.fieldtest" && f == "later" && v == "recorded"),
            "recorded field missing: {fields:?}"
        );
    }
}
