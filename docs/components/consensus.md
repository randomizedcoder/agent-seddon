# Consensus gate (`provider = "consensus"`)

The response-level quality gate (cognition-graph increment 01,
[design](../design/cognition-graph/01-consensus-gate.md)): a composing
`LlmProvider` where the **generator** answers and a **critic** — ideally a
different model family — judges a fresh, bounded context (task + candidate +
rubric, never the generator's transcript) and returns a strict-JSON verdict. On
a failing verdict the issues are appended as a tail user message (cache-safe)
and the generator revises: fix each issue, or rebut it with evidence.

## Exits

| Exit | When | Result |
|---|---|---|
| pass | verdict `pass: true` | deliver |
| fixed | pass after ≥1 revise round | deliver |
| alternatives | 2–3 defensible options, undecidable with current info | deliver + "Open alternatives" tail note (each with `reconsider_when`); recorded on the observer for the digest ledger (increment 02) |
| exhausted | `max_rounds` hit or identical issue set twice (no progress) | `deliver-with-note` (default: outstanding issues appended) or `fail` |
| critic_error | critic errored / unparseable verdict | **fail open**: deliver, counted |

## Config

```toml
[agent]
provider = "consensus"

[consensus]
generator = "kimi"      # resolve like route upstreams: a [[route.upstreams]] name
critic    = "glm"       # (inline endpoint, key via api_key_file/env) or a registry type
max_rounds = 2          # ceiling 5
scope = "final"         # "final" | "every-iteration" — final: tool-call iterations
                        # pass through (the verifier seam already gates those)
on_exhaustion = "deliver-with-note"   # | "fail"
critic_max_tokens = 512 # ceiling 4096 — see the reasoning-model note below
max_alternatives = 3    # ceiling 4
rubric_file = ""        # operator rubric; empty = compiled default
```

## Operational notes

- **Reasoning-model critics need output headroom.** GLM/Kimi-style critics spend
  `max_tokens` on `reasoning_content` *before* the verdict text; too small a
  budget truncates to empty content and every round fails open
  (`outcome="critic_error"`). Live-observed: a trade-off judgment burned >2k
  reasoning tokens. Size `critic_max_tokens` generously (2048–4096) for
  reasoning critics.
- **Streaming bypasses the gate** (buffering a stream to critique it defeats
  streaming) — run `stream = false` when gating matters.
- Same-id generator/critic is allowed but warned: self-critique loses the
  cross-family self-preference mitigation the design leans on.
- Hostile-verdict handling: confidence clamped, evidence-free issues and
  trigger-less alternatives dropped, counts/lengths capped, verdict text only
  quoted — never executed. Every knob has a hard compile-time ceiling.
- Cost: one critic call per round (+1 generator call per revise round). The
  parse/sanitize layer itself is benched (`gate_verdict`, Ir ceiling in
  `nix/checks/bench.nix`).

Observability: gate outcomes log under the `provider.complete` span
(`consensus gate outcome=… rounds=… alternatives=… confidence=…`); dedicated
`gate_*` metric families land with the graph-node re-expression (increment 04).
