# Parity: `read_file` / `write_file`

Per-feature parity spec for the always-shipped file I/O pair — `read_file` and
`write_file` (the `tool-core` feature, alongside `bash`). The point of this spec
is section 5: these two tools have **zero direct table-driven tests today**, and
their peers guard behaviour we do not.

## 1. Feature & why it matters

`read_file` returns the UTF-8 text of a file relative to the working directory;
`write_file` creates (or overwrites) one. They are the model's hands on the
filesystem — the most-invoked tools after `bash`, and the ones most exposed to
adversarial paths (`..` escapes, absolute paths, secrets, binary blobs). A bug
here is not a wrong answer; it is a path traversal or an oversized blob blowing
the context window. Every peer treats these as security-sensitive surfaces and
tests them heavily; agent-seddon ships them with only indirect coverage.

## 2. agent-seddon today

Both tools live in
[`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs),
behind the shared helpers in
[`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs).

- **`read_file`** (`ReadFileTool`): `resolve_within(cwd, path)` → on reject,
  returns `Observation::error` (model-visible, not a hard error) → else
  `tokio::fs::read_to_string` → `truncate(...)`. `read_to_string` means
  **UTF-8 only**: a non-UTF-8 file surfaces as `could not read \`{path}\`: {e}`
  (a model-visible error, but with an opaque OS message, not a typed "binary
  file" signal). No pagination, no offset/limit, no line numbers, no MIME/image
  handling.
- **`write_file`** (`WriteFileTool`): `resolve_within` → `create_dir_all(parent)`
  (parent dirs are created automatically) → `tokio::fs::write` → reports
  `wrote {n} bytes to \`{path}\``. Overwrites unconditionally; no deny-list, no
  BOM handling, no "created vs overwrote" distinction, no atomic temp-swap.
- **Shared safety** ([`lib.rs`](../../crates/agent-tools/src/lib.rs)):
  `resolve_within` rejects absolute paths and any `..` traversal that escapes
  `cwd` (lexical, does not follow symlinks — `bash` is the intentional escape
  hatch). `truncate` caps output at `MAX_OUTPUT = 12_000` bytes on a UTF-8 char
  boundary, appending `\n...[output truncated]`.

**Test coverage today:** `resolve_within` and `truncate` are table-tested in
[`lib.rs`](../../crates/agent-tools/src/lib.rs) `mod tests`, but **`ReadFileTool`
and `WriteFileTool` themselves have no `execute()` tests at all** — they are only
exercised transitively (e.g. through loop/registry integration tests). There is
no test that reading a file returns its bytes, that a missing file yields a
model-visible error, that a write creates a parent directory, that the output
cap actually fires through `read_file`, or that a non-UTF-8 file is rejected. The
sibling [`edit.rs`](../../crates/agent-tools/src/edit.rs) shows exactly the
`tempdir()` + `#[rstest]` pattern these two are missing.

## 3. Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| opencode | `packages/core/src/tool/{read,read-filesystem,write}.ts` | `packages/core/test/tool-read.test.ts`, `tool-read-filesystem.test.ts`, `tool-write.test.ts` | `bun:test` + Effect layers |
| pi | `packages/coding-agent/src/tools/*` (`createReadTool`/`createWriteTool`) | `packages/coding-agent/test/tools.test.ts` | `bun:test`, per-test `tmpdir` |
| hermes | `tools/file_operations.py`, `tools/file_tools.py` | `tests/tools/test_file_operations.py`, `test_file_tools.py`, `test_file_write_safety.py`, `test_file_read_guards.py`, `test_credential_files.py` | `pytest` (`tmp_path`, `parametrize`, `monkeypatch`) |

**opencode** — read/write are Effect services over a permission + location layer:
- Reads through the location filesystem, asserting `read` permission; **denied
  permission reads nothing** (`readCalls == []`, error `Unable to read X`).
- **Missing path is a model-visible tool failure**, not a system defect
  (`{ type: "error", value: "Unable to read <path>" }`), and asserts *no* read
  was attempted.
- **Binary detected by file magic, not extension**: a PNG in a `pixel.bin` file
  is still returned as image media; an `application/octet-stream` blob is
  rejected `Cannot read binary file`; malformed UTF-8 → `MalformedUtf8Error`.
- **Bounded directory pagination**: `read` of a directory forwards `offset`/`limit`
  to a `list` call and returns a truncated page.
- **Text pagination**: `offset`/`limit` produce a bounded `text-page` with
  `truncated`/`next` continuation; offset past EOF → `OffsetOutOfRangeError`.
- Image handling: resize to configured `max_width`, enforce `max_base64_bytes`,
  reject undecodable image data, fall back to original when the resizer is absent.
- Write: **creates a relative file once** (`existed: false`, single underlying
  write); **overwrite reports it** (`existed: true`, "Wrote file successfully");
  **preserves exactly one BOM** (`﻿before` + `after` → `﻿after`, never
  doubled); external absolute paths require an `external_directory` approval
  before `edit`, and **denied approval writes nothing** (`writes == []`).

**pi** — read/write are plain tools over real temp dirs:
- Read: **content that fits within limits** returns with no truncation banner;
  **ENOENT** rejects with `/ENOENT|not found/i`.
- **Truncate at 2000 lines**: a 2500-line file shows through `Line 2000` and
  `[Showing lines 1-2000 of 2500. Use offset=2001 to continue.]`.
- **Truncate at ~50 KB bytes**: a <2000-line but oversized file shows the byte-
  limit banner `[Showing lines 1-N of 500 (... limit). Use offset=... to continue.]`.
- `offset`, `limit`, and `offset + limit` slice the file and report remaining
  lines; **offset beyond length** → `Offset 100 is beyond end of file (3 lines total)`.
- **Truncation details**: `details.truncation` carries `truncated`, `truncatedBy:"lines"`,
  `totalLines: 2500`, `outputLines: 2000`.
- **MIME from magic**: a PNG stored as `image.txt` is read as `[image/png]`;
  a `.png` file whose bytes are `"definitely not a png"` returns as text, no
  image block. **BMP → PNG**: a 1×1 BMP is converted (`[Image converted from
  image/bmp to image/png.]`).
- Write: **writes contents** (`Successfully wrote`, `details` undefined);
  **creates parent directories** (`nested/dir/test.txt`).

**hermes** — read/write wrapped in a security policy:
- **Write deny-list for secrets**: `~/.ssh/{id_rsa,authorized_keys}`, `~/.netrc`,
  `~/.pgpass`, `~/.npmrc`, `~/.pypirc`, `~/.aws/*` (e.g. `credentials`),
  `~/.kube/config`, and OAuth JSON — all rejected by `_is_write_denied`, with a
  credential-specific error message; `/etc/shadow`, `/etc/hosts` blocked too.
  Tilde-expansion is applied before the check; a configurable safe-root can
  further restrict writes but **never overrides the static deny-list**.
- **Binary rule by content, not just extension**: `_is_likely_binary` flags a
  file when a high ratio (>~30%) of the first ~1000 chars are non-printable
  (excluding tab/newline) — `"\x00\x01\x02\x03" * 250` is binary; `"Hello
  world\nLine 2\n"` is not. Extension is a fast path (`.png`, `.db` → binary).
- **Reject `read_file`'s line-numbered format on write**: `write_file` refuses
  short, status-dominated content that echoes the internal `1|content` read
  banner back (so the model can't round-trip line-numbered output into a file),
  while a large file that merely quotes the banner is allowed.
- **Path traversal blocked**: `../../.ssh/id_rsa`, `../../etc/passwd`, and
  absolute paths are rejected before any I/O; symlinks that resolve outside the
  root are rejected before `realpath`.
- Read guards: device/pseudo files (`/dev/zero`, `/proc/*/fd`) rejected before
  I/O; oversized reads truncate with a continuation hint rather than hard-failing;
  BOM stripped on read and preserved (never doubled) on write.

## 4. Completeness gaps

Ranked by risk. Each becomes a case in §5.

1. **No direct tests at all** (highest): the tools could regress silently.
   Baseline positive/negative round-trip coverage is missing. *(new: agent-seddon)*
2. **UTF-8-only failure mode is untested and opaque.** `read_to_string` rejects
   any non-UTF-8 file, but there is no test pinning that behaviour, and unlike
   opencode/hermes we emit no typed "binary file" signal — just the raw OS error.
3. **Output cap is untested through `read_file`.** `truncate` is unit-tested in
   isolation, but nothing asserts a >12 KB file actually comes back capped with
   the `[output truncated]` marker via the tool.
4. **No pagination.** Peers (opencode, pi) page large files by `offset`/`limit`
   with continuation hints; agent-seddon has neither the schema fields nor the
   behaviour. Out of scope to *add* here, but worth noting as a divergence.
5. **`write_file` overwrites silently.** No "created vs overwrote" signal
   (opencode `existed`, pi banners). Byte-count message exists but the
   create-vs-overwrite distinction is untested/absent.
6. **No write deny-list.** Unlike hermes, `write_file` will happily write
   `~/.ssh/id_rsa`-style relative paths *if inside cwd*; `resolve_within` only
   stops escapes, not sensitive names within the tree. Mostly mitigated by the
   cwd sandbox, but no test documents the boundary.
7. **Path-safety is proven for `resolve_within` but not end-to-end through the
   tools.** `edit.rs` has `negative_path_escape`; `read_file`/`write_file` do not.

Gaps 4–6 are behaviour agent-seddon has deliberately *not* built (the cwd
sandbox + `bash` escape hatch is the design). The test plan below therefore
focuses on locking down what exists (1–3, 7) and pins the intentional
divergences as explicit corner cases rather than porting peer features wholesale.

## 5. Table-driven test plan

Target file: **`crates/agent-tools/src/core.rs`** — add a `#[cfg(test)] mod tests`
mirroring the style of [`edit.rs`](../../crates/agent-tools/src/edit.rs)
(a `run(dir, tool, args)` helper + `#[rstest]` `#[case::…]` blocks).

Doubles used: `agent_testkit::tempdir()` for an isolated filesystem
([`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs));
`agent_core::ToolContext { cwd }`; no provider/memory doubles needed (these tools
touch only the filesystem).

Case tags: `(port: <peer>)` mirrors a peer case; `(new: agent-seddon)` is an
agent-seddon-specific invariant.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolContext;
    use agent_testkit::tempdir;
    use rstest::rstest;
    use serde_json::{json, Value};

    async fn run(dir: &std::path::Path, tool: &dyn Tool, args: Value) -> Observation {
        tool.execute(args, &ToolContext { cwd: dir.to_path_buf() })
            .await
            .unwrap()
    }

    // --- read_file --------------------------------------------------------
    // A seed file `f.txt` is written before each case (except where the path
    // points elsewhere). `Ok(substr)` ⇒ ok output contains `substr`;
    // `Err(substr)` ⇒ error output contains `substr`.
    #[rstest]
    #[case::positive_reads_contents(              // (port: pi "fits within limits")
        Some(("f.txt", "hello world")),
        json!({"path": "f.txt"}),
        Ok("hello world"))]
    #[case::positive_reads_nested(
        Some(("a/b/c.txt", "deep")),
        json!({"path": "a/b/c.txt"}),
        Ok("deep"))]
    #[case::boundary_reads_empty_file(
        Some(("f.txt", "")),
        json!({"path": "f.txt"}),
        Ok(""))]
    #[case::corner_reads_unicode(                 // (new: agent-seddon)
        Some(("f.txt", "café π")),
        json!({"path": "f.txt"}),
        Ok("café π"))]
    #[case::negative_missing_file(                // (port: pi ENOENT / opencode missing-path)
        None,
        json!({"path": "nope.txt"}),
        Err("could not read"))]
    #[case::negative_path_escape(                 // (new: agent-seddon resolve_within)
        None,
        json!({"path": "../secret"}),
        Err("escape"))]
    #[case::negative_absolute_path(               // (new: agent-seddon)
        None,
        json!({"path": "/etc/passwd"}),
        Err("absolute"))]
    #[case::negative_missing_arg(
        None,
        json!({}),
        Err("missing string argument"))]
    #[tokio::test]
    async fn read_file_cases(
        #[case] seed: Option<(&str, &str)>,
        #[case] args: Value,
        #[case] expected: std::result::Result<&str, &str>,
    ) { /* seed via std::fs (create parent dirs), run ReadFileTool, assert */ }

    // Cap + non-UTF-8 need bespoke bodies (large/invalid bytes), kept out of
    // the string table above.
    #[tokio::test]
    async fn read_file_boundary_output_capped_over_12kb() {
        // (new: agent-seddon; cf. pi 50KB / 2000-line truncation)
        // write MAX_OUTPUT + 500 bytes, assert ok output ends with
        // "[output truncated]" and is <= MAX_OUTPUT + marker length.
    }

    #[tokio::test]
    async fn read_file_negative_non_utf8_is_model_error() {
        // (new: agent-seddon; cf. opencode BinaryFileError / hermes _is_likely_binary)
        // write invalid UTF-8 bytes (e.g. 0x80), assert obs.is_error and the
        // message names the path — pins the UTF-8-only contract.
    }

    // --- write_file -------------------------------------------------------
    // `expected_file` is the on-disk content the write should leave at `path`.
    #[rstest]
    #[case::positive_writes_contents(             // (port: pi "writes file contents")
        json!({"path": "out.txt", "content": "hi"}),
        "out.txt", "hi")]
    #[case::positive_creates_parent_dirs(         // (port: pi "creates parent directories")
        json!({"path": "nested/dir/out.txt", "content": "deep"}),
        "nested/dir/out.txt", "deep")]
    #[case::boundary_writes_empty_content(
        json!({"path": "out.txt", "content": ""}),
        "out.txt", "")]
    #[case::corner_writes_unicode(                // (new: agent-seddon)
        json!({"path": "out.txt", "content": "café π"}),
        "out.txt", "café π")]
    #[tokio::test]
    async fn write_file_positive_cases(
        #[case] args: Value,
        #[case] check_path: &str,
        #[case] expected_file: &str,
    ) { /* run WriteFileTool, assert !is_error, assert on-disk content, assert
           "wrote N bytes" in output */ }

    #[rstest]
    #[case::positive_overwrites_existing(         // (port: opencode overwrite reports)
        Some("before"),
        json!({"path": "f.txt", "content": "after"}),
        "after")]
    #[case::negative_path_escape(                 // (new: agent-seddon; cf. hermes traversal)
        None,
        json!({"path": "../evil", "content": "x"}),
        // asserts is_error contains "escape"; file must not exist
        "\0ESCAPE")]
    #[case::negative_absolute_path(               // (new: agent-seddon)
        None,
        json!({"path": "/tmp/evil", "content": "x"}),
        "\0ABSOLUTE")]
    #[case::negative_missing_content_arg(
        None,
        json!({"path": "f.txt"}),
        "\0MISSING")]
    #[tokio::test]
    async fn write_file_overwrite_and_reject_cases(
        #[case] pre: Option<&str>,
        #[case] args: Value,
        #[case] expected: &str,       // "\0…" sentinels ⇒ expect error, no write
    ) { /* seed `pre` if Some; run; branch on sentinel */ }
}
```

Prefix legend (matching existing `core.rs`/`edit.rs` cases): `positive_` (happy
path), `negative_` (must error), `corner_` (unusual-but-valid, e.g. unicode,
empty), `boundary_` (at a limit, e.g. output cap, empty file).

## 6. References

- agent-seddon impl: [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs)
  (`ReadFileTool`, `WriteFileTool`).
- agent-seddon shared safety: [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)
  (`resolve_within`, `truncate`, `MAX_OUTPUT = 12_000`).
- agent-seddon test style to copy: [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs)
  (`mod tests`), plus the `resolve_within`/`truncate` tables in `lib.rs`.
- Test doubles: [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir()`).
- Component doc: [`docs/components/tools.md`](../components/tools.md).
- opencode: `packages/core/test/tool-read.test.ts`, `tool-read-filesystem.test.ts`,
  `tool-write.test.ts`.
- pi: `packages/coding-agent/test/tools.test.ts` (`read tool` / `write tool` blocks).
- hermes: `tests/tools/test_file_operations.py`, `test_file_tools.py`,
  `test_file_write_safety.py`, `test_file_read_guards.py`, `test_credential_files.py`.
