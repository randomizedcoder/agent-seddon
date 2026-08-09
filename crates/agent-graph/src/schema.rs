//! The node-type schema registry: what node types exist, what params each
//! accepts, and the [`NodeTypeSchema`] the editor renders from
//! (`DescribeNodeTypes`). One table of [`ParamSpec`]s per type is the single
//! source of truth — the params *validator* and the params *JSON Schema* are
//! both derived from it, so they cannot drift.
//!
//! Params are validated fail-closed: unknown keys reject, values must match
//! their spec's type and bounds, and anything naming a configured resource
//! (a provider, a store) must be a [`safe_segment`] — a path-like name such as
//! `../../etc` is refused here, long before it could reach a registry lookup.

use std::collections::BTreeMap;

use agent_core::{safe_segment, NodePort, NodeTypeSchema};
use serde_json::{json, Value};

/// What one param accepts. The closed vocabulary keeps validation and the
/// generated JSON Schema in lockstep.
#[derive(Debug, Clone, Copy)]
enum ParamKind {
    /// A configured resource name (provider/store) — `safe_segment` enforced.
    Name,
    /// One of a closed set of string literals.
    Choice(&'static [&'static str]),
    /// An unsigned integer within `[min, max]`.
    UInt { min: u64, max: u64 },
    /// A number within `[0.0, 1.0]`.
    Fraction,
    /// Free prose (a branch lens, a rubric line) — length-capped, and always
    /// treated as quoted DATA at use, never instructions to trust.
    Text { max_len: usize },
    /// A boolean flag.
    Bool,
}

/// One accepted param key. Every param is optional — node factories fall back
/// to the same defaults the TOML config uses.
#[derive(Debug, Clone, Copy)]
struct ParamSpec {
    key: &'static str,
    kind: ParamKind,
    doc: &'static str,
}

/// A registered node type: the editor-facing schema plus the param table the
/// validator runs against.
pub struct NodeType {
    pub schema: NodeTypeSchema,
    params: &'static [ParamSpec],
}

impl NodeType {
    /// Validate a node's `params` against this type's table. `Null` means "no
    /// params" (every key is optional); anything else must be an object with
    /// only known, well-typed keys.
    pub fn validate_params(&self, params: &Value) -> Result<(), String> {
        let obj = match params {
            Value::Null => return Ok(()),
            Value::Object(o) => o,
            other => {
                return Err(format!(
                    "params must be an object, got {}",
                    type_name(other)
                ))
            }
        };
        for (key, value) in obj {
            let spec = self
                .params
                .iter()
                .find(|s| s.key == key)
                .ok_or_else(|| format!("unknown param `{}`", key.escape_debug()))?;
            check_value(spec, value)?;
        }
        Ok(())
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check_value(spec: &ParamSpec, v: &Value) -> Result<(), String> {
    match spec.kind {
        ParamKind::Name => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("`{}` must be a string", spec.key))?;
            if !safe_segment(s) {
                return Err(format!(
                    "`{}` is not a valid resource name: `{}`",
                    spec.key,
                    s.escape_debug()
                ));
            }
            Ok(())
        }
        ParamKind::Choice(options) => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("`{}` must be a string", spec.key))?;
            if !options.contains(&s) {
                return Err(format!(
                    "`{}` must be one of {:?}, got `{}`",
                    spec.key,
                    options,
                    s.escape_debug()
                ));
            }
            Ok(())
        }
        ParamKind::UInt { min, max } => {
            let n = v
                .as_u64()
                .ok_or_else(|| format!("`{}` must be a non-negative integer", spec.key))?;
            if n < min || n > max {
                return Err(format!("`{}` must be in {min}..={max}, got {n}", spec.key));
            }
            Ok(())
        }
        ParamKind::Fraction => {
            let f = v
                .as_f64()
                .ok_or_else(|| format!("`{}` must be a number", spec.key))?;
            // NaN fails both comparisons' complement — write the accept-range
            // positively so a hostile NaN is rejected, not waved through.
            if !(0.0..=1.0).contains(&f) {
                return Err(format!("`{}` must be within 0..=1, got {f}", spec.key));
            }
            Ok(())
        }
        ParamKind::Text { max_len } => {
            let s = v
                .as_str()
                .ok_or_else(|| format!("`{}` must be a string", spec.key))?;
            if s.len() > max_len {
                return Err(format!(
                    "`{}` is {} bytes (cap {max_len})",
                    spec.key,
                    s.len()
                ));
            }
            Ok(())
        }
        ParamKind::Bool => {
            if !v.is_boolean() {
                return Err(format!("`{}` must be a boolean", spec.key));
            }
            Ok(())
        }
    }
}

