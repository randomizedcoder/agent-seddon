//! OTLP trace export — a `tracing` layer that batch-exports spans over OTLP/gRPC
//! to the ClickStack OpenTelemetry collector.
//!
//! This is **additive** to the ClickHouse-native sink (the [`crate::ClickHouseLayer`]
//! and [`crate::CompositeMemory`] pair): it composes as one more layer on the same
//! subscriber and is enabled independently, by a non-empty `[telemetry]
//! otlp_endpoint`. It also installs the global W3C trace-context propagator, so
//! `agent_proto::trace` can carry a trace across gRPC component boundaries and the
//! collector reassembles one end-to-end trace.

use opentelemetry::trace::{Span as _, TraceResult, TracerProvider as _};
use opentelemetry::{Context, KeyValue};
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::export::trace::SpanData;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{Span as SdkSpan, SpanProcessor, Tracer, TracerProvider};
use opentelemetry_sdk::Resource;
use tracing_subscriber::registry::LookupSpan;

/// OTLP exporter settings.
#[derive(Debug, Clone)]
pub struct OtelConfig {
    /// OTLP/gRPC endpoint, e.g. `http://localhost:4317` (the ClickStack collector).
    pub endpoint: String,
    /// The `service.name` resource attribute on exported spans.
    pub service_name: String,
    /// Optional `service.instance.id` — we pass the run's session id.
    pub instance_id: Option<String>,
    /// Extra OTLP request headers as raw comma-separated `key=value` pairs. HyperDX/
    /// ClickStack authenticates OTLP with an ingestion key (`authorization=<key>`).
    /// Empty ⇒ no headers.
    pub headers: String,
}

/// Parse `"k=v, k2=v2"` into gRPC metadata (keys lowercased; malformed pairs skipped).
fn parse_headers(raw: &str) -> tonic::metadata::MetadataMap {
    use tonic::metadata::{MetadataKey, MetadataValue};
    let mut md = tonic::metadata::MetadataMap::new();
    for pair in raw.split(',') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if let (Ok(key), Ok(val)) = (
            MetadataKey::from_bytes(k.trim().to_ascii_lowercase().as_bytes()),
            MetadataValue::try_from(v.trim()),
        ) {
            md.insert(key, val);
        }
    }
    md
}

/// Owns the tracer provider so pending spans can be flushed at shutdown. Dropping
/// it without calling [`OtelGuard::shutdown`] still flushes best-effort via the
/// provider's own `Drop`, but an explicit shutdown awaits the final export.
pub struct OtelGuard {
    provider: TracerProvider,
}

impl OtelGuard {
    /// Flush pending spans and stop the exporter. Best-effort.
    pub fn shutdown(self) {
        if let Err(e) = self.provider.shutdown() {
            tracing::debug!("otel tracer provider shutdown: {e:?}");
        }
    }
}

/// Build the OpenTelemetry tracing layer + its lifecycle guard.
///
/// Must be called from within a Tokio runtime — the batch span processor spawns a
/// background export task. Returns an error only if the exporter fails to build
/// (e.g. a malformed endpoint); the caller can then carry on without OTLP.
pub fn otlp_layer<S>(
    cfg: &OtelConfig,
) -> Result<
    (
        tracing_opentelemetry::OpenTelemetryLayer<S, Tracer>,
        OtelGuard,
    ),
    Box<dyn std::error::Error>,
>
where
    S: tracing::Subscriber + for<'span> LookupSpan<'span>,
{
    let mut exporter_builder = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(cfg.endpoint.clone());
    let headers = parse_headers(&cfg.headers);
    if !headers.is_empty() {
        exporter_builder = exporter_builder.with_metadata(headers);
    }
    let exporter = exporter_builder.build()?;

    let mut attrs = vec![KeyValue::new("service.name", cfg.service_name.clone())];
    if let Some(id) = &cfg.instance_id {
        attrs.push(KeyValue::new("service.instance.id", id.clone()));
    }
    // TENANCY (observability track, Phase 5.5): every span created **under a scope**
    // gets its `tenant`/`session` OTEL attributes from `EnrichSpanProcessor::on_start`
    // below, so an OTLP trace is filterable per tenant. `on_start` runs *inline on the
    // span-creating task* — not on the batch-export task — so reading the
    // `current_identity()` task-local here is sound (the deferred-hazard case was
    // reading it inside the exporter). Spans created before their scope is entered
    // (e.g. the `agent.turn` root) carry `tenant`/`repo`/`pr` as explicit span fields
    // instead. The run's session also rides as a process-level resource attribute.

    let provider = TracerProvider::builder()
        .with_span_processor(EnrichSpanProcessor)
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(Resource::new(attrs))
        .build();

    // Propagate W3C trace-context across (future) gRPC hops; harmless in-process.
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    let tracer = provider.tracer(cfg.service_name.clone());
    opentelemetry::global::set_tracer_provider(provider.clone());

    let layer = tracing_opentelemetry::layer().with_tracer(tracer);
    Ok((layer, OtelGuard { provider }))
}

