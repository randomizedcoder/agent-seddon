//! OpenTelemetry GenAI semantic-convention attributes on the loop's spans.
//!
//! Trace UIs that understand LLM calls (HyperDX's LLM dashboard, and others that
//! follow the OTel GenAI conventions) find them by attribute name —
//! `gen_ai.operation.name`, `gen_ai.request.model`, `gen_ai.usage.*`,
//! `gen_ai.tool.name` — not by span name. So each model-call span
//! (`provider.complete` / `provider.stream` / `provider.finalize`) and each
//! `tool.execute` span also carries those names. Prompt and completion text are
//! never attached: they hold repo code and secrets, and are large.
//!
//! Every value that came from the model or the provider (finish reason, tool
//! name, call id) is cut to a bound first, and a cost that is not a finite
//! non-negative number is dropped. A failed call records a fixed status message,
//! never the error text: a provider error can echo the response body, which may
//! carry prompt fragments or credential hints.

use agent_core::{CompletionResponse, ToolCall};
use tracing::Span;

/// `gen_ai.operation.name` of a model call.
pub(crate) const OP_CHAT: &str = "chat";
/// `gen_ai.operation.name` of a tool call.
pub(crate) const OP_TOOL: &str = "execute_tool";
/// Longest provider/model/tool string stamped on a span.
pub(crate) const MAX_NAME_CHARS: usize = 128;
/// `otel.status_message` of a failed model call (the error text is not recorded).
pub(crate) const CALL_FAILED: &str = "model call failed";

/// A model-call span carrying the GenAI request attributes, with the response
/// attributes declared empty for [`observe`] to fill in.
macro_rules! chat_span {
    ($name:literal, $model:expr, $provider:expr $(, $k:ident = $v:expr)*) => {
        tracing::info_span!(
            $name,
            $($k = $v,)*
            model = $model,
            gen_ai.operation.name = $crate::genai::OP_CHAT,
            gen_ai.provider.name = %$crate::genai::bounded($provider, $crate::genai::MAX_NAME_CHARS),
            gen_ai.request.model = %$crate::genai::bounded($model, $crate::genai::MAX_NAME_CHARS),
            gen_ai.conversation.id = %$crate::genai::conversation_id(),
            gen_ai.response.finish_reasons = tracing::field::Empty,
            gen_ai.usage.input_tokens = tracing::field::Empty,
            gen_ai.usage.output_tokens = tracing::field::Empty,
            gen_ai.usage.cache_read_input_tokens = tracing::field::Empty,
            gen_ai.usage.cache_creation_input_tokens = tracing::field::Empty,
            gen_ai.usage.cost = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
            otel.status_message = tracing::field::Empty,
            error.type = tracing::field::Empty,
        )
    };
}
pub(crate) use chat_span;

/// The `tool.execute` span for one call, with the GenAI tool attributes.
pub(crate) fn tool_span(iter: usize, call: &ToolCall) -> Span {
    tracing::info_span!(
        "tool.execute",
        iter,
        tool = %call.name,
        gen_ai.operation.name = OP_TOOL,
        gen_ai.tool.name = %bounded(&call.name, MAX_NAME_CHARS),
        gen_ai.tool.call.id = %bounded(&call.id, MAX_NAME_CHARS),
        gen_ai.conversation.id = %conversation_id(),
    )
}

/// Record a model call's outcome on its [`chat_span`] and pass the result through.
pub(crate) fn observe<E>(
    span: &Span,
    result: Result<CompletionResponse, E>,
) -> Result<CompletionResponse, E> {
    match &result {
        Ok(resp) => record_response(span, resp),
        Err(_) => {
            span.record("otel.status_code", "ERROR");
            span.record("error.type", "provider_error");
            span.record("otel.status_message", CALL_FAILED);
        }
    }
    result
}

