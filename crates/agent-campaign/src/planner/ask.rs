//! One schema-constrained question to the model with a bounded repair loop
//! (`03-decomposition.md` step 3).
//!
//! The same shape as `agent_runtime::structured::complete_structured`, owned here
//! because the runtime crate will depend on this one (CP-04) and because the
//! planner needs two things that loop lacks: the summed [`Usage`] across every
//! round-trip (it is the attempt's `tokens`, counted against `max_plan_tokens`) and
//! a byte cap on the body **before** it is parsed (`adversarial_huge_response`).
//!
//! Providers advertising `supports_response_format` are steered natively; the rest
//! get the schema as a trailing system directive. Either way the Draft-07 seam
//! validates the answer, and a mismatch is fed back as an assistant / user pair at
//! most `max_repairs` times.

use super::schema::MAX_RESPONSE_BYTES;
use agent_core::campaign::{truncate_chars, TokenUsage, MAX_ERROR};
use agent_core::{
    CompletionRequest, LlmProvider, Message, OutputSchema, ResponseFormat, Usage, Verdict,
};
use serde_json::Value;

/// A validated answer plus what it cost.
#[derive(Debug, Clone)]
pub struct Asked {
    pub value: Value,
    pub tokens: TokenUsage,
    /// Provider round-trips.
    pub calls: usize,
    /// Repair turns (`calls - 1` on success).
    pub repairs: usize,
}

/// Why no answer was accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskFailure {
    /// The provider itself failed (network, auth, 5xx …); nothing was repaired.
    Provider(String),
    /// A response body over [`MAX_RESPONSE_BYTES`]; refused unparsed, no repair.
    TooLarge(usize),
    /// Every attempt failed validation; carries the last validator message.
    Exhausted(String),
}

/// A failed ask still reports what it cost, so the attempt row can carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskError {
    pub failure: AskFailure,
    pub tokens: TokenUsage,
    pub calls: usize,
    pub repairs: usize,
}

impl std::fmt::Display for AskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.failure {
            AskFailure::Provider(e) => write!(f, "provider: {e}"),
            AskFailure::TooLarge(bytes) => {
                write!(f, "response: {bytes} bytes, over {MAX_RESPONSE_BYTES}")
            }
            AskFailure::Exhausted(last) => {
                write!(f, "schema: after {} repair(s): {last}", self.repairs)
            }
        }
    }
}

/// Ask once, repair up to `max_repairs` times, return the first value that
/// validates against `schema`.
pub async fn ask_structured(
    provider: &dyn LlmProvider,
    validator: &dyn OutputSchema,
    mut request: CompletionRequest,
    schema: &Value,
    max_repairs: usize,
) -> Result<Asked, AskError> {
    request.response_format = Some(ResponseFormat {
        schema: schema.clone(),
        strict: true,
        name: Some("campaign_decision".to_string()),
    });
    if !provider.capabilities().supports_response_format {
        request
            .messages
            .push(Message::system(schema_directive(schema)));
    }

    let mut tokens = TokenUsage::default();
    let mut calls = 0usize;
    let mut repairs = 0usize;
    loop {
        let resp = match provider.complete(request.clone()).await {
            Ok(r) => r,
            Err(e) => {
                return Err(AskError {
                    failure: AskFailure::Provider(truncate_chars(&e.to_string(), MAX_ERROR)),
                    tokens,
                    calls,
                    repairs,
                });
            }
        };
        calls += 1;
        add_usage(&mut tokens, resp.usage.as_ref());
        let text = resp.message.content_text();
        if text.len() > MAX_RESPONSE_BYTES {
            return Err(AskError {
                failure: AskFailure::TooLarge(text.len()),
                tokens,
                calls,
                repairs,
            });
        }
        let (why, correction) = match serde_json::from_str::<Value>(strip_fences(&text)) {
            Ok(value) => {
                let verdict = validator.validate(schema, &value);
                if verdict.ok {
                    return Ok(Asked {
                        value,
                        tokens,
                        calls,
                        repairs,
                    });
                }
                let why = truncate_chars(&verdict.errors.join("; "), MAX_ERROR);
                let correction = repair_prompt(&verdict);
                (why, correction)
            }
            Err(_) => (
                "not valid JSON".to_string(),
                "Your previous output was not valid JSON. Return ONLY a JSON object matching \
                 the schema — no prose, no code fences."
                    .to_string(),
            ),
        };
        if repairs >= max_repairs {
            return Err(AskError {
                failure: AskFailure::Exhausted(why),
                tokens,
                calls,
                repairs,
            });
        }
        request.messages.push(Message::assistant(text));
        request.messages.push(Message::user(correction));
        repairs += 1;
    }
}

/// Sum a provider's reported usage into the attempt's counts (`u32` → `i64`,
/// saturating: a hostile provider cannot overflow the total).
fn add_usage(tokens: &mut TokenUsage, usage: Option<&Usage>) {
    if let Some(u) = usage {
        tokens.tokens_in = tokens.tokens_in.saturating_add(i64::from(u.prompt_tokens));
        tokens.tokens_out = tokens
            .tokens_out
            .saturating_add(i64::from(u.completion_tokens));
    }
}

