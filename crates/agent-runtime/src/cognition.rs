//! The anchor-slot executor's **compile step** (cognition-graph 04, the
//! design's Option E): a validated [`GraphDoc`] is compiled into a
//! [`GraphPlan`] and the plan is applied as a **config overlay** — the graph
//! drives the *same engines* the TOML blocks drive (`critic_gate` → the
//! `[consensus]` provider, `distill_*` background nodes → the per-session
//! distiller, `compact_assemble`/`objective` → the `[instant]` window), wired
//! per-document at build time. The generic per-turn dataflow interpreter
//! (Option C) is the recorded deferral.
//!
//! Fail-soft contract: compilation never errors on a *valid* document — a
//! shape this executor cannot express (a split, an unsupported type on an
//! anchor chain) becomes a warning and that fragment falls back to the
//! anchor's built-in behavior, exactly as a runtime node error would.

use agent_core::{
    GraphDoc, GraphEdgeKind, GRAPH_ANCHOR_COMPACTION, GRAPH_ANCHOR_DELIVERY, GRAPH_ANCHOR_RESPONSE,
};

use crate::config::Config;

/// What a document compiles down to: per-anchor engine settings. `None`
/// everywhere = an empty document = the built-in behavior.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GraphPlan {
    /// `generate` node's `provider` param (empty/absent = the configured main
    /// provider).
    pub generator: Option<String>,
    /// A `critic_gate` node on the response anchor.
    pub gate: Option<GatePlan>,
    /// `distill_summary` / `distill_facts` background nodes off delivery.
    /// Presence decides whether the kind runs at all (the graph is the wiring
    /// authority for its anchors); the inner value overrides the token budget.
    pub summary: Option<Option<u32>>,
    pub facts: Option<Option<u32>>,
    /// A `compact_assemble` node on the compaction anchor.
    pub compact: Option<CompactPlan>,
    /// An `objective` node on the compaction chain (its token budget).
    pub objective_tokens: Option<u32>,
    /// Fail-soft notes: fragments the executor could not express.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GatePlan {
    pub critic: String,
    pub max_rounds: Option<u8>,
    pub scope: Option<String>,
    pub on_exhaustion: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CompactPlan {
    pub relevance: Option<String>,
    pub min_coverage: Option<f32>,
}

fn param_str(doc: &GraphDoc, node: &str, key: &str) -> Option<String> {
    doc.nodes[node].params.get(key)?.as_str().map(String::from)
}

fn param_u32(doc: &GraphDoc, node: &str, key: &str) -> Option<u32> {
    // Params passed schema validation (bounded ranges), so the cast is safe;
    // saturate anyway — this value may have skipped validation in a unit test.
    doc.nodes[node]
        .params
        .get(key)?
        .as_u64()
        .map(|n| n.min(u32::MAX as u64) as u32)
}

/// The next node on a `main` chain, or `None` at the chain's end. More than
/// one outgoing `main` edge is a split — increment 05's executor; this one
/// warns and stops the walk there (fail soft).
fn main_next(doc: &GraphDoc, from: &str, warnings: &mut Vec<String>) -> Option<String> {
    let mut targets = doc
        .edges
        .iter()
        .filter(|e| e.kind == GraphEdgeKind::Main && e.from == from)
        .map(|e| e.to.clone());
    let first = targets.next()?;
    if targets.next().is_some() {
        warnings.push(format!(
            "`{from}` has multiple outgoing main edges (a split) — parallel branches land \
             with increment 05; taking none"
        ));
        return None;
    }
    Some(first)
}