/// Derive the JSON Schema (draft-07 shape, with the UI reading `description`)
/// from a param table — the editor's form definition.
fn params_schema(specs: &[ParamSpec]) -> Value {
    let mut props = serde_json::Map::new();
    for s in specs {
        let mut p = serde_json::Map::new();
        p.insert("description".into(), json!(s.doc));
        match s.kind {
            ParamKind::Name => {
                p.insert("type".into(), json!("string"));
            }
            ParamKind::Choice(options) => {
                p.insert("type".into(), json!("string"));
                p.insert("enum".into(), json!(options));
            }
            ParamKind::UInt { min, max } => {
                p.insert("type".into(), json!("integer"));
                p.insert("minimum".into(), json!(min));
                p.insert("maximum".into(), json!(max));
            }
            ParamKind::Fraction => {
                p.insert("type".into(), json!("number"));
                p.insert("minimum".into(), json!(0.0));
                p.insert("maximum".into(), json!(1.0));
            }
            ParamKind::Text { max_len } => {
                p.insert("type".into(), json!("string"));
                p.insert("maxLength".into(), json!(max_len));
            }
            ParamKind::Bool => {
                p.insert("type".into(), json!("boolean"));
            }
        }
        props.insert(s.key.to_string(), Value::Object(p));
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": Value::Object(props),
    })
}

fn port(name: &str, kind: &str) -> NodePort {
    NodePort {
        name: name.into(),
        kind: kind.into(),
    }
}

/// The registry of node types this build understands: the six types that
/// re-express increments 01–03 plus increment 05's fork/join set
/// (`split`/`join`/`merge`).
/// New types follow the seam-impl recipe: implement the node, add its table
/// here, and `every_node_type_has_a_schema` keeps the palette honest.
pub struct NodeTypeRegistry {
    types: BTreeMap<&'static str, NodeType>,
}

impl NodeTypeRegistry {
    pub fn builtin() -> Self {
        let mut types = BTreeMap::new();
        let mut add = |ty: &'static str,
                       title: &str,
                       doc: &str,
                       inputs: Vec<NodePort>,
                       outputs: Vec<NodePort>,
                       params: &'static [ParamSpec]| {
            types.insert(
                ty,
                NodeType {
                    schema: NodeTypeSchema {
                        node_type: ty.into(),
                        type_version: 1,
                        title: title.into(),
                        doc: doc.into(),
                        inputs,
                        outputs,
                        params_schema: params_schema(params),
                    },
                    params,
                },
            );
        };

        const GENERATE: &[ParamSpec] = &[
            ParamSpec {
                key: "provider",
                kind: ParamKind::Name,
                doc: "Configured provider name; empty = the session's main provider.",
            },
            ParamSpec {
                key: "lens",
                kind: ParamKind::Text { max_len: 2048 },
                doc: "Branch focus appended at the prompt TAIL (cache-safe), e.g. \
                      \"correctness and strict safety\". Quoted data, never trusted \
                      instructions.",
            },
        ];
        add(
            "generate",
            "Generate",
            "Produce the candidate response from the turn context.",
            vec![port("context", "context")],
            vec![port("response", "response")],
            GENERATE,
        );

        const CRITIC_GATE: &[ParamSpec] = &[
            ParamSpec {
                key: "critic",
                kind: ParamKind::Name,
                doc: "Configured provider name of the critic (also attachable via a capability edge).",
            },
            ParamSpec {
                key: "max_rounds",
                kind: ParamKind::UInt { min: 1, max: 5 },
                doc: "Generate/critique rounds before the exhaustion policy applies.",
            },
            ParamSpec {
                key: "scope",
                kind: ParamKind::Choice(&["final", "every-iteration"]),
                doc: "Gate only final answers, or every completion.",
            },
            ParamSpec {
                key: "on_exhaustion",
                kind: ParamKind::Choice(&["deliver-with-note", "fail"]),
                doc: "Deliver-with-note or fail the turn when rounds run out.",
            },
        ];
        add(
            "critic_gate",
            "Consensus gate",
            "Loop-until node: cross-check the response with a critic model, revising until \
             agreement (increment 01's engine).",
            vec![port("response", "response"), port("critic", "llm")],
            vec![port("response", "response")],
            CRITIC_GATE,
        );

        const DISTILL_SUMMARY: &[ParamSpec] = &[ParamSpec {
            key: "max_tokens",
            kind: ParamKind::UInt { min: 16, max: 4096 },
            doc: "Output-token budget for the rolling summary.",
        }];
        add(
            "distill_summary",
            "Distill: summary",
            "Background: merge the delivered exchange into the session's rolling summary row.",
            vec![port("exchange", "exchange")],
            vec![port("digest", "digest")],
            DISTILL_SUMMARY,
        );

