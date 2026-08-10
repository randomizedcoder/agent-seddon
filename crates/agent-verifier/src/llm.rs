//! An LLM-backed verifier: ask a model whether a requested tool call is a correct,
//! relevant step toward the goal, and turn its structured answer into a
//! [`VerifierReport`]. This is the model-backed counterpart to [`crate::SchemaVerifier`]
//! — it can catch *semantic* slips (wrong tool, off-goal arguments) a schema check
//! can't, at the cost of a model call.
//!
//! **Fails open.** Any provider error, a missing/unparseable response, or an
//! unrecognised verdict resolves to [`VerifyVerdict::Allow`] — a degraded verifier
//! must never wedge the loop (docs/design/tool-call-verification.md).
//!
//! **Everything sent to the model is attacker-influenced** (the goal, the call's
//! arguments, and the history are all prompt-injectable), so each is bounded before it
//! goes into the prompt, and the model's self-reported `confidence` is untrusted —
//! callers clamp it with [`VerifierReport::clamped_confidence`] before any weighting.

use agent_core::{
    CompletionRequest, LlmProvider, Message, Verifier, VerifierReport, VerifyCtx, VerifyVerdict,
};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Bounds on the (untrusted) content put into the verifier prompt.
const MAX_GOAL_CHARS: usize = 1_000;
const MAX_ARGS_CHARS: usize = 2_000;
const MAX_SCHEMA_CHARS: usize = 1_000;
const MAX_MSG_CHARS: usize = 400;
const MAX_HISTORY_MSGS: usize = 6;
/// Longest hint fed back to the model (never echoes raw argument values).
const MAX_HINT: usize = 512;
/// The verifier's own answer is tiny structured JSON — cap its generation.
const VERIFY_MAX_TOKENS: u32 = 256;

const SYSTEM_PROMPT: &str = "\
You are a tool-call verifier for an autonomous agent. Given the agent's goal and a \
single requested tool call, judge whether that call is a correct and relevant step \
toward the goal. Respond with ONLY a JSON object and nothing else:\n\
{\"verdict\": \"allow\" | \"revise\" | \"deny\", \"reason\": \"<one short sentence>\", \"confidence\": <0.0-1.0>}\n\
Use \"revise\" (with a corrective reason) for a call that is likely wrong but fixable \
(wrong arguments, wrong tool for this step); \"deny\" only for a clearly harmful or \
irrelevant call; \"allow\" otherwise. Safety is handled elsewhere — judge correctness. \
Be conservative: when unsure, \"allow\".";

pub struct LlmVerifier {
    provider: Arc<dyn LlmProvider>,
}

impl LlmVerifier {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    /// The bounded user prompt describing the call to judge.
    fn user_prompt(ctx: &VerifyCtx<'_>) -> String {
        let mut p = String::new();
        p.push_str("Goal: ");
        p.push_str(&cap(ctx.goal, MAX_GOAL_CHARS));
        p.push_str("\n\nTool call to verify:\n  name: ");
        p.push_str(&ctx.call.name);
        p.push_str("\n  arguments: ");
        p.push_str(&cap(&ctx.call.arguments.to_string(), MAX_ARGS_CHARS));
        if let Some(schema) = ctx.tool_schema {
            p.push_str("\n  declared parameters: ");
            p.push_str(&cap(&schema.parameters.to_string(), MAX_SCHEMA_CHARS));
        }
        // The recent conversation tail, oldest→newest, text-only.
        let tail = ctx
            .history
            .iter()
            .rev()
            .filter_map(|m| {
                let t = m.content_text();
                (!t.is_empty()).then(|| (m.role.as_str(), t))
            })
            .take(MAX_HISTORY_MSGS)
            .collect::<Vec<_>>();
        if !tail.is_empty() {
            p.push_str("\n\nRecent context (most recent last):");
            for (role, text) in tail.into_iter().rev() {
                p.push_str(&format!("\n  {role}: {}", cap(&text, MAX_MSG_CHARS)));
            }
        }
        p
    }
}

#[async_trait]
impl Verifier for LlmVerifier {
    fn name(&self) -> &str {
        "llm"
    }

    async fn verify(&self, ctx: &VerifyCtx<'_>) -> VerifierReport {
        let req = CompletionRequest {
            messages: vec![
                Message::system(SYSTEM_PROMPT),
                Message::user(Self::user_prompt(ctx)),
            ],
            tools: Vec::new(),
            max_tokens: VERIFY_MAX_TOKENS,
            temperature: 0.0,
            response_format: None,
            route: None,
        };
        // Fail open on any provider error — a broken verifier must not block the loop.
        let Ok(resp) = self.provider.complete(req).await else {
            return VerifierReport::allow("llm");
        };
        let text = resp.message.content_text();
        parse_report(&text).unwrap_or_else(|| VerifierReport::allow("llm"))
    }
}

/// Parse the model's answer into a report, or `None` to fail open. The model is told
/// to return bare JSON but often wraps it in prose/markdown, so we extract the first
/// `{...}` object; an unrecognised verdict yields `None`.
fn parse_report(text: &str) -> Option<VerifierReport> {
    let json = extract_json_object(text)?;
    let v: Value = serde_json::from_str(json).ok()?;
    let verdict_str = v.get("verdict")?.as_str()?.trim().to_lowercase();
    let reason = v.get("reason").and_then(Value::as_str).unwrap_or("").trim();
    // Untrusted; clamped by the caller before any weighting. Missing ⇒ a neutral 0.5.
    let confidence = v
        .get("confidence")
        .and_then(Value::as_f64)
        .map_or(0.5, |f| f as f32);

    let verdict = match verdict_str.as_str() {
        "allow" => VerifyVerdict::Allow,
        "revise" => VerifyVerdict::Revise(hint("revise", reason)),
        "deny" => VerifyVerdict::Deny(hint("deny", reason)),
        _ => return None, // unknown verdict ⇒ fail open
    };
    Some(VerifierReport {
        verdict,
        confidence,
        model: "llm".to_string(),
    })
}