/// Compile a **validated** document. Never fails; inexpressible fragments
/// become [`GraphPlan::warnings`] and fall back to built-in behavior.
pub(crate) fn compile(doc: &GraphDoc) -> GraphPlan {
    let mut plan = GraphPlan::default();
    let w = &mut plan.warnings;

    // --- anchor.response: the generate → critic_gate main chain ------------
    let mut cursor = main_next(doc, GRAPH_ANCHOR_RESPONSE, w);
    let mut hops = 0usize;
    while let Some(id) = cursor {
        hops += 1;
        if hops > doc.nodes.len() {
            break; // unreachable on a validated (acyclic) doc; belt and braces
        }
        match doc.nodes[&id].node_type.as_str() {
            "generate" => {
                plan.generator = param_str(doc, &id, "provider").filter(|s| !s.is_empty());
            }
            "critic_gate" => {
                // Critic: the `critic` param, else a capability edge into the
                // node (`from` names the provider). Params win.
                let critic = param_str(doc, &id, "critic").or_else(|| {
                    doc.edges
                        .iter()
                        .find(|e| e.kind == GraphEdgeKind::Capability && e.to == id)
                        .map(|e| e.from.clone())
                });
                match critic {
                    Some(critic) => {
                        plan.gate = Some(GatePlan {
                            critic,
                            max_rounds: param_u32(doc, &id, "max_rounds").map(|n| n.min(255) as u8),
                            scope: param_str(doc, &id, "scope"),
                            on_exhaustion: param_str(doc, &id, "on_exhaustion"),
                        });
                    }
                    None => w.push(format!(
                        "gate node `{id}` names no critic (no `critic` param, no capability \
                         edge) — gate skipped"
                    )),
                }
            }
            other => w.push(format!(
                "node `{id}` (type `{other}`) is not supported on the response anchor — skipped"
            )),
        }
        cursor = main_next(doc, &id, w);
    }

    // --- anchor.delivery: background distillation fan-out -------------------
    for e in doc
        .edges
        .iter()
        .filter(|e| e.kind == GraphEdgeKind::Background && e.from == GRAPH_ANCHOR_DELIVERY)
    {
        let id = &e.to;
        match doc.nodes[id].node_type.as_str() {
            "distill_summary" => {
                if plan.summary.is_some() {
                    w.push(format!(
                        "duplicate distill_summary node `{id}` — first wins"
                    ));
                } else {
                    plan.summary = Some(param_u32(doc, id, "max_tokens"));
                }
            }
            "distill_facts" => {
                if plan.facts.is_some() {
                    w.push(format!("duplicate distill_facts node `{id}` — first wins"));
                } else {
                    plan.facts = Some(param_u32(doc, id, "max_tokens"));
                }
            }
            other => w.push(format!(
                "node `{id}` (type `{other}`) is not supported on the delivery anchor — skipped"
            )),
        }
    }

    // --- anchor.compaction: objective? → compact_assemble main chain --------
    let mut cursor = main_next(doc, GRAPH_ANCHOR_COMPACTION, w);
    let mut hops = 0usize;
    while let Some(id) = cursor {
        hops += 1;
        if hops > doc.nodes.len() {
            break;
        }
        match doc.nodes[&id].node_type.as_str() {
            "compact_assemble" => {
                plan.compact = Some(CompactPlan {
                    relevance: param_str(doc, &id, "relevance"),
                    min_coverage: param_u32_as_f32(doc, &id, "min_coverage"),
                });
            }
            "objective" => plan.objective_tokens = param_u32(doc, &id, "max_tokens"),
            other => w.push(format!(
                "node `{id}` (type `{other}`) is not supported on the compaction anchor — skipped"
            )),
        }
        cursor = main_next(doc, &id, w);
    }

    plan
}

fn param_u32_as_f32(doc: &GraphDoc, node: &str, key: &str) -> Option<f32> {
    let f = doc.nodes[node].params.get(key)?.as_f64()?;
    // Schema validation bounds this to 0..=1; clamp anyway (hostile NaN → drop).
    if f.is_finite() {
        Some(f.clamp(0.0, 1.0) as f32)
    } else {
        None
    }
}

