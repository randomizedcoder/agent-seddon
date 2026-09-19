# 05 — The report

Both layers emit into one machine-readable report, keyed **page → element → case**, so a
run answers "what is the state of every page and every element of that page, and did the
outcome meet expectations?" A small renderer turns it into Markdown/HTML.

## Record schema (one per case executed)

```jsonc
{
  "run_id":     "…",          // one suite invocation (shared with the perf table)
  "layer":      "widget",      // unit|widget|golden|a11y|e2e
  "page":       "prompts",
  "element_id": "prompts.save",
  "case":       "adversarial_huge_content",
  "description":"Save rejects/handles an oversized prompt body",
  "expected_rpc":      "agent.v1.PromptService/Put",
  "expected_outcome":  "content capped; no crash; reload",
  "backend":    "up",          // up|down  (down ⇒ outcome=skip, never fail)
  "outcome":    "pass",        // pass|fail|skip
  "actual":     "…",           // on fail: what happened vs expected
  "rpc_fired":  ["agent.v1.PromptService/Put"],   // the recorded set (fired AND not-fired checked)
  "artifacts":  ["golden-diff/prompts-dark.png", "rpc-log/…"],  // on fail
  "trace_id":   "…"            // Layer B only — link to the ClickHouse trace
}
```

Layer A fills `rpc_fired` from the [`portal-testkit`](03-layer-a-widget.md) recording;
Layer B fills `trace_id` and the observability results (metrics delta, read-RPC state)
into `actual`. `flutter test --machine` (JSON reporter) is the transport for the
hermetic layers; the `portal-e2e` app appends its own records.

## Rendered report

The renderer produces a per-page section, each a table of elements × cases with
pass/fail/skip, a coverage line (elements with ≥1 case vs total keyed elements — tied to
the [completeness critic](03-layer-a-widget.md)), and a **backend legend** so a reader
instantly sees that, e.g., the Agent and Fleet pages were *skipped because their seams
were down*, not broken. This is the artifact that answers the "a report about every page
and every element" requirement directly.

## Failure artifacts

On failure the run captures, alongside the record:

- the **golden diff image** (visual failures),
- the **last widget snapshot** (a `pumpAndSettle` dump / screenshot),
- the **recorded-RPC log** (the full ordered call set — makes "wrong/extra RPC" obvious),
- (Layer B) the **`trace_id`**, a direct link into the ClickHouse/HyperDX trace
  ([`07`](07-envoy-otel.md)) so a live failure is one query from its full cross-hop
  timeline.

Artifacts are written under `$out` for the gated checks and to a run directory for
`portal-e2e`.

## Aggregation

A tiny renderer app (`nix run .#portal-test-report`, or folded into the checks' output)
merges the per-layer JSON into the single rendered report and, if present, joins the
[perf rows](06-performance.md) so each element's latest latency shows next to its
pass/fail. Kept dependency-light (a small script) per the repo's
[avoid-bash / prefer-expressive](../../../CLAUDE.md) convention — logic in Python behind
a thin shell shim.
