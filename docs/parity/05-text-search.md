# Parity 05 — text search (`grep` / `find` / `ls`)

Per-feature parity spec for the gitignore-aware text-search tools. Tracks what
agent-seddon ships today against what peer coding agents test, and lays out a
table-driven test plan to close the gaps. Scope is the `tool-search` trio only;
the index-backed `search` tool (tantivy, the `SearchBackend` seam) is covered by
its own spec.

## 1. Feature & why it matters

`grep`, `find`, and `ls` are the model's eyes on an unfamiliar tree. Almost every
task starts with "where does X live?" — so their behavior on the tricky cases
(gitignored files, hidden directories, flag-like patterns, big result sets)
directly shapes what the agent can see and how much junk it wastes context on.

Two properties matter beyond "does it match":

- **Gitignore/hidden awareness.** Skipping `.git/`, `node_modules/`, and
  `.gitignore`d paths keeps output relevant and — as hermes-agent's `#1558`
  regression shows — is a real **prompt-injection boundary**: a 3.5 MB cached
  catalog under a hidden dir carried adversarial text the model then obeyed. What
  the search tools *don't* surface is a safety property, not just ergonomics.
- **Bounded, literal, side-effect-free.** A pattern is *data*, never a shell
  flag or a command. Peers explicitly test that `--pre=…` / `--help` are treated
  as literal search text with no injection, and that huge result sets truncate
  with a clear marker rather than blowing the context window.

## 2. agent-seddon today

The trio lives in
[`crates/agent-tools/src/search.rs`](../../crates/agent-tools/src/search.rs) and
walks the tree with ripgrep's [`ignore`](https://docs.rs/ignore) crate, so
`.gitignore` and hidden files are skipped by default — the same walker ripgrep
uses. The blocking walk runs on `spawn_blocking`. Path safety and output capping
come from the shared helpers in
[`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs).

| Tool   | Args                                 | Output shape                                  |
| ------ | ------------------------------------ | --------------------------------------------- |
| `grep` | `pattern`, `path?`, `case_insensitive?` | `path:line:text` per match                 |
| `find` | `pattern` (regex vs. rel path), `path?` | matching relative paths                     |
| `ls`   | `path?`, `recursive?`                 | names (dirs suffixed `/`); recursive walks tree |

Behavior worth pinning down:

- **Regex, not glob.** Both `grep` and `find` compile the `pattern` with
  `RegexBuilder`; `find` matches the regex against each *relative path*. Peers
  (pi, opencode) take a **glob** for find — a deliberate divergence to document,
  not silently "fix".
- **Bounded output.** `grep`/`find`/`ls --recursive` stop at `MAX_HITS = 300`
  and append a truncation marker (`...[more matches truncated]` /
  `...[truncated]`); the final text is additionally capped at `MAX_OUTPUT`
  (~12 KB) by `truncate`.
- **Errors are `Observation::error`, not `Err`.** Invalid regex →
  `"invalid regex: …"`; a path that escapes the working dir →
  `resolve_within`'s `"path escapes the working directory"`. The model sees the
  outcome and can retry.
- **Empty results** read `(no matches)` (grep/find) or `(empty)` (ls).

Existing tests (in the `#[cfg(test)] mod tests` block of `search.rs`, ~14 cases
across 5 groups; doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs)):

| Group               | Cases | Covers                                                        |
| ------------------- | ----- | ------------------------------------------------------------ |
| `rel_cases`         | 3     | relative-path formatting: inside / cwd-itself / outside      |
| `resolve_root_cases`| 4     | default `.`, subdir, `../..` escape, absolute reject         |
| `grep_cases`        | 4     | line-numbered matches, no-match, invalid regex, path escape  |
| `find_cases`        | 3     | `\.rs$`, match-all `.`, no-match                             |
| `ls_cases`          | 1     | dirs get a trailing `/`                                       |

**Not yet covered** (the target of §5): gitignore is respected by the walk but
never *asserted*; hidden files/dirs are skipped but unasserted; no
`case_insensitive` case; no flag-like-pattern (injection) case; no truncation /
`MAX_HITS` case; no `ls recursive` case; no binary/unreadable-file skip; no
`find` glob-vs-regex divergence note pinned by a test.

