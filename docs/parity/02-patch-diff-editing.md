# Parity: patch / diff editing (`apply_patch`)

> Per-feature parity spec for a **unified-diff / patch edit tool**.
> **Status: implemented** — `apply_patch` now ships (`tool-patch` feature,
> [`crates/agent-tools/src/patch.rs`](../../crates/agent-tools/src/patch.rs)),
> with the table-driven tests, a real-tool gRPC roundtrip, an observability
> assertion, and an iai-callgrind bench + dhat leak test described below. The
> remaining follow-ups are noted inline (fuzzy whitespace matching and the
> per-path failure-escalation tracker). This was the headline editing gap: our
> only structured editor was the single-hunk `edit`.

## Feature & why it matters

`edit` (see [tools](../components/tools.md), [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs))
does exactly one thing: replace one unique `old_string` with one `new_string` in
one file. That is safe and cheap, but it forces the model into a chatty,
one-call-per-change loop whenever a single logical change spans several files or
several regions of a file. Each call re-pays the round-trip, and a half-finished
sequence can leave the tree in a state no single call intended.

A patch/diff tool closes that gap. One `apply_patch` call carries a *batch* of
operations — **add** new files, **update** existing files across multiple hunks,
and **delete** files — described in a single unified-diff-style envelope. The
model expresses the whole edit once; the tool applies it as a unit and reports a
per-file summary. This is the standard "V4A" patch surface both peer agents
expose (`*** Begin Patch` … `*** End Patch`), and it is the natural complement to
`edit`: `edit` for a surgical one-liner, `apply_patch` for a coherent multi-file
change.

The value is concentrated in three guarantees `edit` cannot give: **multi-hunk /
multi-file in one call**, **all-or-nothing validation** (a malformed or
non-matching hunk blocks *every* write, so the tree never lands half-patched from
a bad batch), and **context-anchored hunks** (`@@` hints + fuzzy matching) that
survive small drift in the file the model was looking at.

## agent-seddon today

**There is no patch tool.** The closest shipped capabilities are:

- **`edit`** ([`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs))
  — single exact-string replacement, unique-match-or-`replace_all`, one file per
  call. No hunks, no add/delete, no multi-file batch.
- **`write_file`** (`tool-core`) — whole-file overwrite. Can create or replace a
  file, but the model must resend the entire contents and there is no diff,
  no anchoring, and no batching.

Neither offers add+update+delete in one call, atomic batch semantics, or
context-hint matching.

### Where a new `apply_patch` would live

Following the seam/feature/registry pattern in [tools](../components/tools.md)
and [extending.md](../extending.md):

- **Impl:** a new `crates/agent-tools/src/patch.rs`, exporting `ApplyPatchTool`
  (name `"apply_patch"`), implementing `agent_core::Tool` exactly like
  `EditTool` — `schema()` advertising a single `patch` string arg, `execute()`
  parsing the envelope, resolving every target through the shared
  `resolve_within` guard, applying the batch, and returning a capped
  (`truncate`) per-file summary as an `Observation`.
- **Feature:** a new `tool-patch` cargo feature on the `agent-tools` crate
  (sibling to `tool-edit`), so the parser + tool only compile when selected.
- **Registration:** one guarded line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) next to the
  `tool-edit` block:
  ```rust
  #[cfg(feature = "tool-patch")]
  r.tool("apply_patch", |_cfg| {
      Ok(Arc::new(agent_tools::ApplyPatchTool) as Arc<dyn Tool>)
  });
  ```
  Selected by name via `[tools] enabled` (empty ⇒ all registered tools).

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| opencode | `packages/core/src/tool/apply-patch.ts` (+ `packages/core/src/patch.ts` parser) | `packages/core/test/tool-apply-patch.test.ts`, `packages/core/test/patch.test.ts` | `bun:test` + Effect `testEffect` |
| hermes-agent | `tools/patch_parser.py` (`parse_v4a_patch` / `apply_v4a_operations`); `_handle_patch` in `tools/file_tools.py` | `tests/tools/test_patch_parser.py`, `tests/tools/test_patch_failure_tracking.py` | `pytest` (class-grouped) |
| pi | *(no separate patch tool)* — multi-edit lives inside `edit` | `packages/coding-agent/src/core/tools/edit.ts` | — |

**pi** deliberately has no `apply_patch`: its `edit` tool accepts a list of edits
applied to one file in a single call (a multi-edit, not a cross-file unified
diff). So pi contributes prior art for *batched hunks within one file* but not
for the add/delete/multi-file envelope; the cases below are drawn from opencode
and hermes.

Concrete peer cases worth porting:

**opencode — `patch.test.ts` (parser):**
- parses `add` / `update` (with `@@ section` context) / `delete` hunks from one
  envelope;
- strips a `cat <<'EOF' … EOF` heredoc wrapper;
- derives fuzzy line updates while **preserving a BOM** (`﻿`);
- matches **EOF-anchored** chunks (`*** End of File`) from the end of the file;
- **rejects malformed hunk bodies** — an add line without `+` (`Invalid add file
  line`), an update with no `@@` chunk (`expected at least one @@ chunk`), a
  delete with a body (`Invalid patch line`).

**opencode — `tool-apply-patch.test.ts` (tool):**
- registers as the single tool `apply_patch` and **sequentially applies** add,
  update, and delete in one call, emitting `A/M/D` lines + per-file diff stats;
- **rejects moves/renames** (`*** Move to:`) before applying any hunk;
- **all targets resolved & approved before any content is read** (permission
  asserted once for the batch; `readsBeforeEditApproval == 0`);
- rejects an `add` targeting an **existing file**;
- **race**: rejects an `add` whose target *appears during* permission approval;
- rejects an invalid later `update` **before** applying an earlier `add` (no
  partial apply from the prepare phase);
- documents that atomic rollback of the *commit* phase is not yet supported (a
  later commit-time failure leaves earlier applies in place and reports them).

**hermes — `test_patch_parser.py`:**
- `UPDATE` with `@@ context @@` hints (single and multiple hunks);
- `ADD` with only `+` lines — with a context hint inserts at the hint, **without
  a hint appends at EOF**;
- `DELETE` emits a **real unified diff of the removed lines** (`-def old_func():`
  …, `/dev/null`), not a placeholder comment;
- **validation phase**: one invalid hunk ⇒ **nothing is written** (`written ==
  {}`, error says `validation failed`);
- a **pure-context hunk** (only ` ` lines) does not block a later real hunk, but
  a patch of *only* context hunks reports `no changes` and writes nothing;
- validation errors are **hunk-numbered** (`hunk 2`);
- large files: a hunk at line 2200 in a **2500-line file must not truncate** to
  2000 lines, and a >2000-char line survives verbatim;
- non-prefix `|` / `-` characters inside unmodified context lines are preserved.

**hermes — `test_patch_failure_tracking.py`:**
- **per-(task, path) consecutive-failure escalation**: first two failures get a
  normal hint; the **3rd** consecutive failure on the same path injects an
  escalating `_hint` (`failure #3`, `Stop retrying`, mentions the `write_file`
  fallback);
- a **success clears** the counter; different paths and different tasks have
  **independent** counters.

## Completeness gaps

A best-in-class `apply_patch` must guarantee:

1. **One envelope, three op kinds.** Parse `*** Begin Patch` … `*** End Patch`
   with `*** Add File:`, `*** Update File:`, `*** Delete File:` sections; an
   update carries one or more `@@`-anchored hunks of ` `/`-`/`+` lines.
2. **Atomic batch (no partial apply).** Validate *every* op against the current
   tree first — parse OK, add-targets absent, update/delete-targets present,
   every hunk locates its context. If any op fails validation, **write nothing**
   and return an error naming the offending file/hunk. (The commit phase applies
   sequentially; a commit-time I/O failure after validation is reported with the
   list of already-applied files — matching opencode's documented "no rollback
   yet" behaviour.)
3. **Add semantics.** `Add File` must **reject an existing target** (no silent
   clobber) and create parent directories. Trailing-newline normalized.
4. **Delete semantics.** `Delete File` removes an existing file and reports the
   **real removed lines** in the summary diff.
5. **Update / anchoring.** `@@ context @@` hints locate the hunk; **fuzzy**
   whitespace-tolerant matching; **EOF-anchored** hunks match from the end;
   **addition-only** hunks insert at the hint or append at EOF when no hint.
6. **Encoding fidelity.** Preserve a leading **BOM** and the file's existing line
   endings; never truncate long lines or long files.
7. **Reject unsupported ops.** Moves/renames rejected up front (until
   implemented), with a clear message and no writes.
8. **Path safety.** Every target resolved through `resolve_within` (reject `..`
   and absolute escapes), exactly as `edit`/`write_file` do.
9. **Actionable errors.** Malformed-hunk and non-matching-context errors are
   specific and **hunk-numbered**; repeated failures on one path escalate toward
   "re-read the file or fall back to `write_file`".
10. **Bounded output.** Per-file summary capped via `truncate`, like every other
    built-in.

## Table-driven test plan

Proposed target: `#[cfg(test)] mod tests` in the new
[`crates/agent-tools/src/patch.rs`](../../crates/agent-tools/src/patch.rs),
modeled directly on the `edit.rs` table (a `run(dir, args)` helper over a
`tempdir()` from [`agent_testkit`](../../crates/agent-testkit/src/lib.rs), and an
`rstest` case table). Doubles: `agent_testkit::tempdir()` for the filesystem
fixture; no provider/memory doubles are needed (the tool is pure filesystem, like
`EditTool`). Files are seeded before the call and asserted after.

The `Fixture` describes the seed tree; the expectation is `Ok(assert)` (a
post-condition on the tree) or `Err(substr)` (error message must contain
`substr`) — the same shape as `edit_cases`.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolContext;
    use agent_testkit::tempdir;
    use rstest::rstest;
    use serde_json::{json, Value};

    async fn run(dir: &std::path::Path, args: Value) -> Observation {
        ApplyPatchTool
            .execute(args, &ToolContext { cwd: dir.to_path_buf() })
            .await
            .unwrap()
    }

    fn patch(body: &str) -> Value {
        json!({ "patch": format!("*** Begin Patch\n{body}\n*** End Patch") })
    }

    /// `seed`: (path, contents) files to create before the call.
    /// `Ok(())`  ⇒ patch applies; `check` (below) asserts the tree.
    /// `Err(s)` ⇒ error containing `s`, and the seed tree is left untouched.
    #[rstest]
    // ---- happy path: one call, all three op kinds (port: opencode) ----
    #[case::multi_op_add_update_delete(
        &[("update.txt", "before\n"), ("remove.txt", "remove\n")],
        patch("*** Add File: nested/new.txt\n+created\n\
               *** Update File: update.txt\n@@\n-before\n+after\n\
               *** Delete File: remove.txt"),
        Ok(())  // new.txt == "created\n", update.txt == "after\n", remove.txt gone
    )]
    #[case::update_with_context_hint(                       // (port: hermes)
        &[("src/main.py", "def greet():\n    print(\"hello\")\n")],
        patch("*** Update File: src/main.py\n@@ def greet @@\n \
               def greet():\n-    print(\"hello\")\n+    print(\"hi\")"),
        Ok(())  // body now prints "hi"
    )]
    #[case::add_only_hunk_appends_at_eof(                   // (port: hermes)
        &[("app.py", "existing = True\n")],
        patch("*** Update File: app.py\n+def new_func():\n+    return True"),
        Ok(())  // "existing = True" kept; def new_func appended at EOF
    )]
    #[case::add_only_hunk_at_context_hint(                  // (port: hermes)
        &[("app.py", "def main():\n    pass\n")],
        patch("*** Update File: app.py\n@@ def main @@\n+def helper():\n+    return 42"),
        Ok(())  // helper inserted at the `def main` anchor
    )]
    #[case::delete_reports_removed_lines(                   // (port: hermes)
        &[("old.py", "def old_func():\n    return 42\n")],
        patch("*** Delete File: old.py"),
        Ok(())  // file gone; summary contains "-def old_func():"
    )]
    #[case::eof_anchored_chunk(                             // (port: opencode)
        &[("f.txt", "marker\nmiddle\nmarker\nend\n")],
        patch("*** Update File: f.txt\n@@\n-marker\n+marker changed\n \
               end\n*** End of File"),
        Ok(())  // only the trailing "marker" changes
    )]
    #[case::preserves_bom(                                  // (port: opencode)
        &[("bom.txt", "\u{feff}old\n")],
        patch("*** Update File: bom.txt\n@@\n-old\n+new"),
        Ok(())  // result is "\u{feff}new\n" — BOM retained
    )]
    #[case::big_file_not_truncated(                         // (port: hermes)
        &[("big.py", /* 2500 lines, line 2200 == old_value */ "")],
        patch("*** Update File: big.py\n@@ marker_at_2200 @@\n \
               line_2200\n-old_value\n+new_value"),
        Ok(())  // written file still has 2500 lines
    )]
    // ---- atomic batch: one bad op blocks ALL writes (port: hermes/opencode) ----
    #[case::invalid_hunk_writes_nothing(
        &[("a.py", "def good():\n    return 1\n"), ("b.py", "completely different\n")],
        patch("*** Update File: a.py\n@@\n-    return 1\n+    return 2\n\
               *** Update File: b.py\n THIS LINE DOES NOT EXIST\n-old\n+new"),
        Err("validation failed")  // a.py must be UNCHANGED
    )]
    #[case::later_update_missing_blocks_earlier_add(        // (port: opencode)
        &[],
        patch("*** Add File: created.txt\n+created\n\
               *** Update File: missing.txt\n@@\n-before\n+after"),
        Err("missing.txt")  // created.txt must NOT exist afterwards
    )]
    #[case::hunk_number_in_error(                           // (port: hermes)
        &[("a.py", "first = 1\n")],
        patch("*** Update File: a.py\n@@ first @@\n-first = 1\n+first = 2\n\
               @@ missing @@\n-does_not_exist = 1\n+does_not_exist = 2"),
        Err("hunk 2")
    )]
    #[case::only_context_hunks_no_changes(                  // (port: hermes)
        &[("a.py", "anchor\n")],
        patch("*** Update File: a.py\n@@ anchor @@\n anchor"),
        Err("no changes")
    )]
    // ---- rejections (port: opencode) ----
    #[case::add_existing_file_rejected(
        &[("existing.txt", "sentinel\n")],
        patch("*** Add File: existing.txt\n+replacement"),
        Err("exists")  // sentinel preserved
    )]
    #[case::move_rejected(
        &[("old.txt", "before\n")],
        patch("*** Update File: old.txt\n*** Move to: moved.txt\n@@\n-before\n+after"),
        Err("moves are not supported")  // nothing written
    )]
    #[case::malformed_add_line(                             // (port: opencode)
        &[],
        patch("*** Add File: add.txt\nmissing plus"),
        Err("Invalid add file line")
    )]
    #[case::update_without_chunk(                           // (port: opencode)
        &[("update.txt", "x\n")],
        patch("*** Update File: update.txt"),
        Err("at least one @@ chunk")
    )]
    #[case::empty_patch(                                    // (new)
        &[],
        json!({ "patch": "" }),
        Err("empty")
    )]
    // ---- path safety, mirroring edit.rs (new) ----
    #[case::path_escape_rejected(
        &[],
        patch("*** Add File: ../secret\n+x"),
        Err("escape")
    )]
    #[tokio::test]
    async fn apply_patch_cases(
        #[case] seed: &[(&str, &str)],
        #[case] args: Value,
        #[case] expected: std::result::Result<(), &str>,
    ) {
        let dir = tempdir();
        for (path, contents) in seed {
            let full = dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(full, contents).unwrap();
        }
        let obs = run(&dir, args).await;
        match expected {
            Ok(()) => assert!(!obs.is_error, "unexpected error: {}", obs.content),
            Err(substr) => {
                assert!(obs.is_error, "expected error, got ok: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "error `{}` missing `{substr}`",
                    obs.content
                );
            }
        }
    }
}
```

Per-case post-conditions (the "Applied…"/"UNCHANGED" comments above) are asserted
inline in the real test — the table stays readable while each `Ok(())` case still
checks the exact resulting bytes and each `Err` case checks the seed tree is
untouched, exactly as `edit_cases` reads the file back.

A separate small table should cover **per-path failure escalation** (port:
hermes) — three consecutive non-matching `apply_patch` calls on the same path in
one session, asserting the third `Observation` carries the escalating hint
mentioning `write_file`; a success in between resets the counter. This needs a
tiny stateful failure tracker on the tool (or a session-scoped side table); it is
noted here as a follow-up rather than folded into the pure-filesystem table
above.

## References

**agent-seddon**
- [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs) — the
  tool structure + test style this spec mirrors (`schema`, `execute`,
  `resolve_within`, `truncate`, `rstest` table).
- [`crates/agent-tools/src/patch.rs`](../../crates/agent-tools/src/patch.rs) —
  *proposed* home for `ApplyPatchTool` and its tests (does not exist yet).
- [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  — `register_builtins`, where the `tool-patch` factory line is added.
- [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) —
  `tempdir()` fixture used by the test table.
- [`docs/components/tools.md`](../components/tools.md),
  [`docs/extending.md`](../extending.md) — the seam → feature → registry workflow.

**opencode**
- `packages/core/src/tool/apply-patch.ts` — the tool (resolve-all-then-approve,
  sequential apply, reject moves, reject add-over-existing).
- `packages/core/src/patch.ts` — the V4A parser (`Patch.parse` / `Patch.derive` /
  BOM handling).
- `packages/core/test/tool-apply-patch.test.ts` — tool tests (sequential apply,
  move rejection, appear-during-approval race, partial-apply reporting).
- `packages/core/test/patch.test.ts` — parser tests (add/update/delete, heredoc,
  fuzzy+BOM, EOF anchor, malformed rejection).

**hermes-agent**
- `tools/patch_parser.py` — `parse_v4a_patch` / `apply_v4a_operations` (validation
  phase, context hints, addition-only hunks, delete diff, hunk-numbered errors).
- `tools/file_tools.py` — `_handle_patch` (per-(task, path) failure escalation).
- `tests/tools/test_patch_parser.py` — parser + apply tests.
- `tests/tools/test_patch_failure_tracking.py` — consecutive-failure escalation.

**pi**
- `packages/coding-agent/src/core/tools/edit.ts` — multi-edit inside `edit` (no
  separate `apply_patch`; prior art for batched hunks within a single file only).
