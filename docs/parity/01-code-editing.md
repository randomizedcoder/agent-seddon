# Parity spec 01 — code-editing `edit` tool

Per-feature parity spec for the surgical string-replacement tool. Tracks what
agent-seddon ships today, what the peer agents assert, and the concrete behaviour
+ tests needed to be the most complete of the four.

> **Status: implemented (full spec).** `edit`
> ([`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs)) now
> preserves **CRLF + BOM** (an `\n`-only `old_string` matches a CRLF file), reports
> distinct **ENOENT/EACCES** errors, supports **multi-edit** (an `edits` array
> applied atomically against the original, overlap-rejecting), an opt-in
> **exact-first fuzzy fallback** (trailing whitespace / smart quotes / unicode
> dashes / NBSP / fullwidth), and a best-effort **stale-file guard**. Covered by 25
> rstest cases plus a deterministic fuzzy-match bench and the dhat leak test.
> Caveats: fuzzy is **line-oriented** (whole matched lines are replaced) and does
> **not** do full NFC/decomposition folding; the stale guard's TOCTOU window is too
> small to trigger deterministically in a unit test (the positive path is covered by
> every other case); and the EACCES branch isn't asserted under the root-uid nix
> build sandbox. The §5 plan below is the design of record.
>
> **Follow-up — graduated fuzzy chain + perf.** The fuzzy fallback is now a
> **two-level chain** applied in increasing looseness, taking the *first* level that
> locates the block **uniquely** (never loosening past an ambiguity): `Fold`
> (trailing-ws + unicode look-alikes, as before) → `Collapse` (also flattens
> indentation width and interior spacing/tabs), with the replacement **re-indented**
> to the file's matched block so the file's own indentation is preserved. An
> ambiguous fuzzy match now **reports the match count** ("… (N matches); add
> surrounding context") instead of a bare "not unique". Perf: fuzzy normalizes each
> file line **once per level** (was re-normalizing per window position), and
> multi-edit finds each target in a **single scan** (`match_indices`, was
> count-then-find). New rstest cases cover indentation-flexible + interior-whitespace
> matches and the ambiguity-count error; the fuzzy bench ceiling is unchanged.

## Feature & why it matters

The `edit` tool replaces an exact `old_string` with `new_string` in a single file.
It is the workhorse of any coding agent: far cheaper and safer than round-tripping
a whole file through `write_file`, and the uniqueness guard (match must occur once
unless `replace_all`) stops the model from silently mutating the wrong occurrence.

Because the model authors `old_string` from memory of a file it may have read
turns ago, real-world matches drift: line endings differ (CRLF vs LF), a UTF-8 BOM
is invisible, editors substitute smart quotes / unicode dashes, and trailing
whitespace is easy to miss. How forgiving the match is — and how safely the write
is committed — is exactly where the peers diverge, and where our gaps live.

## agent-seddon today

- **Impl:** [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs) (`EditTool`, ~87 lines).
- **Shared helpers:** [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) — `resolve_within` (rejects absolute paths + `..` escapes, lexical only) and `truncate` (12 KB `MAX_OUTPUT` cap on the success message).
- **Tests:** one table-driven `#[rstest]` `edit_cases` in `edit.rs` (~lines 111–173), **7 cases**, plus the `resolve_within`/`truncate`/`arg_*` tables in `lib.rs` that cover the path-safety and arg-extraction seams. Style: a single `initial` file `f.txt`, `Ok(final)` ⇒ file becomes `final`, `Err(substr)` ⇒ error message contains `substr`. Doubles: only `agent_testkit::tempdir()`.

Current coverage (the 7 cases): unique replace, `replace_all`, replace-with-empty,
unicode content, non-unique-without-flag rejection, missing-string rejection,
path-escape rejection. Plus in-impl guards for empty `old_string` and
`old_string == new_string`.

Honest gaps: our `edit` is **exact-bytes only**. It does not normalise or preserve
line endings (CRLF), does not handle a UTF-8 BOM, has no fuzzy matching, no
multi-edit (one replacement per call), no stale-file / concurrent-write detection,
and surfaces read/write errors as a generic `could not read/write` string (no
distinct ENOENT/EACCES signalling). There is also no permission gate — the file
tools rely on `resolve_within` as defense-in-depth, with `bash` as the unconfined
escape hatch by design.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| opencode | `opencode/packages/core/src/tool/edit.ts` | `opencode/packages/core/test/tool-edit.test.ts` | bun:test + Effect |
| pi       | `pi/packages/coding-agent/src/core/tools/edit.ts` | `pi/packages/coding-agent/test/tools.test.ts`, `.../test/edit-tool-legacy-input.test.ts` | vitest |
| hermes   | — (no string `edit`; uses `patch`) | — | — (see parity doc 02) |

**hermes** has no separate string-replace `edit` tool — it edits via a unified-diff
`patch` tool, covered by parity doc 02. It is intentionally absent from this table.

**opencode** asserts (exact-only, but CRLF/BOM-aware + permission-gated + stale-safe):

- Registers as `edit`; replaces relative exact text through one write; emits a diff + replacement count.
- Accepts an absolute path **inside** the active Location; an **external** absolute path requires a separate `external_directory` approval *before* the `edit` approval.
- No-op rejection: `oldString === newString` ⇒ "No changes to apply…".
- Empty rejection: `oldString === ""` ⇒ "oldString must not be empty. Use write…".
- Missing rejection: not found ⇒ "Could not find oldString… must match exactly, including whitespace and indentation."
- Ambiguous rejection: multiple exact matches without `replaceAll` ⇒ "Found multiple exact matches… set replaceAll to true."
- `replaceAll: true` replaces every occurrence (count reported).
- **CRLF + BOM preservation:** matches an LF `oldString` against a `﻿…\r\n…` file and writes back preserving both the BOM and CRLF endings.
- **Stale-file detection:** if the file changes on disk *after* permission approval but *before* the conditional commit ⇒ "File changed after permission approval. Read it again before editing." and no write.
- **Deny reads no content:** a denied `edit` reads zero bytes and returns an identical opaque error whether or not `oldString` matches (no disclosure); denied `external_directory` never reaches the `edit` assertion or a read.

**pi** asserts (fuzzy matching + multi-edit atomicity + errno surfacing):

- Basic exact replace produces a diff + an applicable patch (`applyPatch` round-trips).
- Missing text ⇒ "Could not find the exact text…"; multiple occurrences ⇒ "Found N occurrences".
- **Multi-edit:** an `edits: [{oldText,newText}, …]` array replaces multiple **disjoint** regions in one call; matched against the **original** file (not incrementally); overlapping regions ⇒ "overlap" error; empty `edits` ⇒ "must contain at least one replacement"; **atomic** — if any edit fails to match, none are applied (file unchanged).
- **Errno surfacing:** missing file ⇒ `…Error code: ENOENT.`; read-only file ⇒ `…Error code: EACCES.`; unknown access error ⇒ original message passed through.
- **Fuzzy matching (falls back only when exact fails):** trailing-whitespace-insensitive; smart single/double quotes → ASCII; unicode en/em dashes → ASCII hyphen; non-breaking space → regular space; fullwidth punctuation & compatibility-equivalent / NFC-normalised unicode forms; **prefers exact over fuzzy**; still fails when genuinely absent; detects **duplicates after normalisation** ⇒ "Found 2 occurrences"; works in multi-edit mode; preserves the correct occurrence when a fuzzy replacement equals a nearby line.
- **CRLF/BOM (mirrors opencode):** LF `oldText` matches CRLF content; CRLF and LF endings each preserved; BOM preserved; duplicates detected across CRLF/LF variants.
- Legacy-input folding: top-level `oldText`/`newText` (and stringified `edits`) fold into the `edits` array; the public schema hides the legacy fields.

## Completeness gaps

Behaviour agent-seddon must add/guarantee to be the most complete (spec only — do
**not** implement here):

- **CRLF preservation.** Detect the file's dominant line ending; match an LF `old_string` against CRLF content, and write back in the original ending.
- **UTF-8 BOM preservation.** Strip a leading BOM before matching; re-attach it on write.
- **Distinct errno surfacing.** Report `ENOENT` (missing) and `EACCES` (read-only / permission) distinctly instead of a single opaque `could not read/write`.
- **Multi-edit.** Accept a batch of `{old_string,new_string}` replacements applied against the *original* content, atomically (all-or-nothing), rejecting **overlapping** target regions and an **empty** batch.
- **Fuzzy fallback (opt-in, exact-first).** When exact match fails, retry with a normalising transform (trailing whitespace, smart quotes, unicode dashes, NBSP, fullwidth/NFC forms), preferring exact; re-run the uniqueness check *after* normalisation so post-normalisation duplicates are still rejected.
- **Stale-file / concurrent-write guard.** Compare on-disk content between read and write (or write-if-unchanged) so a race after the model's read cannot clobber newer content.
- **Non-disclosure on failure.** Keep the missing-vs-present error indistinguishable where a permission layer exists (relevant once a `Policy`/permission gate wraps file writes).

These are behavioural targets; each maps to the test cases below.

## Table-driven test plan

Extend the existing `edit_cases` table in
[`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs), matching
its shape: file `f.txt` starts as `initial`; `Ok(final)` ⇒ file becomes `final`,
`Err(substr)` ⇒ error contains `substr`. Only `agent_testkit::tempdir()` is needed
(no LLM/memory doubles — `edit` is a pure filesystem `Tool`). Multi-edit and errno
cases that need a distinct signature (a byte-exact final incl. `\r\n`, or a
chmod-based `EACCES`) go in sibling tables in the same module.

```rust
#[rstest]
// --- already present (keep) -------------------------------------------------
#[case::positive_unique_replace(
    "hello world",
    json!({"path": "f.txt", "old_string": "world", "new_string": "rust"}),
    Ok("hello rust"))] // (port: opencode)
#[case::positive_replace_all(
    "a a a",
    json!({"path": "f.txt", "old_string": "a", "new_string": "b", "replace_all": true}),
    Ok("b b b"))] // (port: opencode|pi)