/// A `SpanProcessor` that stamps the ambient tenant/session onto every span at
/// **start**. `on_start` runs synchronously on the task that creates the span, so
/// the `current_identity()` task-local is present for any span opened under a
/// scope (the whole agent loop and its seam sub-spans) — making the OTLP trace
/// filterable per tenant without editing each call site. Values are re-validated
/// with `safe_segment` before they are stamped (the identity is verified upstream;
/// this is defence-in-depth at the funnel). Spans created outside a scope (e.g.
/// the `agent.turn` root, opened before `agent_core::scope` is entered) get no
/// attribute here and carry their dimensions as explicit span fields instead.
#[derive(Debug, Default)]
pub(crate) struct EnrichSpanProcessor;

/// The tenant/session attributes to stamp from the ambient identity — `tenant`
/// (= `user`, C25) and `session`, each only when `safe_segment`-valid. Empty when
/// no scope is active (a span created off any scoped task). Pure, so the decision
/// is table-testable without constructing an SDK span.
pub(crate) fn identity_attributes() -> Vec<KeyValue> {
    let Some(key) = agent_core::current_identity() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let user = key.user.as_str();
    if agent_core::safe_segment(user) {
        out.push(KeyValue::new("tenant", user.to_string()));
    }
    let session = key.session.as_str();
    if agent_core::safe_segment(session) {
        out.push(KeyValue::new("session", session.to_string()));
    }
    out
}

impl SpanProcessor for EnrichSpanProcessor {
    fn on_start(&self, span: &mut SdkSpan, _cx: &Context) {
        for kv in identity_attributes() {
            span.set_attribute(kv);
        }
    }

    // Export happens on the batch exporter's own processor; ours only enriches.
    fn on_end(&self, _span: SpanData) {}
    fn force_flush(&self) -> TraceResult<()> {
        Ok(())
    }
    fn shutdown(&self) -> TraceResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::identity_attributes;
    use agent_core::SessionKey;
    use rstest::rstest;

    /// The attribute the helper produced for `key`, as `(name, value)` pairs.
    fn attrs() -> Vec<(String, String)> {
        identity_attributes()
            .into_iter()
            .map(|kv| (kv.key.as_str().to_string(), kv.value.as_str().to_string()))
            .collect()
    }

    #[rstest]
    // desc: a valid scoped identity → both `tenant` (=user) and `session` are stamped.
    #[case::positive_valid_identity("acme", "sess1", vec![("tenant", "acme"), ("session", "sess1")])]
    // desc: a hostile user segment (traversal) is dropped; the valid session still stamps.
    #[case::adversarial_hostile_user("../etc", "sess1", vec![("session", "sess1")])]
    // desc: a hostile session segment is dropped; the valid tenant still stamps.
    #[case::adversarial_hostile_session("acme", "../x", vec![("tenant", "acme")])]
    #[tokio::test]
    async fn on_start_attributes_when_scoped(
        #[case] user: &str,
        #[case] session: &str,
        #[case] expect: Vec<(&str, &str)>,
    ) {
        // `SessionKey::parse` fail-closed rejects the hostile segments, so build the
        // key field-wise (via the local ctor then overwrite) to exercise the funnel's
        // own `safe_segment` guard rather than the constructor's.
        let key = SessionKey {
            user: agent_core::UserId::new(user),
            session: agent_core::SessionId::new(session),
        };
        let got = agent_core::scope(key, async { attrs() }).await;
        let expect: Vec<(String, String)> = expect
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(got, expect);
    }

    // desc: no active scope (span created off any scoped task) → no attributes stamped.
    #[test]
    fn negative_on_start_noop_without_identity() {
        assert!(identity_attributes().is_empty());
    }
}