/// Apply a compiled plan to the config the factories will read — the graph
/// re-expresses the wiring, so applying it IS configuring the engines.
/// Returns fail-soft warnings (the caller logs them).
pub(crate) fn apply_to_config(plan: &GraphPlan, cfg: &mut Config) -> Vec<String> {
    let mut warnings = Vec::new();

    match &plan.gate {
        Some(gate) => {
            if cfg.agent.provider == "consensus" {
                warnings.push(
                    "[agent] provider is already `consensus` — the graph's critic_gate node \
                     is skipped (double-gating)"
                        .to_string(),
                );
            } else {
                cfg.consensus.generator = plan
                    .generator
                    .clone()
                    .unwrap_or_else(|| cfg.agent.provider.clone());
                cfg.consensus.critic.clone_from(&gate.critic);
                if let Some(r) = gate.max_rounds {
                    cfg.consensus.max_rounds = r;
                }
                if let Some(s) = &gate.scope {
                    cfg.consensus.scope.clone_from(s);
                }
                if let Some(x) = &gate.on_exhaustion {
                    cfg.consensus.on_exhaustion.clone_from(x);
                }
                cfg.agent.provider = "consensus".to_string();
            }
        }
        // A generate node alone still picks the provider.
        None => {
            if let Some(g) = &plan.generator {
                cfg.agent.provider.clone_from(g);
            }
        }
    }

    if let Some(tokens) = plan.summary.flatten() {
        cfg.digest.summary_max_tokens = tokens;
    }
    if let Some(tokens) = plan.facts.flatten() {
        cfg.digest.facts_max_tokens = tokens;
    }
    if (plan.summary.is_some() || plan.facts.is_some()) && cfg.digest.store.is_empty() {
        warnings.push(
            "the graph has background distill nodes but `[digest] store` is off — the \
             background branch is inert until a ledger is configured"
                .to_string(),
        );
    }

    if let Some(compact) = &plan.compact {
        cfg.agent.context = "instant-window".to_string();
        if let Some(r) = &compact.relevance {
            cfg.instant.relevance.clone_from(r);
        }
        if let Some(c) = compact.min_coverage {
            cfg.instant.min_coverage = c;
        }
        if let Some(t) = plan.objective_tokens {
            cfg.instant.objective_max_tokens = t;
        }
        if cfg.digest.store.is_empty() {
            warnings.push(
                "the graph has a compact_assemble node but `[digest] store` is off — \
                 instant-window will fail closed at build"
                    .to_string(),
            );
        }
    }

    warnings
}