fn record_response(span: &Span, resp: &CompletionResponse) {
    span.record(
        "gen_ai.response.finish_reasons",
        bounded(&resp.finish_reason, MAX_NAME_CHARS).as_str(),
    );
    let Some(u) = &resp.usage else {
        return;
    };
    span.record("gen_ai.usage.input_tokens", u.prompt_tokens);
    span.record("gen_ai.usage.output_tokens", u.completion_tokens);
    span.record("gen_ai.usage.cache_read_input_tokens", u.cache_read_tokens);
    span.record(
        "gen_ai.usage.cache_creation_input_tokens",
        u.cache_write_tokens,
    );
    if let Some(cost) = u
        .cost
        .map(|c| c.total)
        .filter(|t| t.is_finite() && *t >= 0.0)
    {
        span.record("gen_ai.usage.cost", cost);
    }
}

/// The current session id (`gen_ai.conversation.id`), empty outside a session scope.
pub(crate) fn conversation_id() -> String {
    agent_core::current_identity()
        .map(|k| bounded(k.session.as_str(), MAX_NAME_CHARS))
        .unwrap_or_default()
}

/// The first `max` chars of `s` (never splits a char).
pub(crate) fn bounded(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Cost, Message, Usage};
    use agent_testkit::observe::captured_span_fields;
    use rstest::rstest;

    fn resp(finish: &str, usage: Option<Usage>) -> CompletionResponse {
        CompletionResponse {
            message: Message::assistant("ok"),
            finish_reason: finish.to_string(),
            usage,
        }
    }

    fn usage(cost: Option<f64>) -> Usage {
        Usage {
            prompt_tokens: 1200,
            completion_tokens: 300,
            total_tokens: 1500,
            cache_read_tokens: 800,
            cache_write_tokens: 40,
            cost: cost.map(|total| Cost {
                total,
                ..Default::default()
            }),
        }
    }

    /// The value recorded for `field` on span `name`, if any.
    fn field(fields: &[(String, String, String)], name: &str, field: &str) -> Option<String> {
        fields
            .iter()
            .rev()
            .find(|(s, f, _)| s == name && f == field)
            .map(|(_, _, v)| v.clone())
    }

    fn run(result: Result<CompletionResponse, String>) -> Vec<(String, String, String)> {
        captured_span_fields(|| {
            let span = chat_span!("provider.complete", "glm-5.3", "task-router", iter = 3usize);
            let _ = observe(&span, result);
        })
    }

    #[rstest]
    #[case::positive_request_attributes("gen_ai.operation.name", Some("chat"))]
    #[case::positive_provider("gen_ai.provider.name", Some("task-router"))]
    #[case::positive_model("gen_ai.request.model", Some("glm-5.3"))]
    #[case::positive_input_tokens("gen_ai.usage.input_tokens", Some("1200"))]
    #[case::positive_output_tokens("gen_ai.usage.output_tokens", Some("300"))]
    #[case::positive_cache_read("gen_ai.usage.cache_read_input_tokens", Some("800"))]
    #[case::positive_cache_write("gen_ai.usage.cache_creation_input_tokens", Some("40"))]
    #[case::positive_finish("gen_ai.response.finish_reasons", Some("stop"))]
    #[case::positive_cost("gen_ai.usage.cost", Some("0.25"))]
    #[case::negative_ok_is_not_an_error("otel.status_code", None)]
    #[case::corner_outside_a_session_no_conversation("gen_ai.conversation.id", Some(""))]
    fn chat_span_ok_cases(#[case] key: &str, #[case] want: Option<&str>) {
        let fields = run(Ok(resp("stop", Some(usage(Some(0.25))))));
        assert_eq!(
            field(&fields, "provider.complete", key).as_deref(),
            want,
            "{key}"
        );
    }

    #[rstest]
    #[case::positive_error_status("otel.status_code", Some("ERROR"))]
    #[case::positive_error_type("error.type", Some("provider_error"))]
    #[case::positive_error_message_is_fixed("otel.status_message", Some(CALL_FAILED))]
    #[case::negative_no_usage_on_error("gen_ai.usage.input_tokens", None)]
    #[case::negative_no_finish_on_error("gen_ai.response.finish_reasons", None)]
    fn chat_span_error_cases(#[case] key: &str, #[case] want: Option<&str>) {
        let fields = run(Err("upstream 503".to_string()));
        assert_eq!(
            field(&fields, "provider.complete", key).as_deref(),
            want,
            "{key}"
        );
    }

    #[test]
    fn adversarial_error_text_never_recorded() {
        let fields = run(Err(
            "http 401: {\"error\":\"bad key sk-live-SECRET\",\"prompt\":\"repo code\"}".to_string(),
        ));
        for (span, key, value) in &fields {
            assert!(
                !value.contains("SECRET") && !value.contains("repo code"),
                "{span}.{key} leaked the error text: {value}"
            );
        }
    }

    #[rstest]
    #[case::boundary_no_usage(None, "gen_ai.usage.input_tokens", None)]
    #[case::corner_unpriced(Some(usage(None)), "gen_ai.usage.cost", None)]
    #[case::boundary_zero_cost(Some(usage(Some(0.0))), "gen_ai.usage.cost", Some("0.0"))]
    #[case::adversarial_nan_cost_dropped(Some(usage(Some(f64::NAN))), "gen_ai.usage.cost", None)]
    #[case::adversarial_inf_cost_dropped(
        Some(usage(Some(f64::INFINITY))),
        "gen_ai.usage.cost",
        None
    )]
    #[case::adversarial_negative_cost_dropped(Some(usage(Some(-1.0))), "gen_ai.usage.cost", None)]
    fn chat_span_usage_cases(
        #[case] u: Option<Usage>,
        #[case] key: &str,
        #[case] want: Option<&str>,
    ) {
        let fields = run(Ok(resp("stop", u)));
        assert_eq!(
            field(&fields, "provider.complete", key).as_deref(),
            want,
            "{key}"
        );
    }

    #[rstest]
    #[case::positive_short("stop", "stop")]
    #[case::boundary_exactly_max(&"a".repeat(MAX_NAME_CHARS), &"a".repeat(MAX_NAME_CHARS))]
    #[case::adversarial_huge_finish_reason_cut(&"x".repeat(100_000), &"x".repeat(MAX_NAME_CHARS))]
    #[case::adversarial_multibyte_not_split(&"é".repeat(200), &"é".repeat(MAX_NAME_CHARS))]
    fn finish_reason_bound_cases(#[case] finish: &str, #[case] want: &str) {
        let fields = run(Ok(resp(finish, None)));
        assert_eq!(
            field(
                &fields,
                "provider.complete",
                "gen_ai.response.finish_reasons"
            )
            .as_deref(),
            Some(want)
        );
    }

    #[rstest]
    #[case::positive_tool("read_file", "call_1", "read_file", "call_1")]
    #[case::corner_empty_id("grep", "", "grep", "")]
    #[case::adversarial_huge_tool_name_cut(
        &"t".repeat(10_000),
        "c",
        &"t".repeat(MAX_NAME_CHARS),
        "c"
    )]
    #[case::adversarial_huge_call_id_cut(
        "grep",
        &"i".repeat(10_000),
        "grep",
        &"i".repeat(MAX_NAME_CHARS)
    )]
    fn tool_span_cases(
        #[case] name: &str,
        #[case] id: &str,
        #[case] want_name: &str,
        #[case] want_id: &str,
    ) {
        let call = ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: serde_json::Value::Null,
        };
        let fields = captured_span_fields(|| {
            let _span = tool_span(2, &call);
        });
        let get = |k| field(&fields, "tool.execute", k);
        assert_eq!(get("gen_ai.operation.name").as_deref(), Some(OP_TOOL));
        assert_eq!(get("gen_ai.tool.name").as_deref(), Some(want_name));
        assert_eq!(get("gen_ai.tool.call.id").as_deref(), Some(want_id));
    }

    #[test]
    fn positive_conversation_id_is_the_scoped_session() {
        let key = agent_core::SessionKey::local("sess-42".to_string());
        let fields = captured_span_fields(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(agent_core::scope(key, async {
                let _span = chat_span!("provider.finalize", "m", "p");
            }));
        });
        assert_eq!(
            field(&fields, "provider.finalize", "gen_ai.conversation.id").as_deref(),
            Some("sess-42")
        );
    }
}
