# Design: Adaptive Cognition (mode · compaction · memory)

Status: **design / pre-implementation.** This directory is the design of record for
spending cheap local LLMs on the agent's own *meta-cognition*. It is a draft to
refine; where the shipped code later refines a detail, a `STATUS.md` tracker (added
when implementation starts) becomes authoritative — the same convention as
[`../code-review/`](../code-review/README.md).

## The idea

`agent-seddon` is built to **not** lean on expensive API models. The target
deployment is a pool of cheap, heterogeneous local ones: a colocated **GLM-5.2** on
8×MI300 (192 GB each) that is powerful *and* nearly free, a local **MI50** (32 GB,
`mistral-small:24b`) fine for routine jobs, and a small **RTX 3070** for trivial
ones. The [Code Review Flow](../code-review/README.md) already spends that economics
on *reviewing code*. This design spends it on the agent **managing itself**.

Three functions the loop today does mechanically, or not at all:

> **Mode** — know what kind of work this is, and notice when it changes.
> **Compaction** — when it changes, reshape the context for the new mode, because
> what was useful in one mode is noise in the next.
> **Memory** — as we go, summarize each step *by dimension* and keep a per-dimension
> history, so the right past context can be recalled for the mode we're now in.

The **pivot that connects all three is the mode switch.** A switch is the single
event that (a) triggers a *targeted* compaction, which (b) *sheds* the old mode's
now-irrelevant transcript — but first *flushes* it into dimensional memory — and (c)
*pulls in* the dimensions the new mode needs. Nothing is lost: context flows
**working-set → dimensional history → recalled when relevant.** Because the raw
episodic log is never mutated (a standing `ContextStrategy` invariant), an aggressive
shed is always safe — it can be reconstructed.

## The pipeline

```
                        ┌───────────────────────────────────────────────┐
   each turn ─────────▶ │  01 mode  — classify (history-aware) + decide  │
                        │       switch? (deterministic → light vote →    │
                        │       GLM-5.2 only on disagreement)            │
                        └───────────────┬───────────────────────────────┘
                        same mode       │ ModeSwitch{from,to,reason,confidence}
                            │           ▼
                            │   ┌───────────────────────────────────────┐
                            │   │  the switch fans out to 02 + 03:       │
                            │   └───────┬───────────────┬───────────────┘
                            │           ▼               ▼
                            │   ┌───────────────┐┌───────────────────────┐
                            │   │ 03 flush old  ││ 03 dimension-weighted │
                            │   │ transcript →  ││ recall → pull in the  │
                            │   │ dim history   ││ new mode's dimensions  │
                            │   └───────┬───────┘└───────────┬───────────┘
                            ▼           ▼                    ▼
                        ┌───────────────────────────────────────────────┐
                        │  02 compaction — normal (budget) between        │
                        │  switches; SWITCH compaction on a switch:       │
                        │  re-summarize the middle through the new mode's │
                        │  lens + shed by the before/after table          │
                        └───────────────────────────────────────────────┘
```

## The components

| # | Doc | Component | One line |
|---|---|---|---|
| 01 | [`01-mode.md`](01-mode.md) | **Mode detection & switching** | A **general, always-on** capability — runs every turn in *any* mode to detect when to switch to a more appropriate one. Per-turn, history-aware `TaskMode` detection with a hysteresis switch decision; deterministic → light vote → GLM-5.2 escalation. Relocates the seam out of `agent-review`; supersedes `../code-review/mode-detection.md`. |
| 02 | [`02-compaction.md`](02-compaction.md) | **Mode-aware compaction** | A `ModeAwareWindow` that compacts *through the destination mode's lens* on a switch, shedding by an explicit before/after table. |
| 03 | [`03-memory.md`](03-memory.md) | **Dimensional memory** | A cheap LLM summarizes each step *by dimension* (seeded + emergent) into per-dimension histories; dimension-weighted recall feeds 02. |

## The local-LLM role table

The user's economics made concrete — which model does what, how often, and why.
Everything rides the **existing** `LlmPool` tiers (`../code-review/llm-pool.md`); no
new backend is introduced.

| Function | Tier | Model | Cadence | Why |
|---|---|---|---|---|
| Deterministic mode prefilter | none | — | every turn | free, high-precision |
| Routine mode vote | light / medium | MI50 32B | ambiguous turns only | cheap classification |
| Hard switch decision | heavy | GLM-5.2 | on light-vote disagreement | high-stakes, escalate |
| Per-step dimension tag + summary | medium | MI50 32B | per window / step | frequent, cheap |
| Switch-compaction summary | medium | MI50 32B | on a mode switch | frequent-ish |
| Cross-dimension synthesis | heavy | GLM-5.2 | periodic / on file growth | quality merge |

## Three principles

1. **The mode is the pivot.** Detection, compaction, and memory are one loop joined
   at the switch — not three unrelated features. A switch is what makes a compaction
   *targeted* and a memory recall *relevant*.
2. **Cheap-heavy, tiered.** Spend the free deterministic check first, the MI50 for
   the frequent routine work, and reserve GLM-5.2 for the genuinely hard calls
   (an ambiguous switch, a cross-dimension merge). Escalate on disagreement, never
   by default.
3. **Nothing is destroyed, only moved.** Compaction sheds from the *working set*
   only; the shed content is first flushed to dimensional memory and the episodic
   log is untouched, so every shed is reversible by recall.

## Observability & quality contract

