//! OTLP trace export — a `tracing` layer that batch-exports spans over OTLP/gRPC
//! to the ClickStack OpenTelemetry collector.
//!
//! This is **additive** to the ClickHouse-native sink (the [`crate::ClickHouseLayer`]
//! and [`crate::CompositeMemory`] pair): it composes as one more layer on the same
//! subscriber and is enabled independently, by a non-empty `[telemetry]
//! otlp_endpoint`. It also installs the global W3C trace-context propagator, so
//! `agent_proto::trace` can carry a trace across gRPC component boundaries and the
//! collector reassembles one end-to-end trace.

use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{Tracer, TracerProvider};
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
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(cfg.endpoint.clone())
        .build()?;

    let mut attrs = vec![KeyValue::new("service.name", cfg.service_name.clone())];
    if let Some(id) = &cfg.instance_id {
        attrs.push(KeyValue::new("service.instance.id", id.clone()));
    }

    let provider = TracerProvider::builder()
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