        const DISTILL_FACTS: &[ParamSpec] = &[ParamSpec {
            key: "max_tokens",
            kind: ParamKind::UInt { min: 16, max: 4096 },
            doc: "Output-token budget for the key-facts extraction.",
        }];
        add(
            "distill_facts",
            "Distill: key facts",
            "Background: extract durable key facts from the delivered exchange (NO_FACTS = \
             success without output).",
            vec![port("exchange", "exchange")],
            vec![port("digest", "digest")],
            DISTILL_FACTS,
        );

        const OBJECTIVE: &[ParamSpec] = &[ParamSpec {
            key: "max_tokens",
            kind: ParamKind::UInt { min: 16, max: 512 },
            doc: "Output-token budget for the current-objective statement.",
        }];
        add(
            "objective",
            "Current objective",
            "Summarize the live context's current objective (filed to the ledger; drives \
             relevance selection).",
            vec![port("context", "context")],
            vec![port("objective", "digest")],
            OBJECTIVE,
        );

        const COMPACT_ASSEMBLE: &[ParamSpec] = &[
            ParamSpec {
                key: "relevance",
                kind: ParamKind::Choice(&["keyword", "llm", "all"]),
                doc: "How ledger summaries are selected against the objective.",
            },
            ParamSpec {
                key: "min_coverage",
                kind: ParamKind::Fraction,
                doc: "Ledger summaries per user turn required before assembly is trusted.",
            },
        ];
        add(
            "compact_assemble",
            "Instant compaction",
            "Assemble the compacted context from pre-computed ledger digests instead of \
             summarizing on the critical path (increment 03's strategy).",
            vec![port("context", "context")],
            vec![port("context", "context")],
            COMPACT_ASSEMBLE,
        );

        const SPLIT: &[ParamSpec] = &[ParamSpec {
            key: "max_branches",
            kind: ParamKind::UInt {
                min: 1,
                max: agent_core::MAX_SPLIT_BRANCHES as u64,
            },
            doc: "Fan-out allowance for this split (out-degree must not exceed it; \
                  branches multiply LLM spend).",
        }];
        add(
            "split",
            "Split",
            "Fork: duplicate the flowing context down each main out-edge as a concurrent \
             branch (lenses live on the branches' own nodes).",
            vec![port("context", "context")],
            vec![port("branch", "context")],
            SPLIT,
        );

        const JOIN: &[ParamSpec] = &[
            ParamSpec {
                key: "policy",
                kind: ParamKind::Choice(&["all", "any", "quorum"]),
                doc: "Activation: wait for every branch, the first, or `quorum_k` of them; \
                      satisfied ⇒ stragglers are cancelled.",
            },
            ParamSpec {
                key: "quorum_k",
                kind: ParamKind::UInt {
                    min: 1,
                    max: agent_core::MAX_ANCHOR_BRANCHES as u64,
                },
                doc: "Branches required when policy = quorum.",
            },
            ParamSpec {
                key: "timeout_ms",
                kind: ParamKind::UInt {
                    min: 1,
                    max: agent_core::MAX_JOIN_TIMEOUT_MS,
                },
                doc: "Wall-clock bound on the wait (clamped server-side).",
            },
            ParamSpec {
                key: "on_timeout",
                kind: ParamKind::Choice(&["partial", "fail"]),
                doc: "At the deadline: proceed with the finished branches (≥1), or fall \
                      back to the anchor's built-in behavior.",
            },
        ];
        add(
            "join",
            "Join",
            "Fan-in: Go-style wait on the split's branches per the activation policy; \
             unfinished branches are cancelled once it is satisfied.",
            vec![port("branch", "context")],
            vec![port("branches", "branches")],
            JOIN,
        );

        const MERGE: &[ParamSpec] = &[
            ParamSpec {
                key: "strategy",
                kind: ParamKind::Choice(&["compare", "synthesize", "concat"]),
                doc: "compare = a judge picks the best branch (position-swapped); \
                      synthesize = an aggregator combines the strongest elements; \
                      concat = mechanical, no LLM.",
            },
            ParamSpec {
                key: "judge",
                kind: ParamKind::Name,
                doc: "Configured provider name of the judge/aggregator (also attachable \
                      via a capability edge). Should be a different model family.",
            },
            ParamSpec {
                key: "record_losers",
                kind: ParamKind::Bool,
                doc: "File non-chosen branches as `alternatives` digest rows with a \
                      reconsider-when trigger (a forked exploration is never wasted).",
            },
        ];
        add(
            "merge",
            "Merge",
            "Consume the joined branch results into one outcome: pick the best, combine \
             ideas from several, or concatenate.",
            vec![port("branches", "branches")],
            vec![port("response", "response")],
            MERGE,
        );