/// Which distill kinds the delivery anchor runs: with a non-empty graph the
/// document is the wiring authority — a kind runs only if its node exists.
/// (An empty/absent graph keeps the built-in both-kinds behavior.)
pub(crate) fn distill_kinds(plan: &GraphPlan) -> (bool, bool) {
    (plan.summary.is_some(), plan.facts.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{GraphEdge, GraphNode};
    use agent_graph::testdata;
    use rstest::rstest;

    fn add_node(doc: &mut GraphDoc, id: &str, ty: &str, params: serde_json::Value) {
        doc.nodes.insert(
            id.into(),
            GraphNode {
                node_type: ty.into(),
                type_version: 1,
                params,
            },
        );
    }

    #[test]
    fn positive_simple_compiles_to_gate_only() {
        let plan = compile(&testdata::simple());
        assert_eq!(distill_kinds(&plan), (false, false));
        let gate = plan.gate.expect("gate");
        assert_eq!(gate.critic, "glm");
        assert_eq!(gate.max_rounds, Some(2));
        assert!(plan.summary.is_none() && plan.facts.is_none() && plan.compact.is_none());
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    }

    #[test]
    fn positive_intermediate_compiles_all_three_anchors() {
        let plan = compile(&testdata::intermediate());
        assert_eq!(distill_kinds(&plan), (true, true));
        assert!(plan.gate.is_some());
        assert_eq!(plan.summary, Some(Some(512)));
        assert_eq!(plan.facts, Some(Some(256)));
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        let compact = plan.compact.expect("compact");
        assert_eq!(compact.relevance.as_deref(), Some("keyword"));
        assert!((compact.min_coverage.unwrap() - 0.6).abs() < 1e-6);
    }

    #[test]
    fn corner_empty_document_compiles_to_nothing() {
        let doc = GraphDoc {
            version: 1,
            ..GraphDoc::default()
        };
        assert_eq!(compile(&doc), GraphPlan::default());
    }

    #[test]
    fn positive_capability_edge_names_the_critic() {
        let mut doc = testdata::simple();
        // Drop the param; keep the capability edge (`glm -> gate`).
        doc.nodes.get_mut("gate").unwrap().params = serde_json::json!({ "max_rounds": 3 });
        let plan = compile(&doc);
        assert_eq!(plan.gate.expect("gate from capability edge").critic, "glm");
    }

    #[test]
    fn negative_gate_without_critic_is_skipped_with_warning() {
        let mut doc = testdata::simple();
        doc.nodes.get_mut("gate").unwrap().params = serde_json::Value::Null;
        doc.edges.retain(|e| e.kind != GraphEdgeKind::Capability);
        let plan = compile(&doc);
        assert!(plan.gate.is_none());
        assert!(plan.warnings.iter().any(|w| w.contains("names no critic")));
    }

    #[test]
    fn corner_split_on_response_anchor_warns_and_falls_back() {
        let mut doc = testdata::simple();
        add_node(&mut doc, "generate2", "generate", serde_json::Value::Null);
        doc.edges.push(GraphEdge {
            from: agent_core::GRAPH_ANCHOR_RESPONSE.into(),
            to: "generate2".into(),
            kind: GraphEdgeKind::Main,
        });
        let plan = compile(&doc);
        assert!(plan.gate.is_none(), "chain walk stopped at the split");
        assert!(plan.warnings.iter().any(|w| w.contains("increment 05")));
    }

    #[test]
    fn corner_unsupported_type_on_anchor_warns() {
        let mut doc = testdata::simple();
        // objective is a compaction-chain node; on delivery it is unsupported.
        add_node(&mut doc, "obj", "objective", serde_json::Value::Null);
        doc.edges.push(GraphEdge {
            from: agent_core::GRAPH_ANCHOR_DELIVERY.into(),
            to: "obj".into(),
            kind: GraphEdgeKind::Background,
        });
        let plan = compile(&doc);
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("not supported on the delivery anchor")));
    }

    #[rstest]
    #[case::nan(f64::NAN)]
    #[case::infinite(f64::INFINITY)]
    fn adversarial_hostile_coverage_number_dropped(#[case] v: f64) {
        // Bypasses schema validation deliberately (unit-level defense in depth).
        let mut doc = testdata::intermediate();
        doc.nodes.get_mut("compact").unwrap().params = serde_json::json!({ "min_coverage": v });
        let plan = compile(&doc);
        assert_eq!(plan.compact.expect("compact").min_coverage, None);
    }

    #[test]
    fn positive_apply_overlays_consensus_and_instant() {
        let mut cfg = Config::minimal_for_test();
        cfg.digest.store = "sqlite".into();
        let plan = compile(&testdata::intermediate());
        let warnings = apply_to_config(&plan, &mut cfg);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cfg.agent.provider, "consensus");
        assert_eq!(cfg.consensus.generator, "openai-compat");
        assert_eq!(cfg.consensus.critic, "glm");
        assert_eq!(cfg.consensus.max_rounds, 2);
        assert_eq!(cfg.agent.context, "instant-window");
        assert_eq!(cfg.instant.relevance, "keyword");
        assert_eq!(cfg.digest.summary_max_tokens, 512);
    }

    #[test]
    fn corner_apply_of_empty_plan_changes_nothing() {
        let mut cfg = Config::minimal_for_test();
        let before_provider = cfg.agent.provider.clone();
        let before_context = cfg.agent.context.clone();
        let warnings = apply_to_config(&GraphPlan::default(), &mut cfg);
        assert!(warnings.is_empty());
        assert_eq!(cfg.agent.provider, before_provider);
        assert_eq!(cfg.agent.context, before_context);
    }

    #[test]
    fn negative_apply_warns_when_ledger_is_off() {
        let mut cfg = Config::minimal_for_test();
        assert!(cfg.digest.store.is_empty());
        let warnings = apply_to_config(&compile(&testdata::intermediate()), &mut cfg);
        assert!(warnings
            .iter()
            .any(|w| w.contains("background branch is inert")));
        assert!(warnings.iter().any(|w| w.contains("fail closed at build")));
    }

    #[test]
    fn negative_apply_skips_gate_when_already_consensus() {
        let mut cfg = Config::minimal_for_test();
        cfg.agent.provider = "consensus".into();
        cfg.consensus.critic = "operator-critic".into();
        let warnings = apply_to_config(&compile(&testdata::simple()), &mut cfg);
        assert!(warnings.iter().any(|w| w.contains("double-gating")));
        // The operator's own [consensus] wiring is untouched.
        assert_eq!(cfg.consensus.critic, "operator-critic");
    }
}