#[case::corner_replace_with_empty(
    "abc",
    json!({"path": "f.txt", "old_string": "b", "new_string": ""}),
    Ok("ac"))] // (new: agent-seddon)
#[case::corner_unicode(
    "héllo",
    json!({"path": "f.txt", "old_string": "héllo", "new_string": "wörld"}),
    Ok("wörld"))] // (new: agent-seddon)
#[case::negative_non_unique_without_flag(
    "a a a",
    json!({"path": "f.txt", "old_string": "a", "new_string": "b"}),
    Err("not unique"))] // (port: opencode|pi)
#[case::negative_missing_string(
    "hello",
    json!({"path": "f.txt", "old_string": "zzz", "new_string": "x"}),
    Err("not found"))] // (port: opencode|pi)
#[case::negative_path_escape(
    "hello",
    json!({"path": "../secret", "old_string": "a", "new_string": "b"}),
    Err("escape"))] // (new: agent-seddon)
// --- add: identity / empty guards (already enforced in impl, pin them) -------
#[case::negative_noop_identical(
    "same same",
    json!({"path": "f.txt", "old_string": "same", "new_string": "same"}),
    Err("identical"))] // (port: opencode)
#[case::negative_empty_old_string(
    "content",
    json!({"path": "f.txt", "old_string": "", "new_string": "x"}),
    Err("must not be empty"))] // (port: opencode)
