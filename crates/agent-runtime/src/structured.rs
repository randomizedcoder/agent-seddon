//! Structured output: constrain a completion to a JSON schema, validate it, and
//! repair once on mismatch (parity spec 16).
//!
//! [`complete_structured`] pairs the `LlmProvider` seam with the `OutputSchema`
//! validator seam and a bounded one-shot repair loop — the piece no peer has.
//! Providers advertising `supports_response_format` are steered natively; the
//! rest get the schema injected into the prompt, so structured output works
//! everywhere and validation always gates the result.

use agent_core::{
    CompletionRequest, Error, LlmProvider, Message, OutputSchema, ResponseFormat, Result, Verdict,
};
use agent_metrics::Metrics;
use serde_json::Value;
use std::sync::Arc;

/// Build the configured validator (`[structured] validator`). Only `draft07`
/// ships today; the seam allows swapping in a stricter/native validator later.
#[cfg(feature = "structured")]
pub fn build_validator(name: &str) -> Result<Arc<dyn OutputSchema>> {
    match name {
        "draft07" => Ok(Arc::new(agent_validate::Draft07Validator::new())),
        other => Err(Error::Config(format!(
            "unknown [structured] validator `{other}` (only `draft07` is built in)"
        ))),
    }
}

/// Run a schema-constrained completion with a bounded one-shot repair loop.
///
/// Attaches `schema` to the request as its `response_format` (native path) and,
/// when the provider can't constrain natively, injects the schema into the
/// prompt. Each attempt's output is fence-stripped, parsed, and validated; on
/// mismatch the validation error is fed back and the model is re-prompted, up to
/// `max_repairs` times, before a hard [`Error::Structured`] is returned.
pub async fn complete_structured(
    provider: &dyn LlmProvider,
    validator: &dyn OutputSchema,
    mut request: CompletionRequest,
    schema: &Value,
    max_repairs: usize,
    metrics: &Metrics,
) -> Result<Value> {
    request.response_format = Some(ResponseFormat {
        schema: schema.clone(),
        strict: true,
        name: None,
    });
    // Prompt-inject the schema when the provider can't constrain natively.
    if !provider.capabilities().supports_response_format {
        request
            .messages
            .push(Message::system(schema_directive(schema)));
    }

    let mut attempt = 0usize;
    loop {
        let resp = provider.complete(request.clone()).await?;
        let text = resp.message.content_text();

        match parse_json_lenient(&text) {
            Some(value) => {
                let verdict = validator.validate(schema, &value);
                if verdict.ok {
                    metrics.on_structured_outcome(if attempt == 0 { "pass" } else { "repaired" });
                    return Ok(value);
                }
                if attempt >= max_repairs {
                    metrics.on_structured_outcome("exhausted");
                    return Err(Error::Structured(format!(
                        "output did not match schema after {attempt} repair(s): {}",
                        verdict.errors.join("; ")
                    )));
                }
                push_repair(&mut request, attempt + 1, text, &repair_prompt(&verdict));
            }
            None => {
                if attempt >= max_repairs {
                    metrics.on_structured_outcome("exhausted");
                    return Err(Error::Structured(format!(
                        "output was not valid JSON after {attempt} repair(s)"
                    )));
                }
                push_repair(
                    &mut request,
                    attempt + 1,
                    text,
                    "Your previous output was not valid JSON. Return ONLY a JSON value matching the schema — no prose, no code fences.",
                );
            }
        }
        attempt += 1;
    }
}

/// Append the failed attempt + a correction turn, under a `structured.repair` span.
fn push_repair(request: &mut CompletionRequest, attempt: usize, prev: String, correction: &str) {
    let span = tracing::info_span!("structured.repair", attempt);
    let _e = span.enter();
    request.messages.push(Message::assistant(prev));
    request.messages.push(Message::user(correction.to_string()));
}

fn schema_directive(schema: &Value) -> String {
    format!(
        "You must respond with a single JSON value that validates against this JSON Schema. \
         Output only the JSON — no prose, no code fences.\nSchema:\n{}",
        serde_json::to_string(schema).unwrap_or_default()
    )
}

fn repair_prompt(verdict: &Verdict) -> String {
    format!(
        "Your previous output did not match the required JSON schema:\n{}\nReturn ONLY a corrected JSON value.",
        verdict.errors.join("; ")
    )
}

/// Strip an optional ```/```json code fence and parse the body as JSON. Returns
/// `None` when the (stripped) text is not valid JSON.
fn parse_json_lenient(s: &str) -> Option<Value> {
    serde_json::from_str(strip_fences(s)).ok()
}

