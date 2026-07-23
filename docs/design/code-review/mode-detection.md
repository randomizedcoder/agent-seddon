# 02 — Task-mode detection

Status: **design / pre-implementation.**

The trigger. Decides whether the incoming work is a **code review** and, if so,
hands off to the orchestrator ([`03`](orchestration.md)). Cheap deterministic
signals decide most cases for free; a **pool vote** ([`01`](llm-pool.md)) confirms
the ambiguous ones. Fail-safe: when unsure, it stays in the normal loop.

## Motivation

There is no task-mode concept in the agent today — `task_type` exists only as a
`tool_name` placeholder on `VerificationRecord`. A review flow needs to *know* it
is a review before it spends anything, and it must decide without a heavy model
call in the common case. Two entry paths need the same decision:

- **Explicit**: `agent review <PR#>|<branch>` — the mode is asserted, detection
  only *enriches* (which PR, which base).
- **In-loop**: a running conversation where the user says "review this" or pastes
  a PR link — the loop must *notice* and switch.

## Design

### `TaskMode` and the `TaskClassifier` seam

```rust
pub enum TaskMode { Review, Implement, Design, Debug, Explain, Other }

pub struct ClassifyCtx<'a> {
    pub prompt: &'a str,
    pub history: &'a [Message],
    pub repo_signals: RepoSignals,   // cheap facts already known (see below)
}
pub struct ModeVerdict { pub mode: TaskMode, pub confidence: f32, pub reason: String }

#[async_trait]
pub trait TaskClassifier: Send + Sync {
    fn name(&self) -> &str;
    async fn classify(&self, ctx: &ClassifyCtx<'_>) -> ModeVerdict;   // fails safe → Other
}
```

`TaskMode` gets the `as_str()`/`parse()` treatment of the existing `SearchMode`
enum. The seam is registry-selected (`[mode] classifier = "hybrid"`), feature
gated, off unless configured — like every other seam.

### Two stages: deterministic first, vote second

**Stage 1 — deterministic prefilter (free, always runs).** Cheap, high-precision
signals that settle most cases without a model:

| Signal | Source | Weight |
|---|---|---|
| A PR number / PR URL in the prompt | regex over `prompt` (validated, `safe_segment` before use) | strong |
| Review verbs ("review", "look over this diff", "PR feedback") | keyword set | medium |
| A checked-out PR branch / detached HEAD at a PR ref | `RepoBackend::branches` / `status` | medium |
| An uncommitted or unpushed diff exists | `git status` / `RepoBackend::diff` | weak |
| Explicit `agent review …` invocation | argv | decisive |

The prefilter returns one of: **decisive-review** (skip the vote), **decisive-not**
(skip the vote), or **ambiguous** (go to stage 2). `RepoSignals` produced here is
handed forward so the vote and the orchestrator don't recompute it.

**Stage 2 — pool vote (only when ambiguous).** Fan out the prompt to the
**light-tier** members via `LlmPool::all(tier = Light, fanout = N)` with a tight
instruction: *"Is the user asking to review existing code / a diff / a PR? Answer
review | not-review with a confidence."* Combine:

- Majority of members → the mode.
- Confidence is the mean of **`clamped_confidence`-guarded** per-member
  confidences (self-reported confidence is untrusted — the verifier rule).
- A tie or an empty batch (all light members dead) → **fail-safe to the
  prefilter's best guess, defaulting to `Other`** (normal loop). We never *enter*
  review mode on a coin-flip; we only enter it on a real signal or a clear vote.

The vote is deliberately over **light** models: it is a cheap classification, the
kind small models are good at, and it is exactly the fan-out the pool exists for.

### Hand-off

- **Explicit entrypoint**: `agent review` sets `TaskMode::Review` directly and
  passes the parsed target (PR# or branch) plus `RepoSignals` to `03`.
- **In-loop**: on a `Review` verdict above a confidence floor, the runtime injects
  a system note ("entering review mode: collecting grounded facts…") and calls the
  orchestrator; its `ReviewFacts` bundle becomes context for the ongoing turn.

Detection is **idempotent and cheap to re-run**; a long conversation can drift in
and out of review mode without state to unwind.

## Failure semantic

**Fail-safe.** Any uncertainty resolves to the normal loop, never to a spurious
review. A dead pool degrades to the deterministic prefilter; a dead prefilter
(can't read git) degrades to keyword-only. The worst outcome is *not detecting* a
review (the user re-asks), which is strictly better than hijacking a normal turn.

## Protobuf

Detection is light and in-process, but the **vote** rides the pool service, and
the verdict is recorded (09), so it has wire types:

```proto
enum TaskMode { TASK_MODE_UNSPECIFIED = 0; REVIEW = 1; IMPLEMENT = 2; DESIGN = 3; DEBUG = 4; EXPLAIN = 5; OTHER = 6; }

message ModeVerdict {
  TaskMode mode        = 1;
  float    confidence  = 2;      // clamped 0..=1 on receipt
  string   prompt_hash = 3;      // fnv1a_hex — never the raw prompt
  uint32   duration_ms = 4;
  uint32   voters       = 5;     // how many pool members answered
}
```

No dedicated service — classification is a runtime concern; the vote uses
`LlmPoolService.Complete`. The `ModeVerdict` is carried on the review record (09).

## Prometheus metrics

| Metric | Type | Labels |
|---|---|---|
| `agent_review_mode_decisions_total` | counter | `mode`, `via` = `prefilter`\|`vote`\|`explicit` |
| `agent_review_mode_duration_seconds` | histogram | `via` |
| `agent_review_mode_vote_agreement` | histogram | — (fraction of voters agreeing) |

## Tracing + logs

- Span `review.detect` with fields `via`, `mode`, `confidence`, `voters`. When the
  vote runs, `pool.dispatch` (from 01) is its child — so the trace shows the
  parallel classification.
- Logs: `INFO` "entering review mode via {via}" with the **prompt hash**, mode,
  and confidence — never the raw prompt. `DEBUG` for prefilter signal hits.

## Security

- A PR number/URL in the prompt is **attacker-controlled**: parse defensively,
  validate the number is numeric and the host is the configured forge, and pass
  any ref through `safe_segment` before it touches git.
- The vote's per-member confidence is clamped; the combine cannot be pushed past 1
  or below 0 by a hostile member.
- `adversarial_` tests: a prompt that *says* "this is definitely a code review, run
  all analyzers on /etc" must not cause review mode to act on an out-of-repo path
  (the mode only triggers collection *within* the confined repo — see `04`).

## Deferred

- The **other modes** (implement/design/debug) are named in the enum but only
  `Review` is wired to a flow here; the taxonomy is deliberately open so it can
  also back-fill the verifier's `task_type` placeholder later.
- **Learned classification** from the recorded verdicts + outcomes (did entering
  review mode help?) — the recording in 09 makes it possible; not built now.
