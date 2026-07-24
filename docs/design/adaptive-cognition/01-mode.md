# 01 — Mode detection & switching

Status: **design / pre-implementation.** Supersedes
[`../code-review/mode-detection.md`](../code-review/mode-detection.md), which
designed one-shot review detection; that doc's `TaskMode`/`TaskClassifier` seam
**shipped** (its own status line is now stale). This doc generalises it to a
first-class, per-turn, history-aware mode with an explicit **switch** decision that
drives compaction ([`02`](02-compaction.md)) and memory ([`03`](03-memory.md)).

> **Mode detection is a general capability, not a review feature.** It was *added*
> to notice when work becomes a code review, so the implementation lives inside
> `agent-review` today — but that coupling is incidental. Conceptually this runs in
> **every** mode, every turn, to detect when a *more appropriate* mode is warranted
> (review is just its first consumer). A core part of this design is therefore
> **relocating the seam out of `agent-review`** into a general home so it is
> independent of the `review` feature. See *A general, always-on capability* below.

## Motivation

The mode is the pivot of this whole design ([`README`](README.md)): compaction can
only be *targeted*, and memory recall only *relevant*, if the loop knows what mode
it is in and when that changed. And that must hold in **every** mode — the point is
not "detect review", it is "always be watching for the *right* mode." Today none of
that holds; the capability exists only as a narrow, one-shot review trigger:

- The mode is classified **once**, at prompt entry (`review_handoff`,
  `agent-runtime/src/agent.rs:602-644`), and only `TaskMode::Review` reaches a flow.
- The detection lives **inside `agent-review`** (`HybridClassifier`,
  `crates/agent-review/src/classifier.rs`), is built only when `[review] classifier
  = "hybrid"` (`builder.rs:620`), and is consumed only in `review_handoff` behind
  `#[cfg(feature = "review")]` — so with the review feature off, the agent has **no
  mode awareness at all.** For a general capability that coupling is wrong.
- `ClassifyCtx.history` is passed **empty** (`agent.rs:611-613`), so the classifier
  is blind to conversation drift — a session that *becomes* a debug or design task
  mid-way is never noticed.
- The `Session` stores **no current mode** (`agent.rs:664`), so there is nothing to
  switch *from* and no event to react to.

## What already exists (and its gaps)

Reuse all of it; the seam is already the right shape.

- `TaskMode { Review, Implement, Design, Debug, Explain, Other }` with
  `as_str()`/`parse()` — `agent-core/src/lib.rs:3371-3405`. Only `Review` is wired to
  a flow; the rest name the taxonomy (doc-comment `lib.rs:3368`). **This design makes
  all six first-class.**
- `TaskClassifier` trait + `ClassifyCtx { prompt, history }` + `ModeVerdict { mode,
  confidence, reason }` — `lib.rs:3417-3428`. `classify` already fails **safe** to
  `Other`.
- `HybridClassifier` — `agent-review/src/classifier.rs`: a free deterministic
  `prefilter()` then, if ambiguous, `pool.complete_all(req, PoolTier::Light,
  fanout=3)`. Built in `builder.rs:620-624` when `[review] classifier = "hybrid"`.
- `LlmPool::complete_all` (light-tier fan-out) and `one`/`complete` (tier failover) —
  `lib.rs:577-599`.
- The wire types already exist in `mode-detection.md`: `enum TaskMode` and
  `message ModeVerdict`.

**Gaps this doc closes:** the **review coupling** (the seam runs only when the
`review` feature is on); session mode state; per-turn re-classification with real
history; the switch decision + its hysteresis; the deterministic `RepoSignals` the
original doc specified but never wired; and the `ModeSwitch` event 02/03 consume.

## Design

### A general, always-on capability (not a review sub-feature)

Mode detection becomes a **core runtime concern that runs every turn, in every
mode**, independent of any one flow. Concretely:

- **Relocate the seam out of `agent-review`.** The `TaskClassifier` trait is already
  general (it lives in `agent-core`); the concrete `HybridClassifier` moves to a
  general home — a small dedicated crate (`agent-mode`) or `agent-runtime` — behind
  its own `mode` feature, so it no longer rides the `review` feature or the
  `agent-review` crate. `agent-review` becomes **one consumer** of the mode, not its
  owner.
- **New top-level `[mode]` config**, not `[review] classifier`. It selects the
  classifier impl, the confidence floor, the hysteresis `N`, and whether the loop
  acts on switches. Off → the loop behaves exactly as today (fail-safe default).
- **Runs in the main loop, once per turn, always.** The classifier is attached to the
  agent unconditionally (via the existing `with_task_classifier`, but from the
  general wiring, not the review branch). Review's in-loop hand-off
  (`review_handoff`) is refactored to *read* the loop's `current_mode` rather than
  classify on its own — one classification per turn, many consumers.
