//! `ClickHouseLayer` — a `tracing_subscriber` layer that streams log events into
//! the `agent_logs` table via the telemetry writer.
//!
//! Beyond the event's own fields, each row inherits the `tenant`/`repo`/`pr`
//! dimensions from the **enclosing span scope** (observability track, Phase 5.5):
//! the layer captures those fields off every span into the registry's per-span
//! extensions (`on_new_span`/`on_record`), then in `on_event` walks the ancestor
//! scope (nearest first) to fill them onto the row. This is how a fleet-drain log
//! — which has no ambient `current_identity()` — still gets its owning
//! `user`/`repo`/`pr` (from the `fleet.*` span), and how every log becomes
//! filterable per tenant and per repo without touching a single call site.

use crate::rows::LogRow;
use crate::TelemetryHandle;
use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

pub struct ClickHouseLayer {
    telemetry: TelemetryHandle,
}

impl ClickHouseLayer {
    pub fn new(telemetry: TelemetryHandle) -> Self {
        Self { telemetry }
    }
}

/// The observability dimensions carried on a span, captured off its fields and
/// stashed in the registry's per-span extensions so `on_event` can read them
/// while walking the ancestor scope. Each is the raw span-field value (validated
/// only when it lands on a row, in [`resolve_dims`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct SpanDims {
    pub session: String,
    pub user: String,
    pub repo: String,
    pub pr: String,
}

impl SpanDims {
    fn is_empty(&self) -> bool {
        self.session.is_empty()
            && self.user.is_empty()
            && self.repo.is_empty()
            && self.pr.is_empty()
    }

    /// Fill any still-empty field from `other` (used to merge later `on_record`
    /// updates onto the values captured at span creation).
    fn fill_from(&mut self, other: &SpanDims) {
        if self.session.is_empty() {
            self.session.clone_from(&other.session);
        }
        if self.user.is_empty() {
            self.user.clone_from(&other.user);
        }
        if self.repo.is_empty() {
            self.repo.clone_from(&other.repo);
        }
        if self.pr.is_empty() {
            self.pr.clone_from(&other.pr);
        }
    }
}

/// The resolved identity + fleet dimensions a log row is stamped with.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDims {
    pub session: String,
    pub user: String,
    pub repo: String,
    pub pr: String,
}

/// Merge the ancestor-span dimensions (nearest first) with the ambient identity,
/// producing the final row values. Pure so it is table-testable in isolation.
///
/// Precedence:
///   * `session`/`user` — the ambient `current_identity()` is **authoritative**
///     when present (the loop path, preserving prior behaviour); otherwise they
///     come from the nearest ancestor span that carries them (the fleet path,
///     which has no ambient identity). Empty `session` falls back to the handle's.
///   * `repo`/`pr` — from the span scope only (identity has no such notion).
///
/// Every scope-derived value is re-validated with `safe_segment` at this funnel
/// (a hostile value that slipped a call site is dropped to empty, never stamped).
pub(crate) fn resolve_dims(
    scope: impl Iterator<Item = SpanDims>,
    identity: Option<(String, String)>,
    fallback_session: &str,
) -> ResolvedDims {
    // Nearest-first: keep the first non-empty value seen for each field.
    let mut merged = SpanDims::default();
    for d in scope {
        merged.fill_from(&d);
    }

    let clean = |v: String| -> String {
        if !v.is_empty() && agent_core::safe_segment(&v) {
            v
        } else {
            String::new()
        }
    };
    let scope_session = clean(merged.session);
    let scope_user = clean(merged.user);
    let repo = clean(merged.repo);
    let pr = clean(merged.pr);

    // Identity (already verified upstream) wins for session/user when present.
    let (mut session, user) = match identity {
        Some((s, u)) => (s, u),
        None => (scope_session, scope_user),
    };
    if session.is_empty() {
        session = fallback_session.to_string();
    }

    ResolvedDims {
        session,
        user,
        repo,
        pr,
    }
}

impl<S> Layer<S> for ClickHouseLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut v = DimVisitor::default();
        attrs.record(&mut v);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(v.dims);
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        let mut v = DimVisitor::default();
        values.record(&mut v);
        if v.dims.is_empty() {
            return;
        }
        if let Some(span) = ctx.span(id) {
            let mut ext = span.extensions_mut();
            match ext.get_mut::<SpanDims>() {
                // Later-recorded values fill fields left `Empty` at creation.
                Some(existing) => existing.fill_from(&v.dims),
                None => ext.insert(v.dims),
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
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

        // Pull `tenant`/`repo`/`pr` from the enclosing span scope (nearest first),
        // then overlay the ambient per-turn identity (present on the scoped loop
        // task, absent on the fleet drain path — where the span scope supplies the
        // owning tenant/repo/pr instead).
        let scope = ctx
            .event_scope(event)
            .into_iter()
            .flatten()
            .filter_map(|s| s.extensions().get::<SpanDims>().cloned());
        let identity = agent_core::current_identity()
            .map(|k| (k.session.as_str().to_string(), k.user.as_str().to_string()));
        let dims = resolve_dims(scope, identity, self.telemetry.session_id());

        self.telemetry.record_log(LogRow::new(
            dims.session,
            dims.user,
            dims.repo,
            dims.pr,
            meta.level().to_string(),
            target.to_string(),
            visitor.message.unwrap_or_default(),
            fields,
        ));
    }
}

