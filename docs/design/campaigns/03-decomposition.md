# 03 — Decomposition: the recursive "is this small enough?" loop

The planner is a bounded step inside the driver tick ([`04-executor.md`](04-executor.md)). Per
tenant it selects up to `plan_per_tick` nodes with `state = 'ready' AND kind <> 'leaf'` (index
`tasks_plan`) in `(campaign_id, path)` order — oldest campaign first, then by path prefix, so a
parent is asked before anything under it — and asks the model one question per node. Children
created by a `split` are asked on later ticks. The loop terminates because every answer either
marks a leaf, blocks, waits for a human, or adds children one level deeper under `max_depth`,
`max_children` and `max_nodes`.

As built (CP-03): `agent_campaign::Planner` (`crates/agent-campaign/src/planner/`), one
`plan_node(task)` per node and `tick(limit)` over `plannable(limit)`. It owns its structured-output
loop (`planner/ask.rs`) rather than reusing `agent_runtime::structured`: the dependency edge runs
`agent-runtime → agent-campaign` (CP-04), and the planner needs the summed `Usage` and a byte cap
that helper does not keep. Every outcome the design names is a `PlanOutcome`; only `NotFound` and
backend errors propagate.

## Step 1: take the node

`ready → decomposing` by CAS, recording `expected_version` (the version after the CAS). A node
whose `attempts >= policy.max_plan_attempts` is moved to `blocked` instead, with
`detail.reason = 'attempts_exhausted'`. A node whose campaign has spent
`SUM(tokens_in + tokens_out) FROM task_attempts WHERE campaign_id = …` at or above
`max_plan_tokens` is moved to `blocked` with `detail.reason = 'token_cap'`, without a provider
call; either `blocked` rolls up to the ancestors like a failed leaf
([`02-transactions.md`](02-transactions.md) (b)). The planner computes
`idem_key = sha256(tenant ‖ \0 ‖ task_id ‖ \0 ‖ expected_version ‖ \0 ‖ prompt_hash)`
(`planner/hash.rs`, NUL-separated so no two field splits collide) here but writes **no**
`task_attempts` row yet: the row is inserted inside the finishing transaction (`decompose`,
`mark_leaf` or `plan_close`), so a `Conflict` there rolls the attempt back with everything else
and the key stays usable (T5 `negative_version_conflict`).

Before the provider call the planner scans the node's existing attempts for that key. A hit means
this exact input was already answered (a replayed or partially committed tick); the planner makes
no call and closes the node as `error` under a distinct *replay* key
(`sha256("replay" ‖ \0 ‖ prompt_hash)`) so the node returns to `ready` instead of wedging in
`decomposing` (the store would answer `AlreadyApplied` to the original key and change nothing).
The store's `AlreadyApplied` at insert remains the backstop (T10 `corner_unchanged_input_no_call`).

The planner never descends past a node in `awaiting_approval`: children of an unapproved node do
not exist yet, and a `needs_info` node is not `ready`.

## Step 2: build the prompt

Inputs, in this order, each inside its own fenced block with a random tag (so model-written text
cannot close the fence):

1. **The node**: title, goal, acceptance so far, `est_size`, `touches` so far, `depth`,
   `max_depth`, the number of live siblings.
2. **Ancestors**, root → parent: title and goal each, ≤ 6 rows.
3. **Live siblings**: title and state, ≤ 8 lines.
4. **The repo brief** for the node's goal: `agent repo brief --goal "<title>: <goal>"` (RK-12,
   ≤ 6 KiB, L0 + L1). Until RK-12 lands, the fallback (`FallbackBrief`, `planner/brief.rs`) is
   `docs/architecture.md` followed by the "Conventions" and "Security" sections of `CLAUDE.md`
   (the sections ≤ 3 KiB, the whole ≤ 6 KiB); a source that fails to read yields
   `[brief unavailable: …]` and the node is still planned. The brief comes through the
   `BriefSource` seam so CP-07 swaps RK-12 in without touching the planner.