/// Remove a leading ```` ```lang ```` fence and its closing ```` ``` ````, if present.
fn strip_fences(s: &str) -> &str {
    let t = s.trim();
    let Some(after_open) = t.strip_prefix("```") else {
        return t;
    };
    // Drop the rest of the opening fence line (an optional language tag).
    let body = after_open
        .split_once('\n')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    match body.rfind("```") {
        Some(close) => body[..close].trim(),
        None => body.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::{final_turn, ScriptedProvider};
    use rstest::rstest;
    use serde_json::json;

    fn schema() -> Value {
        json!({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]})
    }
    fn strict_schema() -> Value {
        json!({"type": "object", "properties": {"n": {"type": "integer"}},
               "required": ["n"], "additionalProperties": false})
    }

    /// `Want` mirrors the spec: a final value (+ expected round-trip count) or a
    /// hard-error substring.
    enum Want<'a> {
        ValueAfter(Value, usize),
        Error(&'a str),
    }

    #[rstest]
    #[case::positive_valid_first_try(
        schema(), vec![r#"{"n": 7}"#], 1, Want::ValueAfter(json!({"n": 7}), 1))]
    #[case::positive_repair_then_pass(
        schema(), vec![r#"{"n": "x"}"#, r#"{"n": 7}"#], 1, Want::ValueAfter(json!({"n": 7}), 2))]
    #[case::negative_repair_exhausted(
        schema(), vec![r#"{"n": "x"}"#, r#"{"n": "y"}"#], 1, Want::Error("repair"))]
    #[case::corner_unparseable_then_repaired(
        schema(), vec!["here is your answer: none", r#"{"n": 1}"#], 1, Want::ValueAfter(json!({"n": 1}), 2))]
    #[case::corner_fenced_json_stripped(
        schema(), vec!["```json\n{\"n\": 3}\n```"], 1, Want::ValueAfter(json!({"n": 3}), 1))]
    #[case::negative_additionalprops_then_repaired(
        strict_schema(), vec![r#"{"n":1,"junk":2}"#, r#"{"n":1}"#], 1, Want::ValueAfter(json!({"n": 1}), 2))]
    #[case::negative_no_repair_budget(
        schema(), vec![r#"{}"#], 0, Want::Error("schema"))]
    #[case::negative_unparseable_no_budget(
        schema(), vec!["not json at all"], 0, Want::Error("JSON"))]
    #[tokio::test]
    async fn structured_repair_cases(
        #[case] schema: Value,
        #[case] script: Vec<&str>,
        #[case] max_repairs: usize,
        #[case] want: Want<'_>,
    ) {
        let provider = ScriptedProvider::new(script.into_iter().map(final_turn).collect());
        let validator = agent_validate::Draft07Validator::new();
        let metrics = Metrics::new();
        let req = CompletionRequest {
            messages: vec![Message::user("produce the value")],
            ..Default::default()
        };

        let got =
            complete_structured(&provider, &validator, req, &schema, max_repairs, &metrics).await;

        match want {
            Want::ValueAfter(value, calls) => {
                assert_eq!(got.as_ref().ok(), Some(&value), "got {got:?}");
                assert_eq!(provider.calls(), calls, "provider round-trips");
            }
            Want::Error(sub) => {
                let err = got.unwrap_err().to_string();
                assert!(err.contains(sub), "error `{err}` missing `{sub}`");
            }
        }
    }

    #[tokio::test]
    async fn structured_meters_outcomes() {
        use agent_testkit::observe::MetricsProbe;
        let metrics = Metrics::new();
        let probe = MetricsProbe::new(&metrics);
        let validator = agent_validate::Draft07Validator::new();
        let sch = schema();
        let req = || CompletionRequest {
            messages: vec![Message::user("go")],
            ..Default::default()
        };

        // pass (no repair)
        let p = ScriptedProvider::new(vec![final_turn(r#"{"n":1}"#)]);
        complete_structured(&p, &validator, req(), &sch, 1, &metrics)
            .await
            .unwrap();
        // repaired (one repair)
        let p2 = ScriptedProvider::new(vec![final_turn(r#"{"n":"x"}"#), final_turn(r#"{"n":2}"#)]);
        complete_structured(&p2, &validator, req(), &sch, 1, &metrics)
            .await
            .unwrap();
        // exhausted (no budget)
        let p3 = ScriptedProvider::new(vec![final_turn(r#"{}"#)]);
        let _ = complete_structured(&p3, &validator, req(), &sch, 0, &metrics).await;

        let d = |o: &str| probe.delta(&metrics, "agent_structured_total", Some(o));
        assert_eq!(d("outcome=\"pass\""), 1.0);
        assert_eq!(d("outcome=\"repaired\""), 1.0);
        assert_eq!(d("outcome=\"exhausted\""), 1.0);
    }

    #[test]
    fn build_validator_rejects_unknown() {
        assert!(build_validator("draft07").is_ok());
        assert!(build_validator("nope").is_err());
    }

    #[rstest]
    #[case::plain("{\"n\":1}", "{\"n\":1}")]
    #[case::fenced_json("```json\n{\"n\":1}\n```", "{\"n\":1}")]
    #[case::fenced_bare("```\n{\"n\":1}\n```", "{\"n\":1}")]
    fn strip_fences_cases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(strip_fences(input), expected);
    }
}
