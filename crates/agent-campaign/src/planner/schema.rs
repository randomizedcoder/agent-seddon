//! The one decision schema the planner asks with (`03-decomposition.md` step 3) and
//! the per-depth narrowing of its `decision` enum.
//!
//! The schema mirrors the seam's caps (`MAX_TITLE`, `MAX_ACCEPTANCE`, …) so a
//! model answer that would be `TooLong` at the store is a schema failure first —
//! repaired once or twice, then closed as an attempt `error`. Schema validity is
//! necessary, not sufficient: `validate.rs` applies the rules a schema cannot
//! (resolution of `touches`, `est_size ∈ {xs, s}`, the dependency graph, …).

use agent_core::campaign::{
    MAX_ACCEPTANCE, MAX_ACCEPTANCE_ITEM, MAX_CHILDREN, MAX_QUESTION, MAX_TITLE, MAX_TOUCH,
    MAX_TOUCHES,
};
use serde_json::{json, Value};

/// A child `goal` the model writes is capped below the store's `MAX_GOAL` (4000):
/// the remainder is headroom for the `## Clarification` blocks `answer` appends.
pub const MAX_CHILD_GOAL: usize = 2000;
/// The model's `reason` (a sentence, not a report).
pub const MAX_REASON_ANSWER: usize = 600;
/// A response body over this many bytes is refused before it is parsed
/// (`adversarial_huge_response`): a well-formed answer is a few KiB.
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;

/// The four answers a node can get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Execute,
    Split,
    NeedsInfo,
    Reject,
}

impl Decision {
    pub const ALL: [Decision; 4] = [
        Decision::Execute,
        Decision::Split,
        Decision::NeedsInfo,
        Decision::Reject,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Execute => "execute",
            Decision::Split => "split",
            Decision::NeedsInfo => "needs_info",
            Decision::Reject => "reject",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.as_str() == s)
    }
}

/// Which decisions `depth` may answer under `depth_cap` (`policy.max_depth`):
///
/// * the root is never executed (D7): `split | needs_info | reject`;
/// * a node at `depth_cap − 1` (or deeper, after a policy edit lowered the cap)
///   cannot split: `execute | needs_info | reject` — so a `split` there is a schema
///   failure, not a policy exception. The store's own ceiling (children at
///   `≤ max_depth`) stays one level of slack below this;
/// * a cap of 1 keeps the root rule (the root still cannot be a leaf).
pub fn allowed_decisions(depth: u8, depth_cap: u8) -> &'static [Decision] {
    const ROOT: &[Decision] = &[Decision::Split, Decision::NeedsInfo, Decision::Reject];
    const DEEP: &[Decision] = &[Decision::Execute, Decision::NeedsInfo, Decision::Reject];
    const ANY: &[Decision] = &Decision::ALL;
    if depth == 0 {
        ROOT
    } else if depth.saturating_add(1) >= depth_cap {
        DEEP
    } else {
        ANY
    }
}