/// Captures the observability dimensions off a span's fields, mapping the known
/// aliases onto [`SpanDims`]. Values are taken as strings (numeric `pr` included).
#[derive(Default)]
struct DimVisitor {
    dims: SpanDims,
}

impl DimVisitor {
    fn put(&mut self, name: &str, value: String) {
        // `tracing::field::Empty` never reaches a visitor, so any value here is real.
        // The `is_empty()` guards keep the first value seen per field (a no-match arm
        // falls through to `_`, same as leaving the field unset).
        match name {
            "tenant" | "user" | "user_id" if self.dims.user.is_empty() => self.dims.user = value,
            "session_id" | "session" if self.dims.session.is_empty() => self.dims.session = value,
            "repo" if self.dims.repo.is_empty() => self.dims.repo = value,
            "pr" | "pr_number" if self.dims.pr.is_empty() => self.dims.pr = value,
            _ => {}
        }
    }
}

impl Visit for DimVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // `%x`/`?x` span fields arrive as Debug; strip the quotes a `String`'s
        // Debug adds so `repo = %"acme__web"` doesn't stash `"acme__web"`.
        let s = format!("{value:?}");
        let s = s
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(&s);
        self.put(field.name(), s.to_string());
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field.name(), value.to_string());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), value.to_string());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), value.to_string());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), value.to_string());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field.name(), value.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::Msg;
    use rstest::rstest;
    use std::sync::Mutex;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    fn sd(session: &str, user: &str, repo: &str, pr: &str) -> SpanDims {
        SpanDims {
            session: session.into(),
            user: user.into(),
            repo: repo.into(),
            pr: pr.into(),
        }
    }
    fn rd(session: &str, user: &str, repo: &str, pr: &str) -> ResolvedDims {
        ResolvedDims {
            session: session.into(),
            user: user.into(),
            repo: repo.into(),
            pr: pr.into(),
        }
    }
    fn id(session: &str, user: &str) -> Option<(String, String)> {
        Some((session.into(), user.into()))
    }

    // --- resolve_dims: precedence + funnel validation (pure) -----------------
    #[rstest]
    // desc: a fleet span supplies user/repo/pr but no session, no identity → session falls
    // back to the handle, the rest come from the span.
    #[case::positive_scope_supplies_user_repo_pr(vec![sd("", "acme", "acme__web", "42")], None, rd("h", "acme", "acme__web", "42"))]
    // desc: loop path — ambient identity is authoritative for session+user; repo/pr still
    // come from the span.
    #[case::positive_identity_overrides_session_user(vec![sd("stale", "stale_u", "acme__web", "7")], id("sess1", "acme"), rd("sess1", "acme", "acme__web", "7"))]
    // desc: two nested spans both carry repo → the NEAREST ancestor's value wins.
    #[case::boundary_nearest_ancestor_wins(vec![sd("", "", "inner__y", ""), sd("", "acme", "outer__x", "1")], None, rd("h", "acme", "inner__y", "1"))]
    // desc: no span scope and no identity → handle-session fallback, empty user/repo/pr.
    #[case::negative_no_scope_no_identity(vec![], None, rd("h", "", "", ""))]
    // desc: fleet drain path (identity absent) inside a fleet span → user comes from the
    // SPAN, not "".
    #[case::corner_fleet_user_from_span(vec![sd("", "acme", "acme__web", "42")], None, rd("h", "acme", "acme__web", "42"))]
    // desc: a hostile repo segment (traversal) reaches the funnel → dropped to "" (user/pr
    // survive).
    #[case::adversarial_hostile_repo_rejected(vec![sd("", "acme", "../../etc", "42")], None, rd("h", "acme", "", "42"))]
    // desc: a hostile pr value (path chars) → dropped to "".
    #[case::adversarial_hostile_pr_rejected(vec![sd("", "acme", "acme__web", "../x")], None, rd("h", "acme", "acme__web", ""))]
    fn resolve_dims_cases(
        #[case] scope: Vec<SpanDims>,
        #[case] identity: Option<(String, String)>,
        #[case] expect: ResolvedDims,
    ) {
        assert_eq!(resolve_dims(scope.into_iter(), identity, "h"), expect);
    }

    // --- ClickHouseLayer: end-to-end log rows inherit span dims --------------
    //
    // The callsite interest cache is process-global; serialize the layer tests and
    // rebuild it under each fresh subscriber so a prior no-op subscriber can't leave a
    // span/event callsite cached-disabled (mirrors the grpc/metered span-capture tests).
    static LAYER_LOCK: Mutex<()> = Mutex::new(());

    /// Drive `f` under a `Registry + ClickHouseLayer` over a writer-less handle, and
    /// return the last captured `LogRow`.
    fn last_log(session: &str, f: impl FnOnce()) -> LogRow {
        let _guard = LAYER_LOCK.lock().unwrap();
        let (handle, mut rx) = TelemetryHandle::for_test(session);
        let subscriber = Registry::default().with(ClickHouseLayer::new(handle));
        tracing::subscriber::with_default(subscriber, || {
            tracing::callsite::rebuild_interest_cache();
            f();
        });
        let mut last = None;
        while let Ok(msg) = rx.try_recv() {
            if let Msg::Log(row) = msg {
                last = Some(row);
            }
        }
        last.expect("expected at least one captured log row")
    }

    #[test]
    fn positive_log_inherits_repo_and_pr_from_fleet_span() {
        // desc: a log emitted inside a fleet.review span (no ambient identity) inherits the
        // span's user/repo/pr; session falls back to the handle. expect: dims stamped.
        let row = last_log("handle-sess", || {
            let span = tracing::info_span!(
                "fleet.review",
                tenant = "acme",
                repo = "acme__web",
                pr = 42u64
            );
            let _e = span.enter();
            tracing::info!(target: "agent_runtime::fleet", "reviewing");
        });
        assert_eq!(row.user, "acme");
        assert_eq!(row.repo, "acme__web");
        assert_eq!(row.pr, "42");
        assert_eq!(row.session_id, "handle-sess");
        assert_eq!(row.message, "reviewing");
    }

    #[test]
    fn corner_fleet_log_gets_user_without_ambient_identity() {
        // desc: the fleet drain path has no current_identity(); the fleet span alone must
        // supply the owning user. expect: user from span, not "".
        let row = last_log("h", || {
            let span =
                tracing::info_span!("fleet.progress", tenant = "globex", repo = "globex__api");
            let _e = span.enter();
            tracing::warn!(target: "agent_runtime::fleet", "post soft-failed");
        });
        assert_eq!(row.user, "globex");
        assert_eq!(row.repo, "globex__api");
        assert_eq!(row.level, "WARN");
    }

    #[test]
    fn boundary_log_in_nested_span_takes_nearest_repo() {
        // desc: nested fleet spans both carry repo → the nearest wins; user/pr fill from the
        // outer where the inner is silent. expect: repo=inner, user/pr from outer.
        let row = last_log("h", || {
            let outer = tracing::info_span!(
                "fleet.review",
                tenant = "acme",
                repo = "outer__x",
                pr = 1u64
            );
            let _o = outer.enter();
            let inner = tracing::info_span!("fleet.progress", repo = "inner__y");
            let _i = inner.enter();
            tracing::info!(target: "agent_runtime::fleet", "beat");
        });
        assert_eq!(row.repo, "inner__y");
        assert_eq!(row.user, "acme");
        assert_eq!(row.pr, "1");
    }

    #[test]
    fn negative_log_outside_any_span_has_empty_repo_pr() {
        // desc: a log emitted outside any span, with no identity → empty user/repo/pr, the
        // handle session. expect: only the fallbacks.
        let row = last_log("handle-sess", || {
            tracing::info!(target: "agent_runtime::x", "no span");
        });
        assert_eq!(row.user, "");
        assert_eq!(row.repo, "");
        assert_eq!(row.pr, "");
        assert_eq!(row.session_id, "handle-sess");
    }

    #[test]
    fn adversarial_hostile_span_field_not_written_to_row() {
        // desc: a hostile repo segment on the span reaches the layer funnel → dropped to ""
        // before it lands on a row (user survives). expect: repo empty, user kept.
        let row = last_log("h", || {
            let span = tracing::info_span!(
                "fleet.review",
                tenant = "acme",
                repo = "../../etc",
                pr = 1u64
            );
            let _e = span.enter();
            tracing::info!(target: "agent_runtime::fleet", "x");
        });
        assert_eq!(row.repo, "", "hostile repo dropped at the funnel");
        assert_eq!(row.user, "acme");
    }
}