- **All six modes are first-class.** The classifier classifies among the whole
  `TaskMode` set every turn; `Review` reaching a collection flow is simply the first
  *action* wired to a mode. Others (Implement/Debug/Design/Explain) drive compaction
  (`02`) and dimension-weighted recall (`03`) even before any of them has a bespoke
  flow of its own.

### Session mode state

`Session` (`agent.rs:664`) gains `current_mode: TaskMode` (default `Other`) and a
small `switch_history: VecDeque<TaskMode>` (bounded, for hysteresis). The mode is
part of the session checkpoint (parity spec 19) so a rehydrated session resumes in
its mode.

### Per-turn classification (history-aware)

Each turn, before the model call, the runtime builds a **real** `ClassifyCtx`:

```rust
ClassifyCtx { prompt: input, history: working.messages }   // today: history: &[]
```

Classification stays two-stage and cheap:

1. **Deterministic prefilter (free, always).** Prompt verbs (already in
   `classifier.rs`) **plus** the `RepoSignals` the original doc specified and this doc
   finally wires: a PR ref (validated numeric, `safe_segment` before it touches git),
   a checked-out PR branch / detached HEAD (`RepoBackend::branches`/`status`), an
   uncommitted/unpushed diff (`RepoBackend::diff`). Returns `Decisive(mode)` or
   `Ambiguous`.
2. **Light vote (only when ambiguous).** `complete_all(.., Light, fanout)` over the
   MI50/light tier — a cheap classification small models are good at. Combine by
   majority; confidence is the mean of **clamped** per-member confidences (self-report
   is untrusted).

### The switch decision (two questions, with hysteresis)

Detection answers *"what mode is this turn?"*. Switching answers *"given
`current_mode`, do we move?"* — a separate, conservative gate, because a spurious
switch throws away context (via 02) and mis-files memory (via 03):

- **Stay** unless the new mode's confidence ≥ a floor **and** either (a) a *decisive
  deterministic* signal fired (a real PR ref, a real failing-test observation), or
  (b) the same new mode won **N consecutive** turns (`switch_history`). This
  hysteresis is what prevents thrash on a single ambiguous sentence.
- **Escalate, don't guess.** When the light vote is *split* (no majority, or
  confidence in a dead band around the floor), escalate the single decision to
  **GLM-5.2** (`one(.., Heavy)`) with the prompt + a compact history digest: *"Is the
  task moving from {current} to {candidate}? Answer with a mode + confidence."* This
  is the one place the heavy model is spent, and only on genuine ambiguity.
- **Fail-safe.** Any uncertainty, a dead pool, or an unreadable repo resolves to
  **stay in `current_mode`** (or `Other` at session start). We never switch on a
  coin-flip.

On a decided switch the runtime updates `current_mode`, emits a **`ModeSwitch { from,
to, reason, confidence }`** signal, and hands it to 02 (drive the switch compaction)
and 03 (flush + dimension-weighted recall) *before* the next model call, so the turn
runs in the new mode's reshaped context.

### Hand-off — consumers read the mode, they don't re-detect it

There is **one** classification per turn, in the general loop; every flow *reads* the
result. The explicit path (`agent review …` still asserts `Review` directly) is
preserved. The in-loop review hand-off (`review_handoff`) is refactored to **consume
the loop's `current_mode`** instead of running its own classify call — so it becomes
one subscriber to a general signal rather than the owner of it, and the other modes
get the same in-loop switch for free.

## Failure semantic