## 3. Peer implementations & their tests

| Peer         | Impl path                                                    | Test path                                                                    | Framework          |
| ------------ | ----------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------ |
| pi           | `pi/packages/coding-agent/src/core/tools/{grep,find,ls}.ts` | `pi/packages/coding-agent/test/tools.test.ts`                                | vitest             |
| hermes-agent | `hermes-agent/tools/file_operations.py` (`ShellFileOperations`) | `hermes-agent/tests/tools/test_search_{budget_truncation,hidden_dirs,error_guard}.py` | pytest (parametrize) |
| opencode     | `opencode/packages/core/src/{ripgrep,filesystem/ignore}.ts` | `opencode/packages/core/test/filesystem/{search,ignore}.test.ts`            | bun:test           |

**pi** (`describe("grep"/"find"/"ls")`):

- grep — *single-file search includes the filename*: `grep({pattern:"match", path:file})` yields `example.txt:2: match line`.
- grep — *respects a global limit and emits context lines*: `{limit:1, context:1}` returns the `before`/`match`/`after` triad plus `[1 matches limit reached. Use limit=2 for more, or refine pattern]`, and the second match is absent.
- grep — *flag-like patterns are literal, no injection*: `pattern:"--pre=<script>"` returns "No matches found" and the script's side-effect marker file is never created.
- find — *includes hidden files that aren't gitignored*: `.secret/hidden.txt` and `visible.txt` both returned for `**/*.txt`.
- find — *respects `.gitignore`*: `ignored.txt` in `.gitignore` is excluded; `kept.txt` present.
- find — *surfaces glob parse errors*: `pattern:"["` rejects with an `error parsing glob` / `fd exited` message.
- find — *flag-like patterns literal*: `pattern:"--help"` → "No files found matching pattern".
- ls — *lists dotfiles and directories*: `.hidden-file` and `.hidden-dir/` both shown.

**hermes-agent**:

- `test_search_hidden_dirs.py` — `find`/`grep` must skip hidden dirs like ripgrep does by default: `.hub/index-cache/catalog.json` and `.git/objects/pack-abc.idx` never appear, but visible `SKILL.md` still does. (This is the `#1558` prompt-injection regression.)
- `test_search_budget_truncation.py` — on timeout the search returns *partial* results with `truncated is True` and `limit_reason == "search_timeout"`, and the trailing `[Command timed out…]` marker is stripped from the payload (not parsed as a fake hit).
- `test_search_error_guard.py` — invalid regex (`"["`) hard-fails with `"Search failed"` and no matches; a *partial* error (one unreadable file amid matches) keeps the matches and does **not** surface an error; truncation via `head` (SIGPIPE) is not mistaken for an error; the default search limit is `50`.

**opencode**:

- `search.test.ts` — *glob with a limit*: `glob({pattern:"**/*.ts", limit:10})` returns `["src/match.ts"]`.
- `search.test.ts` — *grep with include filtering*: `grep({pattern:"needle", include:"*.ts", limit:10})` matches `src/match.ts` and skips `src/skip.txt`; the submatch text is `"needle"`.
- `ignore.test.ts` — nested and non-nested ignore rules all match: `node_modules`, `node_modules/`, `node_modules/bar`, `node_modules/bar/`.

## 4. Completeness gaps

Ranked by how much they'd bite an agent in practice.

1. **Gitignore respected but never asserted.** The `ignore` crate gives it to us,
   but nothing pins it — a refactor to a plain `WalkDir` would pass CI. Peers
   (pi, hermes, opencode) all test this directly. **(gap → port)**
2. **Hidden-file/dir handling unasserted.** `.git/`, `node_modules/`, and dot-dirs
   are skipped by the walk; that's the hermes `#1558` injection boundary and pi's
   ls-dotfiles case. No test pins either the skip *or* the divergence that our
   `ls` (non-recursive) uses `read_dir` and *does* list dotfiles. **(gap → port)**