/// Build a bounded hint for a non-allow verdict, with a sensible default when the
/// model gave no reason.
fn hint(kind: &str, reason: &str) -> String {
    let mut h = if reason.is_empty() {
        format!("the verifier judged this call should be {kind}d; reconsider the tool call")
    } else {
        reason.to_string()
    };
    h.truncate(floor_boundary(&h, MAX_HINT));
    h
}

/// The first balanced-looking `{...}` slice of `text` (from the first `{` to the last
/// `}`), or `None`. Lenient: `serde_json` still validates the slice.
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

/// Truncate `s` to at most `max` bytes on a char boundary, appending `…` if cut.
fn cap(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let cut = floor_boundary(s, max);
    format!("{}…", &s[..cut])
}

/// Largest char boundary `<= max`.
fn floor_boundary(s: &str, max: usize) -> usize {
    let mut cut = max.min(s.len());
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{ToolCall, ToolSchema};
    use agent_testkit::{final_turn, ScriptedProvider};
    use serde_json::json;

    fn ctx_call() -> (ToolCall, ToolSchema) {
        let call = ToolCall {
            id: "1".into(),
            name: "bash".into(),
            arguments: json!({ "command": "ls" }),
        };
        let schema = ToolSchema {
            name: "bash".into(),
            description: "run a shell command".into(),
            parameters: json!({ "type": "object", "required": ["command"] }),
        };
        (call, schema)
    }

    /// Drive `verify` with a provider scripted to answer `answer`, and return the verdict.
    async fn verdict_for(answer: &str) -> VerifyVerdict {
        let (call, schema) = ctx_call();
        let provider = Arc::new(ScriptedProvider::new(vec![final_turn(answer)]));
        let v = LlmVerifier::new(provider);
        let ctx = VerifyCtx {
            call: &call,
            goal: "list files",
            history: &[],
            tool_schema: Some(&schema),
        };
        v.verify(&ctx).await.verdict
    }

    // --- parse_report: the verdict mapping in isolation ---------------------
    #[rstest::rstest]
    #[case::positive_allow(r#"{"verdict":"allow","confidence":0.9}"#, Some(VerifyVerdict::Allow))]
    #[case::positive_revise(
        r#"{"verdict":"revise","reason":"wrong flag","confidence":0.7}"#,
        Some(VerifyVerdict::Revise(String::new()))
    )]
    #[case::positive_deny(
        r#"{"verdict":"deny","reason":"off goal"}"#,
        Some(VerifyVerdict::Deny(String::new()))
    )]
    // corner: the model wraps the JSON in prose / a markdown fence — still parsed.
    #[case::corner_wrapped(
        "Sure!\n```json\n{\"verdict\": \"allow\"}\n```",
        Some(VerifyVerdict::Allow)
    )]
    #[case::corner_case_insensitive(r#"{"verdict":"ALLOW"}"#, Some(VerifyVerdict::Allow))]
    // negative / adversarial: no JSON, malformed, or an unknown verdict ⇒ fail open.
    #[case::negative_no_json("I think this looks fine", None)]
    #[case::adversarial_malformed(r#"{"verdict": "allow""#, None)]
    #[case::adversarial_unknown_verdict(r#"{"verdict":"maybe"}"#, None)]
    fn parse_report_cases(#[case] text: &str, #[case] want: Option<VerifyVerdict>) {
        let got = parse_report(text).map(|r| match r.verdict {
            // Normalize the hint string away — these cases pin the *variant*, not the hint.
            VerifyVerdict::Revise(_) => VerifyVerdict::Revise(String::new()),
            VerifyVerdict::Deny(_) => VerifyVerdict::Deny(String::new()),
            v => v,
        });
        assert_eq!(got, want);
    }

    /// `positive_`: a scripted "revise" answer flows end-to-end into a `Revise` verdict
    /// carrying the model's reason as the hint.
    #[tokio::test]
    async fn positive_verify_maps_revise_with_reason() {
        let v =
            verdict_for(r#"{"verdict":"revise","reason":"use `find` instead","confidence":0.8}"#)
                .await;
        match v {
            VerifyVerdict::Revise(h) => assert!(h.contains("find"), "hint carries the reason: {h}"),
            other => panic!("expected Revise, got {other:?}"),
        }
    }

    /// `adversarial_`: a garbage / non-JSON model answer must fail **open** (Allow), never
    /// wedge or crash the loop.
    #[tokio::test]
    async fn adversarial_garbage_answer_fails_open() {
        assert_eq!(verdict_for("lol no idea 🤷").await, VerifyVerdict::Allow);
    }

    /// `adversarial_`: a hostile confidence (NaN / out-of-range) is clamped to `0.0..=1.0`
    /// by the report's own accessor before any weighting.
    #[test]
    fn adversarial_confidence_is_clamped() {
        let r = parse_report(r#"{"verdict":"allow","confidence":42.0}"#).unwrap();
        assert_eq!(r.clamped_confidence(), 1.0);
        let r = parse_report(r#"{"verdict":"allow","confidence":-5.0}"#).unwrap();
        assert_eq!(r.clamped_confidence(), 0.0);
    }
}
