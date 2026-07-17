//! `ClickHouseLayer` — a `tracing_subscriber` layer that streams log events into
//! the `agent_logs` table via the telemetry writer.

use crate::rows::LogRow;
use crate::TelemetryHandle;
use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

pub struct ClickHouseLayer {
    telemetry: TelemetryHandle,
}

impl ClickHouseLayer {
    pub fn new(telemetry: TelemetryHandle) -> Self {
        Self { telemetry }
    }
}

impl<S: Subscriber> Layer<S> for ClickHouseLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let target = meta.target();

        // Never capture our own writer's diagnostics or the HTTP/CH client's
        // internals — that would create a tracing → insert → tracing loop.
        if target.starts_with("agent_telemetry")
            || target.starts_with("clickhouse")
            || target.starts_with("hyper")
        {
            return;
        }

        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let fields = if visitor.fields.is_empty() {
            String::new()
        } else {
            serde_json::to_string(&Value::Object(visitor.fields)).unwrap_or_default()
        };

        self.telemetry.record_log(LogRow::new(
            self.telemetry.session_id().to_string(),
            meta.level().to_string(),
            target.to_string(),
            visitor.message.unwrap_or_default(),
            fields,
        ));
    }
}

/// Pulls the `message` field out and collects the rest as a JSON object.
#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    fields: Map<String, Value>,
}

impl FieldVisitor {
    fn put(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            if let Value::String(s) = value {
                self.message = Some(s);
            } else {
                self.message = Some(value.to_string());
            }
        } else {
            self.fields.insert(field.name().to_string(), value);
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, Value::String(format!("{value:?}")));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::String(value.to_string()));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::from(value));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, Value::from(value));
    }
}
