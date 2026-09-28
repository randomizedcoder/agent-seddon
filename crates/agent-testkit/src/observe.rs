//! Test helpers for asserting **observability**: that an operation moved a metric
//! and/or emitted a span. Every seam is metered + traced (see
//! `agent-runtime/src/metered.rs` and the gRPC span tree), so a feature test can
//! prove its code path is observable, not just correct.

use std::sync::{Arc, Mutex, OnceLock};

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

/// A no-op subscriber whose only job is to be **registered**: it answers
/// `register_callsite` with [`tracing::subscriber::Interest::always`] and enables
/// nothing, so it never records a span itself.
///
/// Why it exists: `tracing-core` caches per-callsite *interest*. While the process
/// has at most one live dispatcher, that cache is rebuilt from **the calling
/// thread's** default dispatcher only (`Rebuilder::JustOne`). A test that emits a
/// span with no subscriber installed can therefore stamp `Interest::never` on a
/// callsite from its own thread, *after* a concurrent [`captured_spans`] /
/// [`captured_span_fields`] has installed its collector — and the collector sees
/// nothing. Keeping a second, always-interested dispatcher alive for the whole
/// process forces the `Read` path, which ANDs every live dispatcher's interest:
/// `always ∧ anything` is never `never`, so a captured callsite is always dispatched
/// and the capture subscriber's own `enabled()` decides. Subscriber-less emits go to
/// `NoSubscriber`, which is a no-op, exactly as before.
struct AlwaysInterested;

impl tracing::Subscriber for AlwaysInterested {
    fn register_callsite(
        &self,
        _metadata: &'static tracing::Metadata<'static>,
    ) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::always()
    }
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        false
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, _event: &tracing::Event<'_>) {}
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

/// The process-wide pinned dispatcher (see [`AlwaysInterested`]). Never installed as
/// a default; constructing the `Dispatch` is what registers it.
static INTEREST_PIN: OnceLock<tracing::Dispatch> = OnceLock::new();

/// Register the always-interested dispatcher once per process, then rebuild the
/// interest cache so callsites already cached as `never` are re-evaluated. Idempotent
/// and cheap after the first call.
fn pin_interest() {
    INTEREST_PIN.get_or_init(|| {
        let d = tracing::Dispatch::new(AlwaysInterested);
        tracing::callsite::rebuild_interest_cache();
        d
    });
}

/// Run `f` with a subscriber that records the **name of every span created**, and
/// return those names in creation order. Lets a test assert a code path emitted an
/// expected span (e.g. `skill.load`) without a live OTLP collector.
///
/// Safe to run in parallel with tests that emit the same span with no subscriber:
/// see [`AlwaysInterested`]. Callers do not need their own
/// `rebuild_interest_cache()`.
pub fn captured_spans<F: FnOnce()>(f: F) -> Vec<String> {
    use tracing_subscriber::layer::SubscriberExt;

    pin_interest();
    let names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(SpanCollector(names.clone()));
    tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        f();
    });
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
///
/// Immune to the callsite-interest race like [`captured_spans`] (see
/// [`AlwaysInterested`]); callers do not need their own `rebuild_interest_cache()`.
pub fn captured_span_fields<F: FnOnce()>(f: F) -> Vec<SpanField> {
    use tracing_subscriber::layer::SubscriberExt;

    pin_interest();
    let fields: Arc<Mutex<Vec<SpanField>>> = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(FieldCollector(fields.clone()));
    tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        f();
    });
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

    // ---- callsite-interest race (the `progress::tests` flake) -------------

    /// Emit one span at a callsite unique to this test. Called first from a thread
    /// with **no** default subscriber, which is exactly what stamps `Interest::never`
    /// on the callsite under `Rebuilder::JustOne`.
    fn emit_race_span() {
        let _s = tracing::info_span!("observe.race", k = 1);
    }

    // desc: the callsite is first registered from a subscriber-less thread while the
    // capture subscriber is installed on ours; without the pin, the interest cache
    // reads `never` and the capture sees `[]` (deterministic on the old code, because
    // `with_default` is thread-local and the spawned thread has no default).
    #[test]
    fn positive_capture_survives_no_subscriber_first_registration() {
        let fields = captured_span_fields(|| {
            std::thread::spawn(emit_race_span)
                .join()
                .expect("emitter thread panicked");
            emit_race_span();
        });
        assert!(
            fields
                .iter()
                .any(|(sp, f, v)| sp == "observe.race" && f == "k" && v == "1"),
            "span lost to the interest cache: {fields:?}"
        );
    }

    // desc: two captures back to back both record — the pin registers once and stays
    // registered; nothing about the second call depends on the first.
    #[test]
    fn corner_pin_is_idempotent() {
        for i in 0..2 {
            let spans = captured_spans(|| {
                tracing::info_span!("observe.idem").in_scope(|| {});
            });
            assert!(
                spans.contains(&"observe.idem".to_string()),
                "capture {i} lost the span: {spans:?}"
            );
        }
        assert!(INTEREST_PIN.get().is_some(), "pin not registered");
    }

    // desc: the pin must not force-enable spans. A capture subscriber whose filter
    // rejects the target still yields nothing: `always ∧ never = sometimes`, so the
    // installed subscriber's own `enabled()` decides.
    #[test]
    fn negative_filtered_span_not_captured() {
        use tracing_subscriber::layer::{Layer, SubscriberExt};

        pin_interest();
        let names: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let filtered = SpanCollector(names.clone()).with_filter(
            tracing_subscriber::filter::filter_fn(|meta| meta.target() != "observe_filtered"),
        );
        let subscriber = tracing_subscriber::registry().with(filtered);
        tracing::subscriber::with_default(subscriber, || {
            tracing::callsite::rebuild_interest_cache();
            tracing::info_span!(target: "observe_filtered", "observe.rejected").in_scope(|| {});
            tracing::info_span!("observe.accepted").in_scope(|| {});
        });
        let got = names.lock().expect("poisoned").clone();
        assert!(
            !got.contains(&"observe.rejected".to_string()),
            "filtered span leaked through: {got:?}"
        );
        assert!(
            got.contains(&"observe.accepted".to_string()),
            "unfiltered span missing: {got:?}"
        );
    }
}