/// The decision schema with `decision.enum` narrowed to `allowed`.
pub fn decision_schema(allowed: &[Decision]) -> Value {
    let decisions: Vec<&str> = allowed.iter().map(|d| d.as_str()).collect();
    let est_size = json!({ "enum": ["xs", "s", "m", "l"] });
    let acceptance = json!({
        "type": "array", "maxItems": MAX_ACCEPTANCE,
        "items": { "type": "string", "maxLength": MAX_ACCEPTANCE_ITEM }
    });
    let touches = json!({
        "type": "array", "maxItems": MAX_TOUCHES,
        "items": { "type": "string", "maxLength": MAX_TOUCH }
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["decision", "reason", "confidence"],
        "properties": {
            "decision":   { "enum": decisions },
            "reason":     { "type": "string", "maxLength": MAX_REASON_ANSWER },
            "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
            "question":   { "type": "string", "maxLength": MAX_QUESTION },
            "acceptance": acceptance,
            "touches":    touches,
            "est_size":   est_size,
            "children": {
                "type": "array", "maxItems": MAX_CHILDREN,
                "items": {
                    "type": "object", "additionalProperties": false,
                    "required": ["title", "goal", "est_size"],
                    "properties": {
                        "title":      { "type": "string", "minLength": 1, "maxLength": MAX_TITLE },
                        "goal":       { "type": "string", "minLength": 1, "maxLength": MAX_CHILD_GOAL },
                        "acceptance": acceptance,
                        "touches":    touches,
                        "est_size":   est_size,
                        "depends_on": {
                            "type": "array", "maxItems": MAX_CHILDREN - 1,
                            "items": { "type": "integer", "minimum": 1, "maximum": MAX_CHILDREN }
                        }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::OutputSchema;
    use agent_validate::Draft07Validator;
    use rstest::rstest;

    fn names(ds: &[Decision]) -> Vec<&'static str> {
        ds.iter().map(|d| d.as_str()).collect()
    }

    #[rstest]
    #[case::positive_root(0, 6, &["split", "needs_info", "reject"])]
    #[case::positive_middle(2, 6, &["execute", "split", "needs_info", "reject"])]
    #[case::boundary_cap_minus_two(4, 6, &["execute", "split", "needs_info", "reject"])]
    #[case::boundary_cap_minus_one(5, 6, &["execute", "needs_info", "reject"])]
    #[case::corner_at_cap(6, 6, &["execute", "needs_info", "reject"])]
    #[case::corner_beyond_cap_after_policy_edit(6, 3, &["execute", "needs_info", "reject"])]
    #[case::corner_cap_one_root(0, 1, &["split", "needs_info", "reject"])]
    #[case::corner_cap_one_child(1, 1, &["execute", "needs_info", "reject"])]
    #[case::corner_cap_two_child(1, 2, &["execute", "needs_info", "reject"])]
    #[case::adversarial_depth_255(255, 6, &["execute", "needs_info", "reject"])]
    #[case::adversarial_cap_zero(1, 0, &["execute", "needs_info", "reject"])]
    fn allowed_rows(#[case] depth: u8, #[case] cap: u8, #[case] want: &[&str]) {
        assert_eq!(names(allowed_decisions(depth, cap)), want);
    }

    #[test]
    fn positive_decision_round_trip() {
        for d in Decision::ALL {
            assert_eq!(Decision::parse(d.as_str()), Some(d));
        }
        assert_eq!(Decision::parse("maybe"), None);
        assert_eq!(Decision::parse("Execute"), None);
    }

    fn validate(schema: &Value, v: &Value) -> Result<(), String> {
        let verdict = Draft07Validator::new().validate(schema, v);
        if verdict.ok {
            Ok(())
        } else {
            Err(verdict.errors.join("; "))
        }
    }

    #[rstest]
    #[case::positive_execute(
        json!({"decision": "execute", "reason": "small", "confidence": 0.9,
               "acceptance": ["a"], "touches": ["src/lib.rs"], "est_size": "s"}), true)]
    #[case::positive_split(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "t", "goal": "g", "est_size": "m", "depends_on": []}]}), true)]
    #[case::negative_unknown_decision(
        json!({"decision": "maybe", "reason": "r", "confidence": 0.5}), false)]
    #[case::negative_missing_reason(json!({"decision": "reject", "confidence": 0.5}), false)]
    #[case::negative_child_missing_goal(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "t", "est_size": "m"}]}), false)]
    #[case::corner_confidence_out_of_range(
        json!({"decision": "reject", "reason": "r", "confidence": 1.5}), false)]
    #[case::boundary_confidence_one(json!({"decision": "reject", "reason": "r", "confidence": 1}), true)]
    #[case::boundary_confidence_zero(json!({"decision": "reject", "reason": "r", "confidence": 0}), true)]
    #[case::boundary_children_8(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": (0..8).map(|i| json!({"title": format!("t{i}"), "goal": "g", "est_size": "s"})).collect::<Vec<_>>()}), true)]
    #[case::boundary_children_9(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": (0..9).map(|i| json!({"title": format!("t{i}"), "goal": "g", "est_size": "s"})).collect::<Vec<_>>()}), false)]
    #[case::boundary_title_120(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "x".repeat(120), "goal": "g", "est_size": "s"}]}), true)]
    #[case::boundary_title_121(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "x".repeat(121), "goal": "g", "est_size": "s"}]}), false)]
    #[case::boundary_goal_2000(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "t", "goal": "g".repeat(2000), "est_size": "s"}]}), true)]
    #[case::boundary_goal_2001(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "t", "goal": "g".repeat(2001), "est_size": "s"}]}), false)]
    #[case::boundary_acceptance_6(
        json!({"decision": "execute", "reason": "r", "confidence": 0.5,
               "acceptance": vec!["a"; 6], "touches": ["p"], "est_size": "s"}), true)]
    #[case::boundary_acceptance_7(
        json!({"decision": "execute", "reason": "r", "confidence": 0.5,
               "acceptance": vec!["a"; 7], "touches": ["p"], "est_size": "s"}), false)]
    #[case::boundary_touches_12(
        json!({"decision": "execute", "reason": "r", "confidence": 0.5,
               "acceptance": ["a"], "touches": vec!["p"; 12], "est_size": "s"}), true)]
    #[case::boundary_touches_13(
        json!({"decision": "execute", "reason": "r", "confidence": 0.5,
               "acceptance": ["a"], "touches": vec!["p"; 13], "est_size": "s"}), false)]
    #[case::adversarial_schema_escape(
        json!({"decision": "reject", "reason": "r", "confidence": 0.5, "tool_calls": []}), false)]
    #[case::adversarial_child_extra_key(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "t", "goal": "g", "est_size": "s", "policy": {}}]}), false)]
    #[case::adversarial_depends_on_task_id(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "t", "goal": "g", "est_size": "s", "depends_on": [1042]}]}), false)]
    #[case::adversarial_depends_on_zero(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "t", "goal": "g", "est_size": "s", "depends_on": [0]}]}), false)]
    #[case::adversarial_oversize_title(
        json!({"decision": "split", "reason": "r", "confidence": 0.5,
               "children": [{"title": "x".repeat(10 * 1024), "goal": "g", "est_size": "s"}]}), false)]
    #[case::adversarial_est_size_unknown(
        json!({"decision": "execute", "reason": "r", "confidence": 0.5,
               "acceptance": ["a"], "touches": ["p"], "est_size": "xl"}), false)]
    #[case::adversarial_not_an_object(json!(["execute"]), false)]
    fn schema_rows(#[case] value: Value, #[case] ok: bool) {
        let schema = decision_schema(&Decision::ALL);
        let got = validate(&schema, &value);
        assert_eq!(got.is_ok(), ok, "{got:?}");
    }

    #[rstest]
    #[case::positive_root_split(0, "split", true)]
    #[case::negative_root_execute(0, "execute", false)]
    #[case::negative_deep_split(5, "split", false)]
    #[case::positive_deep_execute(5, "execute", true)]
    #[case::positive_middle_split(3, "split", true)]
    fn narrowed_rows(#[case] depth: u8, #[case] decision: &str, #[case] ok: bool) {
        let schema = decision_schema(allowed_decisions(depth, 6));
        let v = json!({"decision": decision, "reason": "r", "confidence": 0.5,
                       "acceptance": ["a"], "touches": ["p"], "est_size": "s",
                       "children": [{"title": "t", "goal": "g", "est_size": "s"}]});
        assert_eq!(validate(&schema, &v).is_ok(), ok);
    }
}