3. **Flag-like pattern / injection.** No test proves `--pre=…` / `--help` are
   literal regex text. Ours can't shell out (pure `regex` crate, no subprocess),
   so injection is structurally impossible — but a regression test documents the
   guarantee cheaply. **(gap → port)**
4. **Truncation / `MAX_HITS`.** The 300-hit cap and its marker are untested; a
   changed constant or a dropped marker would go unnoticed. **(gap → new)**
5. **`case_insensitive` grep.** The flag exists and is unexercised. **(gap → new)**
6. **`ls recursive`.** The recursive branch (trailing-`/` on dirs, `(empty)`,
   truncation) has no case. **(gap → new)**
7. **Binary / unreadable files skipped.** `grep_walk` swallows non-UTF-8 reads;
   worth pinning so a match-in-binary never leaks. **(gap → new)**
8. **`find` is regex-over-path, peers glob.** A deliberate divergence — pin it
   with a case so it's a documented decision, not a latent surprise. **(gap → new)**

Out of scope / intentional non-parity: **timeout-based partial results with a
`limit_reason`** (hermes) — our walk is in-process and bounded by hit count, not
a wall-clock timeout, so there's no timeout payload to strip; **`include`/glob
filtering** (opencode grep) — not a current arg; note as a possible future arg,
don't test absent behavior.

## 5. Table-driven test plan

Extends the existing `#[cfg(test)] mod tests` in
[`crates/agent-tools/src/search.rs`](../../crates/agent-tools/src/search.rs).
Reuse the file's own `ctx()` helper and `agent_testkit::tempdir()`; several cases
need a richer fixture than the current `fixture()`, so add a `fixture_ignore()`
that writes a `.gitignore`, a hidden dir, and a binary file alongside the plain
files. Case naming follows the house `positive_ / negative_ / boundary_ /
corner_` prefixes. Tags: **(port: `<peer>`)** mirrors a peer case;
**(new)** is an agent-seddon-specific guarantee.