5. **The rules** (fixed Rust text, in the system message with the role): what `execute` means
   (D7), the size vocabulary, that `touches` must be exact repository paths (node keys arrive with
   RK-08 / CP-07), that `depends_on` lists sibling ordinals, that at most 8 children are allowed,
   that the answer is JSON matching the schema and nothing else.

Every model-written input passes `scan_for_injection` (`crates/agent-core/src/security.rs:97`)
before anything is rendered, in prompt order: the node's `title`, `goal`, `acceptance[i]`,
`touches[i]`; each ancestor's `ancestor:<id>:title` and `ancestor:<id>:goal`; each sibling's
`sibling:<id>:title`. The first hit names its field: the planner makes no provider call and closes
the node through `plan_close(Injection { field })` — `blocked` with `detail.reason = 'injection'`
and `detail.field`, the attempt closed as `error` (`injection: <field>`), `attempts` untouched
(an input injection is not the model's fault and must not eat its retries), rollup like any
blocked node. The brief is **not** screened: `CLAUDE.md` itself discusses injection phrases, and
RK-12's brief is screened by its own track.

Fence tags are random per call (`uuid` v4, 32 hex) so a marker the model wrote on an earlier tick
cannot close a block. `prompt_hash` must still be stable, so the same inputs are rendered a second
time with the canonical tag `"0" × 32` and hashed together with the fixed system text and the
schema JSON: `prompt_hash = sha256(system ‖ \0 ‖ canonical user render ‖ \0 ‖ schema)`. Both tags
are 32 bytes, every cut is a byte count, so the two renders differ only in the tag (T10
`boundary_prompt_hash_stable`). A narrowed enum is a different question and hashes differently.

The assembled user message is capped at 24 KiB; the brief is cut first, then sibling lines are
dropped from the end, then the ancestors' goals are halved — never mid-character, never mid-fence,
each with a visible `[truncated]` marker. The node's own block is never cut; if it alone is over
the cap the attempt closes as `error` (`prompt: … bytes, over 24576 with nothing left to cut`).

## Step 3: ask, with one schema

`ask_structured` (`crates/agent-campaign/src/planner/ask.rs`, the same loop shape as
`Agent::complete_structured`, `crates/agent-runtime/src/agent.rs:1184`) over the Draft-07
`OutputSchema` seam (`crates/agent-core/src/lib.rs:1135`; `Planner::draft07` injects
`agent_validate::Draft07Validator`) with `max_repairs = 2`, `temperature 0`, on the configured
`planner_model` (a large model; decomposition quality is the product, the cheap pool is wrong
here). `response_format` is always set (`campaign_decision`, strict); a schema directive is added
to the system message only when the provider cannot constrain output natively. The body is
refused unparsed over 1 MiB (no repair). Repairs go back as an assistant / user pair carrying the
validator's errors; a failed ask still records the tokens it cost. Schema:

```json
{
  "type": "object",
  "additionalProperties": false,
  "required": ["decision", "reason", "confidence"],
  "properties": {
    "decision":   { "enum": ["execute", "split", "needs_info", "reject"] },
    "reason":     { "type": "string", "maxLength": 600 },
    "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
    "question":   { "type": "string", "maxLength": 600 },
    "acceptance": { "type": "array", "maxItems": 6, "items": { "type": "string", "maxLength": 300 } },
    "touches":    { "type": "array", "maxItems": 12, "items": { "type": "string", "maxLength": 200 } },
    "est_size":   { "enum": ["xs", "s", "m", "l"] },
    "children": {
      "type": "array", "maxItems": 8,
      "items": {
        "type": "object", "additionalProperties": false,
        "required": ["title", "goal", "est_size"],
        "properties": {
          "title":      { "type": "string", "minLength": 1, "maxLength": 120 },
          "goal":       { "type": "string", "minLength": 1, "maxLength": 2000 },
          "acceptance": { "type": "array", "maxItems": 6, "items": { "type": "string", "maxLength": 300 } },
          "touches":    { "type": "array", "maxItems": 12, "items": { "type": "string", "maxLength": 200 } },
          "est_size":   { "enum": ["xs", "s", "m", "l"] },
          "depends_on": { "type": "array", "maxItems": 7, "items": { "type": "integer", "minimum": 1, "maximum": 8 } }
        }
      }
    }
  }
}
```

At `depth ≥ depth_cap − 1` (`depth_cap = clamp(policy.max_depth, 1, 6)`) the `decision` enum is
narrowed to `execute | needs_info | reject` before the call, so a `split` there is a schema
failure, not a policy exception; `≥` rather than `=` so a row somehow deeper than the cap cannot
split either, and the store's own `depth + 1 ≤ max_depth` check stays the backstop. On the root
(`depth = 0`) the enum is `split | needs_info | reject`: roots are never executed (D7). When
`depth_cap = 1` the two rules meet on the root and the root rule wins. The enum in force is also
printed in the node block (`allowed decisions: …`).

## Step 4: validate, fail closed

Schema validity is necessary, not sufficient. In Rust, before any transaction:

| Decision | Required | Otherwise |
|---|---|---|
| `execute` | `acceptance.len() ≥ 1`; `touches.len() ≥ 1` and every entry resolves through the `TouchResolver` seam — v1 `WorktreeTouches`: an exact relative path (≤ 200 chars; no `\`, `:`, `%`, `*`, `?`, `[`, `{`; not absolute), `safe_segment` per segment, `confine(root, path)` succeeds, the target exists and is not a symlink; node keys (`rust:…`) are rejected until RK-08 / CP-07 adds the `RepoGraphStore` resolver; `est_size ∈ {xs, s}`; node is not the root; node has no live children | attempt `error`, node back to `ready`, `attempts + 1` |
| `split` | `1 ≤ children.len() ≤ min(8, max_children − live_children)`; every child screened for injection; `depends_on` entries name ordinals within `1..=children.len()`, not self, acyclic (`agent_core::campaign::check_deps`, Kahn over the child list — the same function the stores run again under the lock); `depth + 1 ≤ max_depth`; `nodes + children ≤ max_nodes` (re-checked in the transaction) | same |
| `needs_info` | `question` present, ≤ 600 chars, screened | same |
| `reject` | `reason` present | same |

Screening runs first, in answer order (`reason`, `question`, `acceptance[i]`, `touches[i]`,
`children[i].title | goal | acceptance[j] | touches[j]`), then the table. Injection *inside the
answer* is the model's fault: the attempt closes as `error` with `injection: <field>: …`, the node
goes back to `ready` and `attempts + 1` — not `blocked`, which is reserved for injected prompt
*inputs* (step 2).

`confidence < 0.4` (or a non-finite value) on `execute` **or** `split` is accepted but recorded as
`detail.low_confidence = true` on the `mark_leaf` / `decompose` event (`LOW_CONFIDENCE` in
`agent_core::campaign`; both so `show` marks a doubtful split as well as a doubtful leaf); v1 has
no automatic gate on it (measure first; see open questions in
[`05-increments.md`](05-increments.md)).

## Step 5: write

| Decision | Store call | Resulting state |
|---|---|---|
| `execute` | `mark_leaf` (protocol (b) variant) | `ready`, or `awaiting_approval` when `depth ∈ approve_levels` |
| `split` | `decompose` (protocol (b)) | node `decomposed`; children `ready` or `awaiting_approval` by level |
| `needs_info` | CAS `decomposing → awaiting_approval`, `detail.question` | waits for `agent campaign answer` |
| `reject` | CAS `decomposing → blocked`, `detail.reason` | waits for `replan` or `cancel` |

`Conflict` on write (someone cancelled or replanned meanwhile) writes **nothing** further: the
attempt row was inside the rolled-back transaction, whoever bumped the version already moved the
node, and a `plan_close(error)` would CAS on the same stale `expected_version` and fail the same
way. The planner logs a warning, counts it in `TickSummary.conflicts`, and the next tick re-reads
the truth. `AlreadyApplied` at the write is reported as skipped. `Invalid | TooLong | Denied` from
the store (its re-check under the lock disagreed, e.g. a `max_nodes` race) close the same attempt
as `error` (the failed transaction rolled its row back). A store error between `plan_start` and
the close is answered with a best-effort `plan_close(error)` before it propagates, so the node is
not left in `decomposing`; a node that still wedges (the process died first) is returned to
`ready` by the driver's `reap_decomposing` (CP-05) once it has sat in `decomposing` for longer
than `DECOMPOSING_MAX_SECS` (900 s) — actor `reaper`, `detail.reason = plan_stale`, no attempt
counted, so the next tick simply plans it again.

## Human interaction points

| Situation | Surfaced by | Human action |
|---|---|---|
| level gate (`approve_levels`) | `agent campaign list --needs-attention` shows `awaiting_approval` rows with their proposed children | `agent campaign approve <id>` or `approve --children <parent>`; `cancel <id>` |
| `needs_info` | same list, with the question | `agent campaign answer <id> "…"` (appended to the goal as a screened `## Clarification` block, node → `ready`) |
| `reject` or attempts exhausted | list shows `blocked` with the reason | `replan <id>` (after editing the goal via `answer`), or `cancel` |
| low confidence | `show <id>` marks it | optional `replan` |

## Cost controls

- Per campaign: `max_plan_tokens` over all `decompose` attempts; `max_worker_tokens_per_leaf`
  passed to the worker session as its budget.
- Per node: `max_plan_attempts`.
- Per tick: `plan_per_tick` calls per tenant.
- Unchanged input makes no call: the `idem_key` includes `prompt_hash`, and the planner scans the
  node's attempts for it before calling (step 1); the store's `AlreadyApplied` at the attempt
  insert is the backstop.

## Worked example: gap SI-4 as a campaign

Root (`1042`, objective, `source_ref = gap:SI-4`): "The model cannot ask structural questions in
bounded slices." Policy defaults. Brief attached. The model answers `split` with three children:

```json
{ "decision": "split", "reason": "needs a seam impl, a tool, and registry wiring", "confidence": 0.8,
  "children": [
    { "title": "PgAst engine behind AstBackend", "goal": "…", "est_size": "m",
      "touches": ["rust:trait:agent_core::AstBackend", "crates/agent-ast/src/lib.rs"] },
    { "title": "repo_graph tool with a question enum", "goal": "…", "est_size": "m",
      "touches": ["rust:trait:agent_core::Tool"], "depends_on": [1] },
    { "title": "register_builtins wiring + component doc", "goal": "…", "est_size": "s",
      "touches": ["rust:fn:agent_runtime::registry::register_builtins", "docs/components/"], "depends_on": [1, 2] }
  ] }
```

Depth 1 is gated: all three are `awaiting_approval`; the human approves. Next tick, node
`1042.1` (est `m`) is asked and answers `split` into `1042.1.1` "Trait impl + read verbs over the
store" (`s`) and `1042.1.2` "Feature flag, tests, doc" (`s`, depends on 1). Node `1042.3` answers
`execute` with two acceptance criteria and its two `touches`; it becomes a leaf, `ready` but
unclaimable until `1042.1` and `1042.2` are `done`. The driver claims `1042.1.1` first
(`ORDER BY campaign_id, path`), then `1042.1.2` once the first PR is merged, and so on; when
`1042.1.1` and `1042.1.2` are `done`, `1042.1` rolls up to `done`, and eventually `1042`.
