//! A deterministic, model-free verifier: check a tool call's arguments against
//! the tool's JSON Schema, and ask the model to `Revise` on a mismatch.
//!
//! This is the reference `Verifier`. It needs no model, is certain (confidence
//! 1.0), and catches the class of slip we watched a live model make — calling a
//! tool with a required argument missing (mistral called `bash` with no
//! `command`). The check is deliberately SHALLOW: required-field presence plus a
//! top-level type check. That is bounded work regardless of how large or nested
//! the model's arguments are — important, because the arguments are
//! attacker-controlled (the model is prompt-injectable).
//!
//! It only ever returns `Allow` or `Revise` — never `Deny`. A schema mismatch is
//! a fixable mistake, not something to block; safety stays with `Policy`/`Scanner`.

use agent_core::{Verifier, VerifierReport, VerifyCtx, VerifyVerdict};
use async_trait::async_trait;
use serde_json::Value;

/// Longest hint we will build. The hint names the offending field; we never echo
/// the model's argument *values* into it (they are untrusted and could be huge).
const MAX_HINT: usize = 512;

pub struct SchemaVerifier;

impl SchemaVerifier {
    pub fn new() -> Self {
        Self
    }

    /// The first schema problem with `args`, or `None` when they satisfy `schema`.
    /// `schema` is the tool's `parameters` (a JSON Schema object).
    fn first_violation(schema: &Value, args: &Value) -> Option<String> {
        // We only understand object schemas — every tool takes a named-argument
        // object. A non-object schema (or none) means "no check we can make".
        let props = schema.get("properties");
        let is_object_schema =
            schema.get("type").and_then(Value::as_str) == Some("object") || props.is_some();
        if !is_object_schema {
            return None;
        }

        // The arguments must themselves be an object.
        let obj = match args {
            Value::Object(m) => m,
            _ => return Some("the arguments must be a JSON object of named fields".to_string()),
        };

        // Required fields must be present.
        if let Some(req) = schema.get("required").and_then(Value::as_array) {
            for r in req {
                if let Some(name) = r.as_str() {
                    if !obj.contains_key(name) {
                        return Some(format!("missing required argument `{name}`"));
                    }
                }
            }
        }

        // Present fields must match their declared type (when the schema declares
        // one). Absent/unknown types are not checked — we only flag a definite
        // mismatch, never guess.
        if let Some(Value::Object(properties)) = props {
            for (name, spec) in properties {
                let Some(actual) = obj.get(name) else {
                    continue; // absent optional field — required-ness handled above
                };
                if let Some(expected) = spec.get("type") {
                    if !type_matches(expected, actual) {
                        let want = type_name(expected);
                        let got = json_type_name(actual);
                        return Some(format!("argument `{name}` should be {want}, got {got}"));
                    }
                }
            }
        }
        None
    }
}

impl Default for SchemaVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Verifier for SchemaVerifier {
    fn name(&self) -> &str {
        "schema"
    }

    async fn verify(&self, ctx: &VerifyCtx<'_>) -> VerifierReport {
        // No schema (unknown tool) ⇒ nothing to check ⇒ fail open with Allow.
        let Some(schema) = ctx.tool_schema else {
            return VerifierReport::allow("schema");
        };
        match Self::first_violation(&schema.parameters, &ctx.call.arguments) {
            None => VerifierReport::allow("schema"),
            Some(problem) => {
                let mut hint = format!(
                    "the `{}` tool call does not match its schema: {problem}. \
                     Reissue the call with corrected arguments.",
                    ctx.call.name
                );
                hint.truncate(MAX_HINT);
                VerifierReport {
                    verdict: VerifyVerdict::Revise(hint),
                    confidence: 1.0, // a deterministic check is certain
                    model: "schema".to_string(),
                }
            }
        }
    }
}

/// Does `actual` satisfy the JSON-Schema `type` declaration `expected`? `expected`
/// may be a single type string or an array of accepted types (a union).
fn type_matches(expected: &Value, actual: &Value) -> bool {
    match expected {
        Value::String(t) => one_type_matches(t, actual),
        Value::Array(types) => types
            .iter()
            .filter_map(Value::as_str)
            .any(|t| one_type_matches(t, actual)),
        // An unrecognised `type` declaration ⇒ don't flag (only flag a definite
        // mismatch, never a schema we can't read).
        _ => true,
    }
}

fn one_type_matches(t: &str, v: &Value) -> bool {
    match t {
        "string" => v.is_string(),
        // JSON Schema `integer` accepts a whole number; `number` accepts any.
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "array" => v.is_array(),
        "object" => v.is_object(),
        "null" => v.is_null(),
        _ => true, // unknown type keyword: don't flag
    }
}