// --- add: CRLF preservation -------------------------------------------------
#[case::boundary_crlf_lf_oldstring_matches(
    "first\r\nsecond\r\nthird\r\n",
    json!({"path": "f.txt", "old_string": "second", "new_string": "REPLACED"}),
    Ok("first\r\nREPLACED\r\nthird\r\n"))] // (port: opencode|pi)
#[case::boundary_lf_preserved_for_lf_file(
    "first\nsecond\nthird\n",
    json!({"path": "f.txt", "old_string": "second", "new_string": "REPLACED"}),
    Ok("first\nREPLACED\nthird\n"))] // (port: pi)
// --- add: fuzzy fallback (exact-first) --------------------------------------
#[case::corner_fuzzy_trailing_ws(
    "line one   \nline two\n",
    json!({"path": "f.txt", "old_string": "line one\nline two", "new_string": "replaced"}),
    Ok("replaced\n"))] // (port: pi)
#[case::corner_fuzzy_smart_quotes(
    "console.log(\u{2018}hello\u{2019});\n",
    json!({"path": "f.txt", "old_string": "console.log('hello');", "new_string": "console.log('world');"}),
    Ok("console.log('world');\n"))] // (port: pi)
#[case::corner_fuzzy_unicode_dash(
    "range: 1\u{2013}5\n",
    json!({"path": "f.txt", "old_string": "range: 1-5", "new_string": "range: 10-50"}),
    Ok("range: 10-50\n"))] // (port: pi)
#[case::corner_fuzzy_nbsp(
    "hello\u{00A0}world\n",
    json!({"path": "f.txt", "old_string": "hello world", "new_string": "hello universe"}),
    Ok("hello universe\n"))] // (port: pi)
