# Skill authoring

The agent writes its own reusable procedures. Parity spec
[30](../parity/30-skill-authoring.md), closing the loop on spec
[07](../parity/07-skills.md)'s read-only skills.

An agent that solves a hard task once can capture the procedure as a `SKILL.md`,
and every later run pays only the discovery cost to replay it. Procedural
knowledge that compounds is the highest-leverage extension point in the system —
a skill is just a file, so a good one authored once is available forever with no
code, no recompile, no plugin registration.

## Why this is security-sensitive

A skill the agent writes today is **read straight back into a future system
prompt tomorrow**. A poisoned body is not a one-turn problem — it is a
persistent, cross-session foothold. This is exactly the memory-poisoning threat
model from spec 10, moved onto the procedural store.

So the write is guarded five ways:

| Guard | Why |
|---|---|
| **Name is a safe segment** | it becomes a directory; `[A-Za-z0-9._-]`, no `..`, separators, or leading `-` |
| **Body is injection-scanned** | refuses to persist a body that would hijack future prompts |
| **Description is scanned too** | it appears in every skill menu |
| **No silent overwrite** | an existing skill needs an explicit `overwrite: true` |
| **Policy-gated** | it is a side-effecting tool like any other |

The scan prefers the [`Scanner`](scanner.md) seam when wired and falls back to
`agent_core::scan_for_injection`, so the guard holds either way.

Fields are collapsed to a single line before rendering, so a newline in a
description cannot forge extra frontmatter keys (`author`, `version`) — the test
asserts no forged *key* appears, not that a substring is absent.

## Off by default

```toml
[skills]
write     = false   # enable the `skill_write` tool
write_dir = ""      # empty ⇒ <working_dir>/.agent/skills
```

Authoring is a privileged, persistent action, so the tool is not registered
unless an operator opts in.

## Provenance and versioning

Every authored skill records who wrote it and which revision it is:

```yaml
---
name: cut-release
description: Run the release checklist
author: agent
version: 2
---
```

`author: agent` lets a later reader — human or agent — tell a machine-authored
skill from a human-authored one, which matters if you ever auto-curate. An
overwrite **bumps the version** rather than silently losing the edit history.

There is deliberately **no timestamp**: it would make the file nondeterministic
for no information the version doesn't already carry.

## Limits

| Cap | Value | Why |
|---|---|---|
| `SKILL.md` size | 32 KiB | a skill is loaded into a system prompt; unbounded is a context-window DoS |
| Name length | 64 chars | it is a directory segment |

Over the size cap the error suggests splitting detail into supporting files and
keeping `SKILL.md` a summary — which is how progressive disclosure is supposed to
work anyway.

## The round trip

The point is that authoring and discovery meet: an end-to-end test has the agent
call `skill_write`, then runs spec 07's `discover` and asserts the new skill is
found and its body loads. Without that, the feature would be decorative.

## Deferred

- **`edit` / `patch` actions.** hermes ships fuzzy find-replace within an
  existing skill; here an update is a full rewrite via `overwrite: true`.
- **Supporting files.** A skill is a single `SKILL.md`; hermes allows a directory
  of auxiliary files the skill references.
- **Auto-curation.** Provenance is recorded so a future curator can distinguish
  agent-authored skills, but nothing curates yet.