fn type_name(expected: &Value) -> String {
    match expected {
        Value::String(t) => format!("a {t}"),
        Value::Array(types) => {
            let names: Vec<&str> = types.iter().filter_map(Value::as_str).collect();
            format!("one of [{}]", names.join(", "))
        }
        _ => "the declared type".to_string(),
    }
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{Message, ToolCall, ToolSchema};
    use rstest::rstest;
    use serde_json::json;

    fn schema(parameters: Value) -> ToolSchema {
        ToolSchema {
            name: "bash".into(),
            description: "run a command".into(),
            parameters,
        }
    }

    fn bash_schema() -> ToolSchema {
        schema(json!({
            "type": "object",
            "properties": { "command": {"type": "string"}, "timeout": {"type": "integer"} },
            "required": ["command"]
        }))
    }

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: "bash".into(),
            arguments: args,
        }
    }

    async fn run(schema: Option<&ToolSchema>, args: Value) -> VerifyVerdict {
        let c = call(args);
        let hist: Vec<Message> = vec![];
        let ctx = VerifyCtx {
            call: &c,
            goal: "do the thing",
            history: &hist,
            tool_schema: schema,
        };
        SchemaVerifier::new().verify(&ctx).await.verdict
    }

    #[tokio::test]
    async fn positive_valid_call_is_allowed() {
        let s = bash_schema();
        assert_eq!(
            run(Some(&s), json!({"command": "ls"})).await,
            VerifyVerdict::Allow
        );
    }

    #[tokio::test]
    async fn positive_optional_field_may_be_omitted() {
        let s = bash_schema();
        // `timeout` is optional; omitting it is fine.
        assert_eq!(
            run(Some(&s), json!({"command": "ls -la"})).await,
            VerifyVerdict::Allow
        );
    }

    #[tokio::test]
    async fn negative_missing_required_arg_asks_for_revision() {
        // The exact class a live model produced: `bash` with no `command`.
        let s = bash_schema();
        match run(Some(&s), json!({})).await {
            VerifyVerdict::Revise(h) => assert!(h.contains("missing required argument `command`")),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn negative_wrong_type_asks_for_revision() {
        let s = bash_schema();
        match run(Some(&s), json!({"command": 42})).await {
            VerifyVerdict::Revise(h) => assert!(h.contains("`command` should be a string")),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn corner_no_schema_fails_open() {
        // Unknown tool ⇒ no schema ⇒ Allow (fail open).
        assert_eq!(
            run(None, json!({"anything": true})).await,
            VerifyVerdict::Allow
        );
    }

    #[rstest]
    #[case::string_ok(json!({"type":"string"}), json!("x"), true)]
    #[case::integer_rejects_float(json!({"type":"integer"}), json!(1.5), false)]
    #[case::number_accepts_float(json!({"type":"number"}), json!(1.5), true)]
    #[case::union(json!({"type":["string","null"]}), json!(null), true)]
    #[case::unknown_type_not_flagged(json!({"type":"weird"}), json!(1), true)]
    #[tokio::test]
    async fn boundary_type_matching(#[case] spec: Value, #[case] val: Value, #[case] ok: bool) {
        let s = schema(json!({
            "type": "object",
            "properties": { "x": spec },
            "required": ["x"]
        }));
        let got = matches!(run(Some(&s), json!({"x": val})).await, VerifyVerdict::Allow);
        assert_eq!(got, ok);
    }

    // --- adversarial: arguments are attacker-controlled (prompt-injectable model)

    #[tokio::test]
    async fn adversarial_non_object_arguments_are_flagged_not_panicked() {
        let s = bash_schema();
        // The model sends a bare string / array / number instead of an object.
        for bogus in [json!("rm -rf /"), json!([1, 2, 3]), json!(42), json!(null)] {
            match run(Some(&s), bogus).await {
                VerifyVerdict::Revise(h) => assert!(h.contains("must be a JSON object")),
                other => panic!("expected Revise, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn adversarial_huge_field_name_does_not_blow_the_hint() {
        // A hostile giant argument value must not end up echoed into the hint.
        let s = bash_schema();
        let huge = "A".repeat(100_000);
        match run(Some(&s), json!({"command": huge})).await {
            // Correct type ⇒ Allow; the point is it does not panic or balloon.
            VerifyVerdict::Allow => {}
            other => panic!("expected Allow, got {other:?}"),
        }
        // A missing-required hint must stay bounded even with a hostile tool name.
        let v = SchemaVerifier::new();
        let c = ToolCall {
            id: "c".into(),
            name: "Z".repeat(100_000),
            arguments: json!({}),
        };
        let hist: Vec<Message> = vec![];
        let ctx = VerifyCtx {
            call: &c,
            goal: "g",
            history: &hist,
            tool_schema: Some(&s),
        };
        if let VerifyVerdict::Revise(h) = v.verify(&ctx).await.verdict {
            assert!(h.len() <= MAX_HINT, "hint must be capped, was {}", h.len());
        } else {
            panic!("expected Revise");
        }
    }

    #[tokio::test]
    async fn adversarial_deeply_nested_args_are_bounded() {
        // Deep nesting must not cause unbounded recursion — the check is shallow.
        let s = schema(json!({
            "type": "object",
            "properties": { "x": {"type": "object"} },
            "required": ["x"]
        }));
        // 256 levels: deep enough to prove the verifier does not recurse into the
        // value (it only checks the top-level type), shallow enough that
        // serde_json's own recursive Drop does not overflow building the fixture.
        let mut v = json!({});
        for _ in 0..256 {
            v = json!({ "n": v });
        }
        // `x` is an object ⇒ type matches ⇒ Allow, without recursing into it.
        assert_eq!(run(Some(&s), json!({"x": v})).await, VerifyVerdict::Allow);
    }
}