        Self { types }
    }

    pub fn get(&self, node_type: &str) -> Option<&NodeType> {
        self.types.get(node_type)
    }

    /// Every registered schema, stable order (the editor's palette).
    pub fn schemas(&self) -> Vec<NodeTypeSchema> {
        self.types.values().map(|t| t.schema.clone()).collect()
    }
}

impl Default for NodeTypeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn positive_every_node_type_has_a_schema() {
        let reg = NodeTypeRegistry::builtin();
        let schemas = reg.schemas();
        assert_eq!(schemas.len(), 9);
        for s in &schemas {
            assert!(!s.title.is_empty() && !s.doc.is_empty(), "{}", s.node_type);
            assert_eq!(s.type_version, 1);
            // The derived JSON Schema is a closed object.
            assert_eq!(s.params_schema["additionalProperties"], false);
            assert!(reg.get(&s.node_type).is_some());
        }
    }

    #[rstest]
    #[case::no_params("critic_gate", serde_json::Value::Null, true)]
    #[case::empty_object("critic_gate", serde_json::json!({}), true)]
    #[case::full("critic_gate", serde_json::json!({
        "critic": "glm", "max_rounds": 2, "scope": "final", "on_exhaustion": "deliver-with-note"
    }), true)]
    #[case::generate_provider("generate", serde_json::json!({"provider": "task-router"}), true)]
    #[case::fraction_bounds("compact_assemble", serde_json::json!({"min_coverage": 1.0}), true)]
    #[case::negative_unknown_key("critic_gate", serde_json::json!({"criticc": "glm"}), false)]
    #[case::negative_wrong_type("critic_gate", serde_json::json!({"max_rounds": "two"}), false)]
    #[case::negative_bad_choice("critic_gate", serde_json::json!({"scope": "sometimes"}), false)]
    #[case::negative_params_not_object("generate", serde_json::json!([1, 2]), false)]
    #[case::boundary_rounds_low("critic_gate", serde_json::json!({"max_rounds": 1}), true)]
    #[case::boundary_rounds_high("critic_gate", serde_json::json!({"max_rounds": 5}), true)]
    #[case::boundary_rounds_over("critic_gate", serde_json::json!({"max_rounds": 6}), false)]
    #[case::boundary_rounds_zero("critic_gate", serde_json::json!({"max_rounds": 0}), false)]
    #[case::corner_coverage_zero("compact_assemble", serde_json::json!({"min_coverage": 0.0}), true)]
    fn boundary_param_validation(
        #[case] ty: &str,
        #[case] params: serde_json::Value,
        #[case] ok: bool,
    ) {
        let reg = NodeTypeRegistry::builtin();
        let got = reg.get(ty).expect("registered").validate_params(&params);
        assert_eq!(got.is_ok(), ok, "{got:?}");
    }

    // Resource names become registry lookups — path-shaped, ref-shaped, or
    // oversized names are refused at validation, never sanitized.
    #[rstest]
    #[case::traversal(serde_json::json!({"provider": "../../etc/passwd"}))]
    #[case::slash(serde_json::json!({"provider": "a/b"}))]
    #[case::leading_dash(serde_json::json!({"provider": "-rf"}))]
    #[case::empty(serde_json::json!({"provider": ""}))]
    #[case::huge(serde_json::json!({"provider": "x".repeat(10_000)}))]
    #[case::negative_number(serde_json::json!({"provider": -1}))]
    fn adversarial_hostile_resource_names_rejected(#[case] params: serde_json::Value) {
        let reg = NodeTypeRegistry::builtin();
        assert!(reg
            .get("generate")
            .unwrap()
            .validate_params(&params)
            .is_err());
    }

    // Hostile numbers: NaN/negative/huge must not slip through numeric params.
    #[rstest]
    #[case::nan_coverage("compact_assemble", "min_coverage", serde_json::Value::from(f64::NAN))]
    #[case::neg_coverage("compact_assemble", "min_coverage", serde_json::json!(-0.5))]
    #[case::over_coverage("compact_assemble", "min_coverage", serde_json::json!(1.5))]
    #[case::neg_rounds("critic_gate", "max_rounds", serde_json::json!(-3))]
    #[case::huge_tokens("distill_summary", "max_tokens", serde_json::json!(u64::MAX))]
    fn adversarial_hostile_numbers_rejected(
        #[case] ty: &str,
        #[case] key: &str,
        #[case] value: serde_json::Value,
    ) {
        let reg = NodeTypeRegistry::builtin();
        let params = serde_json::json!({ key: value });
        assert!(reg.get(ty).unwrap().validate_params(&params).is_err());
    }
}
