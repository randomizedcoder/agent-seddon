//! W3C trace-context propagation over tonic gRPC metadata.
//!
//! These adapters let an OpenTelemetry propagator inject/extract the standard
//! `traceparent` / `tracestate` headers into a request's metadata, so a trace
//! started in one component (say the agent loop) continues across a gRPC hop into
//! another (a model gateway, a tool worker) and lands as a single end-to-end trace
//! in the ClickStack collector.
//!
//! They are inert until a propagator is installed
//! ([`opentelemetry::global::set_text_map_propagator`], done by the telemetry
//! init in `agent-telemetry`) and until the deferred gRPC transports wire them in:
//! [`inject_context`] from a client interceptor, [`extract_context`] on the server
//! side. Both are specified in `docs/grpc.md`.

use opentelemetry::propagation::{Extractor, Injector};
use tonic::metadata::{KeyRef, MetadataKey, MetadataMap, MetadataValue};

/// Adapts a tonic [`MetadataMap`] as an OTel [`Injector`] (client → wire).
pub struct MetadataInjector<'a>(pub &'a mut MetadataMap);

impl Injector for MetadataInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(key) = MetadataKey::from_bytes(key.as_bytes()) {
            if let Ok(val) = MetadataValue::try_from(&value) {
                self.0.insert(key, val);
            }
        }
    }
}

/// Adapts a tonic [`MetadataMap`] as an OTel [`Extractor`] (wire → server).
pub struct MetadataExtractor<'a>(pub &'a MetadataMap);

impl Extractor for MetadataExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0
            .keys()
            .map(|k| match k {
                KeyRef::Ascii(v) => v.as_str(),
                KeyRef::Binary(v) => v.as_str(),
            })
            .collect()
    }
}

/// Inject `cx` into outgoing request `meta` using the globally configured
/// propagator (a no-op if none is installed). Call from a gRPC client interceptor.
pub fn inject_context(cx: &opentelemetry::Context, meta: &mut MetadataMap) {
    opentelemetry::global::get_text_map_propagator(|p| {
        p.inject_context(cx, &mut MetadataInjector(meta));
    });
}

/// Extract a parent context from incoming request `meta`. Call on the server side
/// and make each handler's span a child of the returned context.
pub fn extract_context(meta: &MetadataMap) -> opentelemetry::Context {
    opentelemetry::global::get_text_map_propagator(|p| p.extract(&MetadataExtractor(meta)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{
        SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState,
    };
    use opentelemetry_sdk::propagation::TraceContextPropagator;

    #[test]
    fn traceparent_roundtrips_through_metadata() {
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

        let sc = SpanContext::new(
            TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap(),
            SpanId::from_hex("b7ad6b7169203331").unwrap(),
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        );
        let cx = opentelemetry::Context::new().with_remote_span_context(sc.clone());

        let mut meta = MetadataMap::new();
        inject_context(&cx, &mut meta);
        // The W3C header is present on the wire.
        assert!(meta.get("traceparent").is_some());

        // …and extracting it on the "server" side recovers the same trace/span ids.
        let got = extract_context(&meta);
        let got_sc = got.span().span_context().clone();
        assert_eq!(got_sc.trace_id(), sc.trace_id());
        assert_eq!(got_sc.span_id(), sc.span_id());
        assert!(got_sc.is_sampled());
    }
}