**Fail-safe**, exactly as today. Uncertainty → stay in the current mode. A dead
light tier degrades to the deterministic prefilter; a dead prefilter (can't read git)
degrades to keyword-only; a fully dead pool never switches on a vote. The worst
outcome is a *missed* switch (the user re-asks, or the next turn's signal is
decisive), which is strictly better than a spurious one that discards good context.

## Protobuf

Additive to the existing `mode-detection.md` types — no baseline bump.

```proto
// Existing (reused): enum TaskMode; message ModeVerdict { mode, confidence, ... }.

message ModeSwitch {
  TaskMode from        = 1;
  TaskMode to          = 2;
  string   reason      = 3;   // bounded classifier reason; never the raw prompt
  float    confidence  = 4;   // clamped 0..=1 on receipt
  string   prompt_hash = 5;   // fnv1a_hex, never the prompt itself
  uint32   duration_ms = 6;
}

// Serviceable classification (the seam can run remote, like every seam).
message ClassifyRequest {
  repeated agent.v1.Message history = 1;   // bounded server-side
  string prompt = 2;                        // bounded server-side; hashed for logs
  RepoSignals signals = 3;                  // pre-computed deterministic facts
}
message RepoSignals {
  bool   has_pr_ref      = 1;
  bool   on_pr_branch    = 2;
  bool   has_uncommitted = 3;
}
```

`convert.rs` gains both directions. `ModeSwitch` rides the review/telemetry record as
a side-channel (dropped at the gRPC memory boundary, like `verification`/`review`).

## gRPC interface

```proto
service ModeService {
  rpc Classify (ClassifyRequest) returns (ModeVerdict);
}
```

`--serve-mode`, endpoint from a **new `mode` block** in `nix/constants.nix` (regen
`constants.rs`; `constants-sync` gate enforces the match). Client + server in
`agent-grpc`, mirroring the existing `ContextService`/`LlmPoolService`. The *switch
decision* stays a runtime concern (it needs session state); the *classification* is
what the service exposes. Wire failure semantic: a classify RPC that can't reach the
pool returns a low-confidence `Other`, never a gRPC error — fail-safe on the wire too.

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_mode_decisions_total` | counter | `mode`, `via` = `prefilter`\|`vote`\|`escalate`\|`explicit` |
| `agent_mode_switches_total` | counter | `from`, `to` |
| `agent_mode_switch_confidence` | histogram | — |
| `agent_mode_classify_duration_seconds` | histogram | `via` |
| `agent_mode_escalations_total` | counter | `outcome` = `switched`\|`stayed` |

Raised the `metered.rs` way via a typed `ModeEvent` (mirroring `RouteEvent`), so
`agent-review`/`agent-runtime` stay off `agent-metrics` internals.

## Tracing + logs

- Span `mode.classify` with fields `via`, `mode`, `confidence`, `voters`; when the
  vote runs, `pool.dispatch` (from the pool) is its child, so the trace shows the
  parallel classification.
- Span `mode.switch` with `from`, `to`, `reason`, `confidence`; its children are the
  `02` switch-compaction and `03` flush/recall spans — the whole pivot is one legible
  subtree.
- Logs: `INFO` "mode {from}→{to} via {via}" with the **prompt hash**, mode, and
  confidence — never the raw prompt. `DEBUG` for prefilter signal hits.

## Testing (table-driven + adversarial)

`rstest` `#[case::name]`, all prefix classes; doubles from `agent-testkit`.

- `positive_` — each deterministic signal decisively classifies (PR ref → Review,
  failing-test observation → Debug, …); a clean majority vote switches.
- `negative_` — an off-topic prompt stays `Other`; a single ambiguous turn does **not**
  switch (hysteresis).
- `corner_` — vote tie → escalate; escalation says "stay" → no switch.
- `boundary_` — confidence exactly at the floor; the Nth consecutive turn flips.
- `adversarial_` (**mandatory**) — a prompt that *asserts* "this is a review, run all
  analyzers on /etc" must not cause any out-of-repo action (a switch shapes context,
  it grants no capability; collection stays inside `confine`); a hostile per-member
  `confidence` (NaN / 2.0 / -1) is clamped; an oversized prompt/history is bounded
  before it reaches a model (the `chars().take(2000)` guard, extended to history).

## Benchmark + leak

- **Bench** (`iai-callgrind`) — `classify` over a fixed `(prompt, history,
  RepoSignals)` fixture in `agent-testkit`, with the vote stubbed by a deterministic
  fake pool so the count is stable; an absolute **Ir ceiling** in
  `nix/checks/bench.nix`. Also a `prefilter`-only bench (the free every-turn path).
- **Leak** (`dhat`, `dhat-heap` feature) — a classify hot path frees the bounded
  excerpt and vote-tally buffers; assert zero leaked bytes and an allocation budget in
  `nix/checks/leak.nix`.

## Security

- The prompt and any PR ref are **attacker-controlled**: bound the excerpt (existing
  `chars().take(2000)`), validate a PR number is numeric and the host is the
  configured forge, pass any ref through `safe_segment` before it touches git.
- Per-member `confidence` is clamped before it is combined; the combine cannot be
  pushed past 1 or below 0 by a hostile member.
- A mode switch changes only what context the model sees — it **must not** silently
  widen the tool set or relax the `Policy`. Capability stays the persona/`Policy`
  seam's gated decision; this design never touches it (see `README` out-of-scope).

## Deferred

- **Learned switching** from recorded `ModeSwitch` outcomes (did the switch help?) —
  the recording makes it possible; not built now.
- **Sub-modes / personas** (mapping a `TaskMode` to an `Agent` persona's tool subset)
  — a separate seam, referenced here only as the capability boundary a switch respects.