#[case::positive_exact_preferred_over_fuzzy(
    "const x = 'exact';\nconst y = 'other';\n",
    json!({"path": "f.txt", "old_string": "const x = 'exact';", "new_string": "const x = 'changed';"}),
    Ok("const x = 'changed';\nconst y = 'other';\n"))] // (port: pi)
#[case::negative_fuzzy_still_absent(
    "completely different\n",
    json!({"path": "f.txt", "old_string": "this does not exist", "new_string": "x"}),
    Err("not found"))] // (port: pi)
#[case::negative_dup_after_normalization(
    "hello world   \nhello world\n",
    json!({"path": "f.txt", "old_string": "hello world", "new_string": "replaced"}),
    Err("not unique"))] // (port: pi)
#[tokio::test]
async fn edit_cases(
    #[case] initial: &str,
    #[case] args: Value,
    #[case] expected: std::result::Result<&str, &str>,
) { /* existing harness: write f.txt, run, assert on file/error */ }
```

Sibling tables in the same `mod tests` (distinct signatures, same `tempdir()`
double):

```rust
// --- BOM preservation: read back exact bytes incl. BOM -----------------------
#[rstest]
#[case::boundary_bom_crlf_preserved(
    "\u{FEFF}first\r\nsecond\r\nthird\r\n",
    "second", "REPLACED",
    "\u{FEFF}first\r\nREPLACED\r\nthird\r\n")] // (port: opencode|pi)
fn edit_preserves_bom(/* write, run, assert byte-exact read_to_string */) {}

// --- multi-edit: disjoint / atomic / overlap / empty -------------------------
#[rstest]
#[case::positive_multi_disjoint(
    "alpha\nbeta\ngamma\ndelta\n",
    json!({"path": "f.txt", "edits": [
        {"old_string": "alpha", "new_string": "ALPHA"},
        {"old_string": "gamma", "new_string": "GAMMA"}]}),
    Ok("ALPHA\nbeta\nGAMMA\ndelta\n"))] // (port: pi)
#[case::corner_multi_matches_original_not_incremental(
    "foo\nbar\nbaz\n",
    json!({"path": "f.txt", "edits": [
        {"old_string": "foo", "new_string": "foo bar"},
        {"old_string": "bar", "new_string": "BAR"}]}),
    Ok("foo bar\nBAR\nbaz\n"))] // (port: pi)
#[case::negative_multi_overlap(
    "one\ntwo\nthree\n",
    json!({"path": "f.txt", "edits": [
        {"old_string": "one\ntwo", "new_string": "X"},
        {"old_string": "two\nthree", "new_string": "Y"}]}),
    Err("overlap"))] // (port: pi)
#[case::negative_multi_empty(
    "hello\n",
    json!({"path": "f.txt", "edits": []}),
    Err("at least one"))] // (port: pi)
#[case::negative_multi_atomic_no_partial(
    "alpha\nbeta\n",
    json!({"path": "f.txt", "edits": [
        {"old_string": "alpha", "new_string": "ALPHA"},
        {"old_string": "missing", "new_string": "X"}]}),
    Err("not found"))] // file must stay "alpha\nbeta\n" // (port: pi)
fn edit_multi_cases(/* … */) {}

// --- errno surfacing: needs chmod / a missing path ---------------------------
#[rstest]
#[case::negative_enoent("nope.txt", "ENOENT")]      // (port: pi)
#[case::negative_eacces_readonly("ro.txt", "EACCES")] // chmod 0o444 // (port: pi)
fn edit_surfaces_errno(/* … */) {}
```

Case-prefix key: `positive_` succeeds, `negative_` rejects, `corner_` odd-but-valid
input (fuzzy/unicode), `boundary_` line-ending/BOM edges. `(port: …)` names the
peer the case came from; `(new: agent-seddon)` marks cases with no peer origin
(already in our table).

## References

- **agent-seddon:** [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs), [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) (`resolve_within`, `truncate`, `MAX_OUTPUT`), [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`).
- **opencode:** `opencode/packages/core/src/tool/edit.ts`, `opencode/packages/core/test/tool-edit.test.ts`.
- **pi:** `pi/packages/coding-agent/src/core/tools/edit.ts`, `pi/packages/coding-agent/test/tools.test.ts`, `pi/packages/coding-agent/test/edit-tool-legacy-input.test.ts`.
- **hermes:** no string `edit` — unified-diff `patch` tool instead; see parity doc [`02-patch-diff-editing.md`](02-patch-diff-editing.md).
