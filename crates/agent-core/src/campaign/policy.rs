//! The per-campaign policy (`01-schema.md` "`policy` JSON"): stored on the root only,
//! defaults filled in before the snapshot is written, validated at creation and on every
//! human edit. The ranges are the table CHECKs' ranges, so a policy can never loosen a
//! CHECK. Fields are `i64` on purpose: an out-of-range number fails in [`Policy::validate`]
//! naming the field, instead of in serde with a type message.

use super::{CampaignError, CampaignResult, LEASE_MAX_SECS, LEASE_MIN_SECS};
use serde::{Deserialize, Serialize};

/// Hard ceilings shared with the schema (`depth BETWEEN 0 AND 6`, `ordinal BETWEEN 1 AND 8`).
pub const POLICY_MAX_DEPTH: i64 = 6;
pub const POLICY_MAX_CHILDREN: i64 = 8;
pub const POLICY_MAX_NODES: i64 = 200;
pub const POLICY_MAX_PLAN_ATTEMPTS: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Policy {
    /// Depths whose new children start `awaiting_approval` (each in `1..=max_depth`).
    pub approve_levels: Vec<i64>,
    /// A PR needs a human `approve` event before the poller may close it as `done`.
    pub require_pr_approval: bool,
    /// Workers open draft PRs.
    pub draft_prs: bool,
    /// `1..=6`.
    pub max_depth: i64,
    /// `1..=8` (the ordinal grammar caps it).
    pub max_children: i64,
    /// `1..=200` rows per campaign.
    pub max_nodes: i64,
    /// `1..=10` planner attempts per node before it blocks.
    pub max_plan_attempts: i64,
    /// Planner token budget over the whole campaign (`≥ 1`).
    pub max_plan_tokens: i64,
    /// Worker session budget per leaf (`≥ 1`).
    pub max_worker_tokens_per_leaf: i64,
    /// Reserved (v1 never replans automatically).
    pub auto_replan: bool,
    /// Lease length handed to `claim`, `60..=86400`.
    pub lease_secs: i64,
}

/// The JSON type each key must carry; checked before serde so the error names the key.
#[derive(Clone, Copy)]
enum Shape {
    Int,
    Bool,
    IntArray,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Shape::Int => "an integer",
            Shape::Bool => "a boolean",
            Shape::IntArray => "an array of integers",
        }
    }
}

/// Every key of [`Policy`], with its shape. Keep in step with the struct (the
/// `positive_field_table_matches_struct` test checks it).
const FIELDS: &[(&str, Shape)] = &[
    ("approve_levels", Shape::IntArray),
    ("require_pr_approval", Shape::Bool),
    ("draft_prs", Shape::Bool),
    ("max_depth", Shape::Int),
    ("max_children", Shape::Int),
    ("max_nodes", Shape::Int),
    ("max_plan_attempts", Shape::Int),
    ("max_plan_tokens", Shape::Int),
    ("max_worker_tokens_per_leaf", Shape::Int),
    ("auto_replan", Shape::Bool),
    ("lease_secs", Shape::Int),
];

impl Default for Policy {
    fn default() -> Self {
        Self {
            approve_levels: vec![1],
            require_pr_approval: true,
            draft_prs: true,
            max_depth: POLICY_MAX_DEPTH,
            max_children: POLICY_MAX_CHILDREN,
            max_nodes: POLICY_MAX_NODES,
            max_plan_attempts: 3,
            max_plan_tokens: 400_000,
            max_worker_tokens_per_leaf: 2_000_000,
            auto_replan: false,
            lease_secs: 1800,
        }
    }
}