fn schema_directive(schema: &Value) -> String {
    format!(
        "You must respond with a single JSON object that validates against this JSON Schema. \
         Output only the JSON — no prose, no code fences.\nSchema:\n{}",
        serde_json::to_string(schema).unwrap_or_default()
    )
}

fn repair_prompt(verdict: &Verdict) -> String {
    format!(
        "Your previous output did not match the required JSON schema:\n{}\nReturn ONLY a \
         corrected JSON object.",
        truncate_chars(&verdict.errors.join("; "), MAX_ERROR)
    )
}

/// Remove a leading ```` ```lang ```` fence and its closing ```` ``` ````, if present.
fn strip_fences(s: &str) -> &str {
    let t = s.trim();
    let Some(after_open) = t.strip_prefix("```") else {
        return t;
    };
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
    use agent_core::{CompletionResponse, ModelCapabilities};
    use agent_testkit::{final_turn, FnProvider, ScriptedProvider};
    use agent_validate::Draft07Validator;
    use rstest::rstest;
    use serde_json::json;

    fn schema() -> Value {
        json!({"type": "object", "additionalProperties": false,
               "properties": {"n": {"type": "integer"}}, "required": ["n"]})
    }

    fn req() -> CompletionRequest {
        CompletionRequest {
            messages: vec![Message::user("go")],
            ..Default::default()
        }
    }

    fn with_usage(text: &str, prompt: u32, completion: u32) -> CompletionResponse {
        CompletionResponse {
            usage: Some(Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                ..Default::default()
            }),
            ..final_turn(text)
        }
    }

    enum Want {
        Ok {
            value: Value,
            calls: usize,
            repairs: usize,
        },
        Err {
            failure: AskFailure,
            calls: usize,
            repairs: usize,
        },
    }

    #[rstest]
    #[case::positive_pass(vec![r#"{"n": 7}"#.to_string()], 2,
        Want::Ok { value: json!({"n": 7}), calls: 1, repairs: 0 })]
    #[case::positive_repair_once(vec![r#"{"n": "x"}"#.into(), r#"{"n": 7}"#.into()], 2,
        Want::Ok { value: json!({"n": 7}), calls: 2, repairs: 1 })]
    #[case::positive_repair_twice(vec!["nope".into(), r#"{"n": "x"}"#.into(), r#"{"n": 1}"#.into()], 2,
        Want::Ok { value: json!({"n": 1}), calls: 3, repairs: 2 })]
    #[case::corner_fenced_json(vec!["```json\n{\"n\": 3}\n```".into()], 2,
        Want::Ok { value: json!({"n": 3}), calls: 1, repairs: 0 })]
    #[case::negative_exhausted(vec![r#"{"n": "x"}"#.into(); 3], 2,
        Want::Err { failure: AskFailure::Exhausted(String::new()), calls: 3, repairs: 2 })]
    #[case::negative_no_budget(vec!["{}".into()], 0,
        Want::Err { failure: AskFailure::Exhausted(String::new()), calls: 1, repairs: 0 })]
    #[case::negative_unparseable_exhausted(vec!["not json".into(); 2], 1,
        Want::Err { failure: AskFailure::Exhausted("not valid JSON".into()), calls: 2, repairs: 1 })]
    #[case::adversarial_extra_key_exhausted(vec![r#"{"n": 1, "tool_calls": []}"#.into(); 2], 1,
        Want::Err { failure: AskFailure::Exhausted(String::new()), calls: 2, repairs: 1 })]
    #[case::adversarial_huge_response(vec!["x".repeat(MAX_RESPONSE_BYTES + 1)], 2,
        Want::Err { failure: AskFailure::TooLarge(MAX_RESPONSE_BYTES + 1), calls: 1, repairs: 0 })]
    #[case::boundary_response_at_cap(
        vec![format!("{{\"n\": {}}}", "1".repeat(MAX_RESPONSE_BYTES - 7))], 0,
        Want::Err { failure: AskFailure::Exhausted(String::new()), calls: 1, repairs: 0 })]
    #[tokio::test]
    async fn ask_rows(#[case] script: Vec<String>, #[case] max_repairs: usize, #[case] want: Want) {
        let provider = ScriptedProvider::new(script.into_iter().map(final_turn).collect());
        let validator = Draft07Validator::new();
        let got = ask_structured(&provider, &validator, req(), &schema(), max_repairs).await;
        let calls_made = calls_of(&got);
        match want {
            Want::Ok {
                value,
                calls,
                repairs,
            } => {
                let a = got.unwrap();
                assert_eq!(a.value, value);
                assert_eq!((a.calls, a.repairs), (calls, repairs));
                assert_eq!(
                    a.tokens,
                    TokenUsage::default(),
                    "scripted turns carry no usage"
                );
            }
            Want::Err {
                failure,
                calls,
                repairs,
            } => {
                let e = got.unwrap_err();
                assert_eq!((e.calls, e.repairs), (calls, repairs), "{e}");
                match (&e.failure, &failure) {
                    (AskFailure::Exhausted(got), AskFailure::Exhausted(want)) => {
                        assert!(got.contains(want.as_str()), "{got:?} lacks {want:?}");
                        assert!(e.to_string().starts_with("schema: after"));
                    }
                    (got, want) => assert_eq!(got, want),
                }
            }
        }
        assert_eq!(provider.calls(), calls_made);
    }

    fn calls_of(r: &Result<Asked, AskError>) -> usize {
        match r {
            Ok(a) => a.calls,
            Err(e) => e.calls,
        }
    }

    #[tokio::test]
    async fn positive_usage_summed_across_calls() {
        let provider = ScriptedProvider::new(vec![
            with_usage(r#"{"n": "x"}"#, 100, 10),
            with_usage(r#"{"n": 1}"#, 120, 5),
        ]);
        let a = ask_structured(&provider, &Draft07Validator::new(), req(), &schema(), 1)
            .await
            .unwrap();
        assert_eq!(a.tokens, TokenUsage::new(220, 15));
        // …and on failure too.
        let provider = ScriptedProvider::new(vec![with_usage("nope", u32::MAX, u32::MAX); 2]);
        let e = ask_structured(&provider, &Draft07Validator::new(), req(), &schema(), 1)
            .await
            .unwrap_err();
        assert_eq!(
            e.tokens,
            TokenUsage::new(2 * i64::from(u32::MAX), 2 * i64::from(u32::MAX))
        );
    }

    struct FailProvider;
    #[async_trait::async_trait]
    impl LlmProvider for FailProvider {
        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities::default()
        }
        async fn complete(&self, _r: CompletionRequest) -> agent_core::Result<CompletionResponse> {
            Err(agent_core::Error::Provider("boom 503".into()))
        }
    }

    #[tokio::test]
    async fn negative_provider_error_no_repair() {
        let e = ask_structured(&FailProvider, &Draft07Validator::new(), req(), &schema(), 2)
            .await
            .unwrap_err();
        assert!(matches!(e.failure, AskFailure::Provider(ref m) if m.contains("boom 503")));
        assert_eq!((e.calls, e.repairs), (0, 0));
        assert!(e.to_string().starts_with("provider: "));
    }

    #[rstest]
    #[case::corner_directive_when_unsupported(false)]
    #[case::positive_native_when_supported(true)]
    #[tokio::test]
    async fn schema_placement(#[case] native: bool) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<CompletionRequest>::new()));
        let s2 = seen.clone();
        let provider = FnProvider::new(move |r: &CompletionRequest| {
            s2.lock().unwrap().push(r.clone());
            final_turn(r#"{"n": 1}"#)
        })
        .with_capabilities(ModelCapabilities {
            supports_response_format: native,
            ..Default::default()
        });
        ask_structured(&provider, &Draft07Validator::new(), req(), &schema(), 0)
            .await
            .unwrap();
        let reqs = seen.lock().unwrap();
        let r = &reqs[0];
        let rf = r
            .response_format
            .as_ref()
            .expect("response_format always set");
        assert_eq!(rf.schema, schema());
        assert!(rf.strict);
        assert_eq!(rf.name.as_deref(), Some("campaign_decision"));
        let directive = r.messages.iter().any(|m| {
            m.content_text()
                .contains("validates against this JSON Schema")
        });
        assert_eq!(directive, !native);
        assert_eq!(r.messages.len(), if native { 1 } else { 2 });
    }

    #[tokio::test]
    async fn positive_repair_turns_carry_the_failure() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<CompletionRequest>::new()));
        let s2 = seen.clone();
        let n = std::sync::atomic::AtomicUsize::new(0);
        let provider = FnProvider::new(move |r: &CompletionRequest| {
            s2.lock().unwrap().push(r.clone());
            match n.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                0 => final_turn(r#"{"n": "x"}"#),
                _ => final_turn(r#"{"n": 1}"#),
            }
        });
        ask_structured(&provider, &Draft07Validator::new(), req(), &schema(), 1)
            .await
            .unwrap();
        let reqs = seen.lock().unwrap();
        assert_eq!(reqs.len(), 2);
        let second = &reqs[1].messages;
        // user, directive, assistant (the bad answer), user (the correction)
        assert_eq!(second.len(), 4);
        assert_eq!(second[2].content_text(), r#"{"n": "x"}"#);
        assert!(second[3]
            .content_text()
            .contains("did not match the required JSON schema"));
    }

    #[rstest]
    #[case::plain("{\"n\":1}", "{\"n\":1}")]
    #[case::fenced_json("```json\n{\"n\":1}\n```", "{\"n\":1}")]
    #[case::fenced_bare("```\n{\"n\":1}\n```", "{\"n\":1}")]
    #[case::corner_unclosed("```json\n{\"n\":1}", "{\"n\":1}")]
    #[case::corner_whitespace("  {\"n\":1}\n", "{\"n\":1}")]
    fn strip_fences_rows(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(strip_fences(input), expected);
    }
}