Every component here ships the **same full set** the code-review flow does — this is
mandatory, not aspirational (`../../../CLAUDE.md`, and the code-review house style).
Each increment doc has a section for each of these, and no seam is "done" without
all of them:

- **Protobuf** — new/updated `.proto` messages and enums (closed sets → enums;
  untrusted floats clamped on receipt; hashes/counts, never raw prompts or source).
  **Additive** so `buf breaking` passes against the committed
  `crates/agent-proto/buf.image.binpb` with **no baseline bump**; a wire-incompatible
  edit deliberately bumps it via `nix run .#buf-image` (the diff records it).
  `convert.rs` both directions; telemetry-local side-channels are dropped at the gRPC
  boundary (the `verification`/`review` precedent).
- **gRPC** — each seam runs as its own service (`--serve-<name>`), the port from a
  **new block in `nix/constants.nix`** (regenerate `constants.rs` with
  `nix run .#gen-constants`; the `constants-sync` gate enforces the match). Client +
  server in `agent-grpc`. The wire failure semantic is stated (fail-soft: a
  per-member error is a field, not an RPC error).
- **Prometheus** — typed metric families with labels, raised the `metered.rs`
  decorator / typed-event way (`agent-providers` stays off `agent-metrics`).
- **Tracing + logs** — named spans with fields and the fan-out child-span tree;
  **never** raw prompts, URLs, or bodies — hashes, counts, tiers, durations only.
- **Table-driven tests** — `rstest` `#[case::name]`, all classes by prefix
  (`positive_`/`negative_`/`corner_`/`boundary_`), and **mandatory `adversarial_`**
  for every untrusted input (traversal / injection / overflow / huge), each asserting
  the rejection. Doubles + `tempdir()` from `agent-testkit`; `#[cfg(test)] mod` at the
  end of the file.
- **Perf benchmark** — an `iai-callgrind` bench over a deterministic `agent-testkit`
  input, with an **absolute Ir ceiling** in `nix/checks/bench.nix`.
- **Leak analysis** — a `dhat` `tests/leak.rs` behind the crate's `dhat-heap`
  feature: a hot path frees everything it allocates, under an allocation budget;
  wired in `nix/checks/leak.nix`.
- A **hermetic `nix/checks/<name>.nix`** gate, registered in `nix/checks/default.nix`,
  plus the matching `docs/components/*.md`.

## Service + metrics allocation

| Service (`--serve-…`) | `nix/constants.nix` block | Fan-out span | Status |
|---|---|---|:--:|
| `mode` | new `mode` block | `mode.classify` / `mode.switch` | designed (01) |
| `context` (existing) | existing `context` block | `context.compact` (enriched) | designed (02) |
| `dimension` | new `dimension` block | `memory.dimension.*` | designed (03) |

Metric families `agent_mode_*`, `agent_context_*`, and `agent_dimension_*` are
specified in the respective docs' Prometheus sections.

## Status

All three components are **design / pre-implementation.** Nothing is coded yet. The
intended build order mirrors the code-review cadence — one focused, individually
gated PR per increment, each earning the next:

1. **01 mode** — the pivot everything else keys off (session mode state + the switch
   event). Cheapest and highest-leverage.
2. **02 compaction** — react to the switch event; the before/after table is the spec.
3. **03 memory** — the dimensional store + the flush/recall that closes the loop.

## Follow-up (noted, not designed here): GPU health-check + capacity-aware load-balancing

A natural question this design raises: if the MI50 is offline **or busy**, calls
should route to GLM-5.2 (and vice-versa), behind an abstraction over a pool of
available GPU resources. Honest accounting of where that stands:

- **Already exists.** The `LlmPool` seam (`../code-review/llm-pool.md`,
  `PoolProvider`) does **active health-probing + a per-member circuit breaker + tier
  failover**: a dead MI50 opens its breaker and `one(min = Medium)` degrades up-tier
  to GLM-5.2 automatically, and the probe answers "will this model actually respond?"
  for an intermittent endpoint. So *alive/dead* rerouting between the two is a config
  away — not a new feature.
- **New (the follow-up).** Today the breaker distinguishes only alive vs dead, not
  *loaded*. A true GPU-resource pool would track a per-endpoint **capacity signal**
  (queue depth / in-flight / latency) and prefer the least-loaded live member,
  feeding the pool's existing `order()` ranking — plus an explicit resource-pool
  abstraction so MI50 ↔ GLM-5.2 (and future GPUs) are interchangeable capacity behind
  a tier. This extends the `LlmPool` deferred list ("cost/latency-minimising policy
  beyond cheapest", "adaptive fan-out"); it is the reliability substrate 01 and 03
  assume, and is best done as its own increment there rather than here.

## House rules these docs follow

- The security model of [`../../../CLAUDE.md`](../../../CLAUDE.md): the model and all
  repo/tool/remote content are **untrusted**. Paths through `confine`, ids/slugs
  through `safe_segment`, LLM/tool output capped and injection-scanned, pool
  hints/counts clamped. `adversarial_` tests are mandatory.
- The seam conventions of [`../../extending.md`](../../extending.md) and the wire
  conventions of [`../../grpc.md`](../../grpc.md).
- The measurement-first ethos of [`../tool-call-verification.md`](../tool-call-verification.md)
  and the code-review flow: record the decisions (mode switches, sheds, dimension
  summaries) so their value can be analysed offline, not assumed.