impl Policy {
    /// Check every range; the error names the offending field
    /// (`policy.<field>: <rule>`).
    pub fn validate(&self) -> CampaignResult<()> {
        fn range(field: &str, v: i64, lo: i64, hi: i64) -> CampaignResult<()> {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(CampaignError::Invalid(format!(
                    "policy.{field}: must be in {lo}..={hi}"
                )))
            }
        }
        range("max_depth", self.max_depth, 1, POLICY_MAX_DEPTH)?;
        range("max_children", self.max_children, 1, POLICY_MAX_CHILDREN)?;
        range("max_nodes", self.max_nodes, 1, POLICY_MAX_NODES)?;
        range(
            "max_plan_attempts",
            self.max_plan_attempts,
            1,
            POLICY_MAX_PLAN_ATTEMPTS,
        )?;
        range("max_plan_tokens", self.max_plan_tokens, 1, i64::MAX)?;
        range(
            "max_worker_tokens_per_leaf",
            self.max_worker_tokens_per_leaf,
            1,
            i64::MAX,
        )?;
        range(
            "lease_secs",
            self.lease_secs,
            i64::from(LEASE_MIN_SECS),
            i64::from(LEASE_MAX_SECS),
        )?;
        if self.approve_levels.len() > POLICY_MAX_DEPTH as usize {
            return Err(CampaignError::Invalid(format!(
                "policy.approve_levels: at most {POLICY_MAX_DEPTH} entries"
            )));
        }
        for (i, level) in self.approve_levels.iter().enumerate() {
            range("approve_levels", *level, 1, self.max_depth)?;
            if self.approve_levels[..i].contains(level) {
                return Err(CampaignError::Invalid(
                    "policy.approve_levels: duplicate entry".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Parse a caller-supplied JSON object (unknown keys, wrong types and out-of-range
    /// values all fail closed), then validate. Missing fields take the defaults, so the
    /// result is the full snapshot to store. Every rejection names the offending key
    /// (`policy.<key>: …`), which serde's own type errors do not.
    pub fn from_json(value: &serde_json::Value) -> CampaignResult<Policy> {
        let Some(map) = value.as_object() else {
            return Err(CampaignError::Invalid(
                "policy: must be a JSON object".to_string(),
            ));
        };
        for (key, v) in map {
            let Some((_, shape)) = FIELDS.iter().find(|(name, _)| name == key) else {
                return Err(CampaignError::Invalid(format!("policy.{key}: unknown key")));
            };
            let ok = match shape {
                Shape::Int => v.as_i64().is_some(),
                Shape::Bool => v.is_boolean(),
                Shape::IntArray => v
                    .as_array()
                    .is_some_and(|a| a.iter().all(|x| x.as_i64().is_some())),
            };
            if !ok {
                return Err(CampaignError::Invalid(format!(
                    "policy.{key}: must be {}",
                    shape.name()
                )));
            }
        }
        let policy: Policy = serde_json::from_value(value.clone())
            .map_err(|e| CampaignError::Invalid(format!("policy: {e}")))?;
        policy.validate()?;
        Ok(policy)
    }

    /// Parse the stored text form (a `policy` column); a stored policy that no longer
    /// parses is a backend fault, not a caller error.
    pub fn from_stored(text: &str) -> CampaignResult<Policy> {
        let policy: Policy = serde_json::from_str(text)
            .map_err(|e| CampaignError::Backend(format!("stored policy: {e}")))?;
        policy
            .validate()
            .map_err(|e| CampaignError::Backend(format!("stored policy: {e}")))?;
        Ok(policy)
    }

    /// The snapshot as JSON text (what the `policy` column holds).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// New children at `depth` start `awaiting_approval`.
    pub fn gated(&self, depth: u8) -> bool {
        self.approve_levels.contains(&i64::from(depth))
    }

    /// `min(8, max_children)` as the grammar's `u8`.
    pub fn children_cap(&self) -> u8 {
        u8::try_from(self.max_children.clamp(1, POLICY_MAX_CHILDREN)).unwrap_or(1)
    }

    /// `max_depth` as the grammar's `u8`.
    pub fn depth_cap(&self) -> u8 {
        u8::try_from(self.max_depth.clamp(1, POLICY_MAX_DEPTH)).unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;

    fn err_names(result: CampaignResult<Policy>, field: &str) {
        match result {
            Err(CampaignError::Invalid(msg)) => {
                assert!(msg.contains(field), "expected `{field}` in `{msg}`");
            }
            other => panic!("expected Invalid naming {field}, got {other:?}"),
        }
    }

    // -- T4 policy rows, at the pure level ---------------------------------------

    #[test]
    fn corner_policy_omitted() {
        let policy = Policy::default();
        policy.validate().unwrap();
        // The design's literal (`01-schema.md` "`policy` JSON").
        assert_eq!(
            serde_json::to_value(&policy).unwrap(),
            json!({
                "approve_levels": [1],
                "require_pr_approval": true,
                "draft_prs": true,
                "max_depth": 6,
                "max_children": 8,
                "max_nodes": 200,
                "max_plan_attempts": 3,
                "max_plan_tokens": 400_000,
                "max_worker_tokens_per_leaf": 2_000_000,
                "auto_replan": false,
                "lease_secs": 1800
            })
        );
        assert_eq!(Policy::from_json(&json!({})).unwrap(), policy);
        assert_eq!(Policy::from_stored(&policy.to_json()).unwrap(), policy);
    }

    #[test]
    fn corner_policy_partial() {
        let policy = Policy::from_json(&json!({"draft_prs": false})).unwrap();
        assert!(!policy.draft_prs);
        assert_eq!(
            Policy {
                draft_prs: true,
                ..policy
            },
            Policy::default()
        );
    }

    #[test]
    fn negative_policy_unknown_key() {
        err_names(
            Policy::from_json(&json!({"max_depth": 6, "bogus": 1})),
            "bogus",
        );
    }

    #[rstest]
    #[case::negative_policy_out_of_range_max_depth(json!({"max_depth": 7}), "max_depth")]
    #[case::negative_policy_out_of_range_max_children(json!({"max_children": 9}), "max_children")]
    #[case::negative_policy_out_of_range_max_nodes(json!({"max_nodes": 0}), "max_nodes")]
    #[case::negative_policy_out_of_range_lease_secs(json!({"lease_secs": 59}), "lease_secs")]
    #[case::negative_policy_out_of_range_attempts(json!({"max_plan_attempts": 11}), "max_plan_attempts")]
    #[case::negative_policy_bad_level_zero(json!({"approve_levels": [0]}), "approve_levels")]
    #[case::negative_policy_bad_level_seven(json!({"approve_levels": [7]}), "approve_levels")]
    #[case::negative_policy_level_above_max_depth(json!({"max_depth": 2, "approve_levels": [3]}), "approve_levels")]
    #[case::negative_policy_duplicate_level(json!({"approve_levels": [1, 1]}), "approve_levels")]
    #[case::negative_policy_too_many_levels(json!({"approve_levels": [1, 2, 3, 4, 5, 6, 1]}), "approve_levels")]
    #[case::negative_policy_zero_plan_tokens(json!({"max_plan_tokens": 0}), "max_plan_tokens")]
    #[case::negative_policy_zero_worker_tokens(json!({"max_worker_tokens_per_leaf": 0}), "max_worker_tokens_per_leaf")]
    #[case::boundary_max_depth_zero(json!({"max_depth": 0}), "max_depth")]
    #[case::boundary_max_children_zero(json!({"max_children": 0}), "max_children")]
    #[case::boundary_max_nodes_201(json!({"max_nodes": 201}), "max_nodes")]
    #[case::boundary_lease_86401(json!({"lease_secs": 86401}), "lease_secs")]
    #[case::adversarial_policy_edit_loosens_check(json!({"max_children": 9}), "max_children")]
    #[case::adversarial_negative_depth(json!({"max_depth": -1}), "max_depth")]
    #[case::adversarial_negative_tokens(json!({"max_plan_tokens": -5}), "max_plan_tokens")]
    #[case::adversarial_float_for_int(json!({"max_depth": 3.5}), "max_depth")]
    #[case::adversarial_string_for_int(json!({"max_depth": "6"}), "max_depth")]
    #[case::adversarial_huge_number(json!({"max_nodes": 1e30}), "max_nodes")]
    #[case::adversarial_u64_overflow(json!({"max_plan_tokens": u64::MAX}), "max_plan_tokens")]
    #[case::adversarial_levels_not_array(json!({"approve_levels": 1}), "approve_levels")]
    #[case::adversarial_levels_strings(json!({"approve_levels": ["1"]}), "approve_levels")]
    #[case::adversarial_bool_for_int(json!({"lease_secs": true}), "lease_secs")]
    #[case::adversarial_int_for_bool(json!({"draft_prs": 1}), "draft_prs")]
    #[case::adversarial_null_field(json!({"max_depth": null}), "max_depth")]
    fn rejected_naming_field(#[case] value: serde_json::Value, #[case] field: &str) {
        err_names(Policy::from_json(&value), field);
    }

    #[rstest]
    #[case::adversarial_policy_array(json!([1, 2]))]
    #[case::adversarial_policy_string(json!("max_depth=6"))]
    #[case::adversarial_policy_null(json!(null))]
    #[case::adversarial_policy_number(json!(6))]
    fn adversarial_policy_not_object(#[case] value: serde_json::Value) {
        err_names(Policy::from_json(&value), "policy");
    }

    #[rstest]
    #[case::boundary_max_depth_1(json!({"max_depth": 1}))]
    #[case::boundary_max_depth_6(json!({"max_depth": 6}))]
    #[case::boundary_max_children_1(json!({"max_children": 1}))]
    #[case::boundary_max_children_8(json!({"max_children": 8}))]
    #[case::boundary_max_nodes_1(json!({"max_nodes": 1}))]
    #[case::boundary_max_nodes_200(json!({"max_nodes": 200}))]
    #[case::boundary_attempts_1(json!({"max_plan_attempts": 1}))]
    #[case::boundary_attempts_10(json!({"max_plan_attempts": 10}))]
    #[case::boundary_lease_floor(json!({"lease_secs": 60}))]
    #[case::boundary_lease_ceiling(json!({"lease_secs": 86400}))]
    #[case::boundary_levels_all_six(json!({"approve_levels": [1, 2, 3, 4, 5, 6]}))]
    #[case::boundary_levels_empty(json!({"approve_levels": []}))]
    #[case::boundary_plan_tokens_1(json!({"max_plan_tokens": 1}))]
    #[case::boundary_plan_tokens_max(json!({"max_plan_tokens": i64::MAX}))]
    #[case::corner_level_at_max_depth(json!({"max_depth": 2, "approve_levels": [2]}))]
    fn accepted(#[case] value: serde_json::Value) {
        let policy = Policy::from_json(&value).unwrap();
        policy.validate().unwrap();
        assert_eq!(Policy::from_stored(&policy.to_json()).unwrap(), policy);
    }

    #[test]
    fn positive_helpers() {
        let policy = Policy::from_json(
            &json!({"approve_levels": [1, 3], "max_children": 3, "max_depth": 4}),
        )
        .unwrap();
        assert!(policy.gated(1));
        assert!(!policy.gated(2));
        assert!(policy.gated(3));
        assert!(!policy.gated(0));
        assert_eq!(policy.children_cap(), 3);
        assert_eq!(policy.depth_cap(), 4);
        assert_eq!(Policy::default().children_cap(), 8);
        assert_eq!(Policy::default().depth_cap(), 6);
    }

    #[test]
    fn positive_field_table_matches_struct() {
        // serde_json's map is sorted; compare as sorted sets.
        let value = serde_json::to_value(Policy::default()).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut table: Vec<&str> = FIELDS.iter().map(|(n, _)| *n).collect();
        table.sort_unstable();
        assert_eq!(keys, table);
    }

    #[test]
    fn negative_stored_policy_is_backend_fault() {
        assert!(matches!(
            Policy::from_stored("not json"),
            Err(CampaignError::Backend(_))
        ));
        assert!(matches!(
            Policy::from_stored(r#"{"max_depth": 9}"#),
            Err(CampaignError::Backend(_))
        ));
        assert!(matches!(
            Policy::from_stored(r#"{"bogus": 1}"#),
            Err(CampaignError::Backend(_))
        ));
    }

    #[test]
    fn positive_error_display_and_conversion() {
        let e = CampaignError::Invalid("policy.max_depth: must be in 1..=6".to_string());
        assert_eq!(e.to_string(), "invalid: policy.max_depth: must be in 1..=6");
        let shared: crate::Error = e.into();
        assert!(matches!(shared, crate::Error::Campaign(ref s) if s.contains("max_depth")));
        let from_path: CampaignError = super::super::PathError::Ordinal.into();
        assert!(matches!(from_path, CampaignError::Invalid(ref s) if s.starts_with("path:")));
        for e in [
            CampaignError::NotFound,
            CampaignError::AlreadyApplied,
            CampaignError::LeaseLost,
            CampaignError::Conflict("v".into()),
            CampaignError::Denied("d".into()),
            CampaignError::TooLong("t".into()),
            CampaignError::Backend("b".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
