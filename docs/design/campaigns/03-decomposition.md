# 03 — Decomposition: the recursive "is this small enough?" loop

The planner is a bounded step inside the driver tick ([`04-executor.md`](04-executor.md)). Per
tenant it selects up to `plan_per_tick` nodes with `state = 'ready' AND kind <> 'leaf'` (index
`tasks_plan`), oldest campaign first, shallowest first, and asks the model one question per node.
Children created by a `split` are asked on later ticks. The loop terminates because every answer
either marks a leaf, blocks, waits for a human, or adds children one level deeper under
`max_depth`, `max_children` and `max_nodes`.

## Step 1: take the node

`ready → decomposing` by CAS, recording `expected_version` (the version after the CAS). A node
whose `attempts >= policy.max_plan_attempts` is moved to `blocked` instead, with
`detail.reason = 'attempts_exhausted'`. A node whose campaign has spent
`SUM(tokens_in + tokens_out) FROM task_attempts WHERE campaign_id = …` at or above
`max_plan_tokens` is moved to `blocked` with `detail.reason = 'token_cap'`, without a provider
call. A `task_attempts` row is inserted `pending` with `idem_key = sha256(tenant, task_id,
expected_version, prompt_hash)`.

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
   ≤ 6 KiB, L0 + L1). Until RK-12 lands, the fallback is the first 6 KiB of
   `docs/architecture.md` followed by the "Conventions" and "Security" sections of `CLAUDE.md`.
5. **The rules** (fixed Rust text): what `execute` means (D7), the size vocabulary, that
   `touches` must be exact keys or paths, that `depends_on` lists sibling ordinals, that at most
   8 children are allowed, that the answer is JSON matching the schema and nothing else.

Every model-written field (goal, title, acceptance, question, and the ancestors' and siblings'
goals) passes `scan_for_injection` (`crates/agent-core/src/security.rs:97`) before it enters the
prompt. A hit anywhere blocks the node with `detail.reason = 'injection'` and
`detail.field`, closes the attempt as `error`, and makes no provider call. The brief is already
screened by its own track.

The assembled prompt is capped at 24 KiB; the brief is truncated first, then siblings, then
ancestors' goals, never mid-fence, with a visible `[truncated]` marker.

## Step 3: ask, with one schema

`Agent::complete_structured` (`crates/agent-runtime/src/agent.rs:1163`) over the Draft-07
`OutputSchema` seam (`crates/agent-core/src/lib.rs:1121`) with `max_repairs = 2`, on the
configured `planner_model` (a large model; decomposition quality is the product, the cheap pool is
wrong here). Schema:

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

At `depth = max_depth − 1` the `decision` enum is narrowed to `execute | needs_info | reject`
before the call, so a `split` there is a schema failure, not a policy exception. On the root
(`depth = 0`) the enum is `split | needs_info | reject`: roots are never executed (D7).

## Step 4: validate, fail closed

Schema validity is necessary, not sufficient. In Rust, before any transaction:

| Decision | Required | Otherwise |
|---|---|---|
| `execute` | `acceptance.len() ≥ 1`; `touches.len() ≥ 1` and every entry resolves (a `node_key` known to the repo-knowledge store, or a path that exists in a checkout at the default branch and passes `safe_segment` per segment); `est_size ∈ {xs, s}`; node is not the root; node has no children | attempt `error`, node back to `ready`, `attempts + 1` |
| `split` | `1 ≤ children.len() ≤ min(8, max_children − live_children)`; every child screened for injection; `depends_on` entries name ordinals within `1..=children.len()`, not self, acyclic (Kahn over the child list); `depth + 1 ≤ max_depth`; `nodes + children ≤ max_nodes` (re-checked in the transaction) | same |
| `needs_info` | `question` present, ≤ 600 chars, screened | same |
| `reject` | `reason` present | same |

`confidence < 0.4` on `execute` is accepted but recorded as `detail.low_confidence = true`; v1
has no automatic gate on it (measure first; see open questions in
[`05-increments.md`](05-increments.md)).

## Step 5: write

| Decision | Store call | Resulting state |
|---|---|---|
| `execute` | `mark_leaf` (protocol (b) variant) | `ready`, or `awaiting_approval` when `depth ∈ approve_levels` |
| `split` | `decompose` (protocol (b)) | node `decomposed`; children `ready` or `awaiting_approval` by level |
| `needs_info` | CAS `decomposing → awaiting_approval`, `detail.question` | waits for `agent campaign answer` |
| `reject` | CAS `decomposing → blocked`, `detail.reason` | waits for `replan` or `cancel` |

`Conflict` on write (someone cancelled or replanned meanwhile) closes the attempt as `error` with
`detail.reason = 'conflict'` and does not retry: the next tick re-reads the truth.

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
- Unchanged input makes no call: the `idem_key` includes `prompt_hash`, so a retried tick against
  an unchanged node with an unfinished attempt is `AlreadyApplied` at the attempt insert.

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