```rust
// Add near the existing `fixture()` in search.rs.
//
// Layout:
//   a.txt            "foo\nbar\nFOO"      (mixed case, for case_insensitive)
//   keep.txt         "needle"
//   ignored.txt      "needle"             (matched by .gitignore)
//   .gitignore       "ignored.txt\n"
//   .secret/h.txt    "needle"             (hidden dir)
//   bin.dat          <invalid UTF-8>      (binary; grep must skip)
fn fixture_ignore() -> PathBuf {
    let dir = tempdir();
    std::fs::write(dir.join("a.txt"), "foo\nbar\nFOO").unwrap();
    std::fs::write(dir.join("keep.txt"), "needle").unwrap();
    std::fs::write(dir.join("ignored.txt"), "needle").unwrap();
    std::fs::write(dir.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::create_dir_all(dir.join(".secret")).unwrap();
    std::fs::write(dir.join(".secret/h.txt"), "needle").unwrap();
    std::fs::write(dir.join("bin.dat"), [0xff, 0xfe, 0x00, 0x9f]).unwrap();
    dir
}

// --- grep: gitignore + hidden + case + injection + binary --------------
// `present` substrings must appear; `absent` must not.
#[rstest]
#[case::positive_ci_matches_mixed_case(
    "foo", json!({"case_insensitive": true}),
    vec!["a.txt:1:foo", "a.txt:3:FOO"], vec![])]                       // (new)
#[case::boundary_cs_default_skips_uppercase(
    "foo", json!({}), vec!["a.txt:1:foo"], vec!["a.txt:3:FOO"])]       // (new)
#[case::negative_gitignored_not_searched(
    "needle", json!({}), vec!["keep.txt"], vec!["ignored.txt"])]      // (port: pi/hermes)
#[case::negative_hidden_dir_not_searched(
    "needle", json!({}), vec!["keep.txt"], vec![".secret"])]         // (port: hermes #1558)
#[case::corner_flag_like_pattern_is_literal(
    "--pre=/x", json!({}), vec!["(no matches)"], vec![])]            // (port: pi injection)
#[case::corner_binary_file_skipped(
    "needle", json!({}), vec!["keep.txt"], vec!["bin.dat"])]         // (new)
#[tokio::test]
async fn grep_gitignore_cases(
    #[case] pattern: &str,
    #[case] extra: Value,
    #[case] present: Vec<&str>,
    #[case] absent: Vec<&str>,
) { /* build args like grep_cases; assert !is_error, contains/!contains */ }

// --- grep: MAX_HITS truncation marker ---------------------------------
#[tokio::test]                                                          // (new)
async fn grep_truncates_at_max_hits() {
    // Write one file with MAX_HITS + 50 matching lines; assert the output
    // contains "...[more matches truncated]" and !is_error.
}

// --- find: gitignore + hidden + regex-not-glob + invalid regex --------
#[rstest]
#[case::negative_gitignored_excluded(
    "needle|keep", vec!["keep.txt"], vec!["ignored.txt"])]           // (port: pi/opencode ignore)
#[case::positive_hidden_not_gitignored_included(
    "h\\.txt", vec![".secret/h.txt"], vec![])]                        // (port: pi hidden-files)
#[case::corner_pattern_is_regex_not_glob(
    "\\.txt$", vec!["a.txt", "keep.txt"], vec![])]                    // (new: divergence)
#[tokio::test]
async fn find_gitignore_cases(
    #[case] pattern: &str,
    #[case] present: Vec<&str>,
    #[case] absent: Vec<&str>,
) { /* like find_cases but over fixture_ignore() */ }

#[rstest]
#[case::negative_invalid_regex("(", "invalid regex")]                 // (port: pi/hermes glob-error)
#[case::negative_path_escape("x", "escape")]                          // (new: path safety)
#[tokio::test]
async fn find_error_cases(#[case] pattern: &str, #[case] needle: &str) {
    // For path_escape, pass json!({"path": "../.."}); assert is_error + contains.
}

// --- ls: dotfiles, dirs, recursive, empty -----------------------------
#[rstest]
#[case::positive_lists_dotfiles(json!({}), vec![".gitignore", ".secret/"], vec![])]  // (port: pi ls-dotfiles)
#[case::positive_recursive_walks_tree(
    json!({"recursive": true}), vec!["keep.txt", ".secret/"], vec!["ignored.txt"])]   // (new: recursive respects .gitignore)
#[tokio::test]
async fn ls_cases_ext(
    #[case] args: Value,
    #[case] present: Vec<&str>,
    #[case] absent: Vec<&str>,
) { /* LsTool.execute over fixture_ignore(); assert contains/!contains */ }

#[tokio::test]                                                          // (new)
async fn ls_empty_dir_reports_empty() {
    // An empty tempdir(); assert content == "(empty)".
}
```

Notes on a couple of expectations:

- **`ls` lists dotfiles by design.** The non-recursive branch uses `read_dir`, so
  `.gitignore` / `.secret/` show up (matching pi). The **recursive** branch uses
  the `ignore` walker, so it *does* honor `.gitignore` — hence `ignored.txt` is
  absent in `positive_recursive_walks_tree` but `.secret/` (hidden, not
  gitignored) is present. Pinning both branches documents the split.
- **No subprocess ⇒ injection is structural.** `corner_flag_like_pattern_is_literal`
  asserts the outcome (no match, no error), not a side-effect marker file like
  pi's — we have no shell to fire, which is the stronger guarantee.

## 6. References

- Impl: [`crates/agent-tools/src/search.rs`](../../crates/agent-tools/src/search.rs) · shared helpers [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) (`resolve_within`, `truncate`, `MAX_OUTPUT`)
- Doubles / `tempdir`: [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
- Component doc: [`docs/components/tools.md`](../components/tools.md) · index-backed search seam [`docs/components/search.md`](../components/search.md)
- Peers: `pi/packages/coding-agent/test/tools.test.ts` · `hermes-agent/tests/tools/test_search_{budget_truncation,hidden_dirs,error_guard}.py` · `opencode/packages/core/test/filesystem/{search,ignore}.test.ts`
- Walker: ripgrep [`ignore`](https://docs.rs/ignore) crate (gitignore + hidden-file semantics inherited by all three tools)
