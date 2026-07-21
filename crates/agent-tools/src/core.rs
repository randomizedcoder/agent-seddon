//! `tool-core` — the always-useful trio: `bash`, `read_file`, `write_file`.

use crate::{arg_bool, arg_str, confine, truncate};
use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Wall-clock ceiling for a single `bash` invocation, so a hung or looping
/// command can't stall the agent indefinitely. Lowered to 1s under `cfg(test)` so
/// the timeout test doesn't wait two minutes (the tool is inert as a dependency,
/// where `cfg(test)` is off, so production keeps the full 120s).
const BASH_TIMEOUT_SECS: u64 = if cfg!(test) { 1 } else { 120 };

// --- bash -----------------------------------------------------------------

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }
    /// Not parallel-safe: `bash` runs arbitrary commands in the shared working
    /// directory with unrestricted filesystem side effects, so the loop must not
    /// run it concurrently with sibling tool calls (which could race on the same
    /// files). The file tools are lexically cwd-pinned; `bash` is the escape hatch.
    fn parallel_safe(&self) -> bool {
        false
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".into(),
            description:
                "Run a bash command in the working directory and return combined stdout/stderr."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The bash command to run." }
                },
                "required": ["command"]
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let command = arg_str(&args, "command")?;
        let run = tokio::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&ctx.cwd)
            .kill_on_drop(true) // ensure the child is killed if we time out
            .output();
        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(BASH_TIMEOUT_SECS),
            run,
        )
        .await
        {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(agent_core::Error::Tool(format!("spawning bash: {e}"))),
            Err(_) => {
                return Ok(Observation::error(format!(
                    "command timed out after {BASH_TIMEOUT_SECS}s and was killed"
                )))
            }
        };

        let mut buf = String::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stdout.is_empty() {
            buf.push_str(&stdout);
        }
        if !stderr.is_empty() {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str("[stderr]\n");
            buf.push_str(&stderr);
        }
        let code = output.status.code().unwrap_or(-1);
        if buf.is_empty() {
            buf = format!("(no output, exit code {code})");
        }
        let is_error = !output.status.success();
        Ok(Observation {
            content: truncate(buf),
            is_error,
        })
    }
}

// --- read_file ------------------------------------------------------------

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read_file".into(),
            description: "Read a text file (relative to the working directory). By default \
                          returns the whole file; pass `offset`/`limit` to read a line window \
                          of a large file, and `line_numbers` to prefix each line with its \
                          number. Binary files are detected and reported, not dumped."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file." },
                    "offset": { "type": "integer", "description": "1-based line to start from (default 1)." },
                    "limit": { "type": "integer", "description": "Max lines to return from `offset` (default: all)." },
                    "line_numbers": { "type": "boolean", "description": "Prefix each line with its 1-based number (default false)." }
                },
                "required": ["path"]
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let path = arg_str(&args, "path")?;
        let offset = args.get("offset").and_then(Value::as_u64);
        let limit = args.get("limit").and_then(Value::as_u64);
        let line_numbers = arg_bool(&args, "line_numbers", false);
        let full = match confine(&ctx.cwd, path) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        let bytes = match tokio::fs::read(&full).await {
            Ok(b) => b,
            Err(e) => return Ok(Observation::error(format!("could not read `{path}`: {e}"))),
        };
        // Binary files (NUL byte or invalid UTF-8) are reported, not dumped as
        // mojibake — the model gets a clear, actionable message instead.
        let text = match as_text(&bytes) {
            Some(t) => t,
            None => {
                return Ok(Observation::ok(format!(
                    "`{path}` is a binary file ({} bytes); not shown as text.",
                    bytes.len()
                )))
            }
        };
        Ok(Observation::ok(render_read(
            text,
            offset,
            limit,
            line_numbers,
        )))
    }
}

/// Interpret bytes as UTF-8 text, or `None` if they look binary (contain a NUL or
/// aren't valid UTF-8).
fn as_text(bytes: &[u8]) -> Option<&str> {
    if bytes.contains(&0) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

/// Render a read result: apply the optional `offset`/`limit` line window, add
/// line numbers if asked, and cap the output — appending a footer that tells the
/// model the file is larger and how to page when the window doesn't reach the end.
fn render_read(text: &str, offset: Option<u64>, limit: Option<u64>, line_numbers: bool) -> String {
    let windowed = offset.is_some() || limit.is_some();
    if !windowed && !line_numbers {
        // Fast path: whole file, no numbering. If truncated *and* the file has
        // multiple lines (so line paging is meaningful), hint how to page.
        let total = text.lines().count();
        let out = truncate(text.to_string());
        if out.len() < text.len() && total > 1 {
            return format!(
                "{out}\n[showing a prefix of {total} lines; pass `offset`/`limit` to page]"
            );
        }
        return out;
    }

    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    let start = offset
        .map(|o| o.max(1) as usize - 1)
        .unwrap_or(0)
        .min(total);
    let end = match limit {
        Some(l) => (start + l as usize).min(total),
        None => total,
    };

    let mut body = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        if line_numbers {
            body.push_str(&format!("{:>6}\t{line}\n", start + i + 1));
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    let mut out = truncate(body);
    if end < total {
        out.push_str(&format!(
            "\n[lines {}–{} of {total}; pass `offset`/`limit` to page]",
            start + 1,
            end
        ));
    }
    out
}

// --- write_file -----------------------------------------------------------

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write_file".into(),
            description:
                "Write (create or overwrite) a UTF-8 text file relative to the working directory."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file." },
                    "content": { "type": "string", "description": "Full file contents to write." }
                },
                "required": ["path", "content"]
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let path = arg_str(&args, "path")?;
        let content = arg_str(&args, "content")?;
        let full = match confine(&ctx.cwd, path) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        if let Some(parent) = full.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return Ok(Observation::error(format!(
                    "could not create dir for `{path}`: {e}"
                )));
            }
        }
        // Atomic write: stage into a sibling temp file, then rename over the target.
        // A crash mid-write leaves the original intact instead of a truncated file
        // (rename is atomic within a filesystem, and the temp is in the same dir).
        let tmp = {
            let name = full
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default();
            full.with_file_name(format!(".{name}.tmp"))
        };
        if let Err(e) = tokio::fs::write(&tmp, content).await {
            return Ok(Observation::error(format!("could not write `{path}`: {e}")));
        }
        if let Err(e) = tokio::fs::rename(&tmp, &full).await {
            let _ = tokio::fs::remove_file(&tmp).await; // don't leave a turd behind
            return Ok(Observation::error(format!("could not write `{path}`: {e}")));
        }
        Ok(Observation::ok(format!(
            "wrote {} bytes to `{path}`",
            content.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_OUTPUT;
    use agent_testkit::tempdir;
    use rstest::rstest;
    use std::path::Path;

    /// Run a tool, folding a hard `Err` (e.g. a missing required arg) into a
    /// model-visible error `Observation` so every case asserts the same shape.
    async fn run(dir: &Path, tool: &dyn Tool, args: Value) -> Observation {
        tool.execute(
            args,
            &ToolContext {
                cwd: dir.to_path_buf(),
            },
        )
        .await
        .unwrap_or_else(|e| Observation::error(e.to_string()))
    }

    fn seed(dir: &Path, file: Option<(&str, &str)>) {
        if let Some((path, contents)) = file {
            let full = dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(full, contents).unwrap();
        }
    }

    // --- read_file ----------------------------------------------------------
    // `Ok(exact)` ⇒ ok, content == `exact`; `Err(substr)` ⇒ error containing `substr`.
    #[rstest]
    #[case::positive_reads_contents(Some(("f.txt", "hello world")), json!({"path": "f.txt"}), Ok("hello world"))]
    #[case::positive_reads_nested(Some(("a/b/c.txt", "deep")), json!({"path": "a/b/c.txt"}), Ok("deep"))]
    #[case::boundary_reads_empty_file(Some(("f.txt", "")), json!({"path": "f.txt"}), Ok(""))]
    #[case::corner_reads_unicode(Some(("f.txt", "café π")), json!({"path": "f.txt"}), Ok("café π"))]
    #[case::negative_missing_file(None, json!({"path": "nope.txt"}), Err("could not read"))]
    #[case::negative_path_escape(None, json!({"path": "../secret"}), Err("escape"))]
    #[case::negative_absolute_path(None, json!({"path": "/etc/passwd"}), Err("absolute"))]
    #[case::negative_missing_arg(None, json!({}), Err("missing string argument"))]
    #[tokio::test]
    async fn read_file_cases(
        #[case] file: Option<(&str, &str)>,
        #[case] args: Value,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        let dir = tempdir();
        seed(&dir, file);
        let obs = run(&dir, &ReadFileTool, args).await;
        match expected {
            Ok(exact) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                assert_eq!(obs.content, exact);
            }
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

    // A file over the output cap comes back truncated with the marker (the cap is
    // unit-tested in isolation; this pins it *through* read_file).
    #[tokio::test]
    async fn read_file_boundary_output_capped_over_12kb() {
        let dir = tempdir();
        std::fs::write(dir.join("big.txt"), "a".repeat(MAX_OUTPUT + 500)).unwrap();
        let obs = run(&dir, &ReadFileTool, json!({"path": "big.txt"})).await;
        assert!(!obs.is_error);
        assert!(obs.content.ends_with("[output truncated]"));
        assert!(obs.content.len() <= MAX_OUTPUT + "\n...[output truncated]".len());
    }

    // A binary file (invalid UTF-8 / NUL bytes) is reported cleanly — a non-error
    // observation naming the file + size — rather than dumped as mojibake or
    // surfaced as a read failure.
    #[tokio::test]
    async fn read_file_binary_is_reported_not_dumped() {
        let dir = tempdir();
        std::fs::write(dir.join("blob.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let obs = run(&dir, &ReadFileTool, json!({"path": "blob.bin"})).await;
        assert!(!obs.is_error, "binary read is informational, not an error");
        assert!(
            obs.content.contains("binary file"),
            "message: {}",
            obs.content
        );
        assert!(obs.content.contains("blob.bin"));
    }

    // --- read_file: line-window paging + line numbers -----------------------
    fn numbered_fixture(dir: &Path) {
        // 10 lines: "line1".."line10"
        let body = (1..=10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(dir.join("f.txt"), body).unwrap();
    }

    #[rstest]
    // offset+limit returns just that window
    #[case::window(json!({"path": "f.txt", "offset": 3, "limit": 2}), &["line3", "line4"], &["line2", "line5"])]
    // offset to end
    #[case::offset_to_end(json!({"path": "f.txt", "offset": 9}), &["line9", "line10"], &["line8"])]
    // limit from start
    #[case::limit_from_start(json!({"path": "f.txt", "limit": 2}), &["line1", "line2"], &["line3"])]
    #[tokio::test]
    async fn read_file_windows(
        #[case] args: Value,
        #[case] present: &[&str],
        #[case] absent: &[&str],
    ) {
        let dir = tempdir();
        numbered_fixture(&dir);
        let obs = run(&dir, &ReadFileTool, args).await;
        assert!(!obs.is_error, "{}", obs.content);
        for p in present {
            assert!(obs.content.contains(p), "missing `{p}`:\n{}", obs.content);
        }
        for a in absent {
            assert!(
                !obs.content.contains(a),
                "should omit `{a}`:\n{}",
                obs.content
            );
        }
    }

    #[tokio::test]
    async fn read_file_window_has_paging_footer() {
        let dir = tempdir();
        numbered_fixture(&dir);
        let obs = run(&dir, &ReadFileTool, json!({"path": "f.txt", "limit": 3})).await;
        assert!(
            obs.content.contains("of 10"),
            "footer missing: {}",
            obs.content
        );
    }

    #[tokio::test]
    async fn read_file_line_numbers_prefix() {
        let dir = tempdir();
        numbered_fixture(&dir);
        let obs = run(
            &dir,
            &ReadFileTool,
            json!({"path": "f.txt", "offset": 2, "limit": 1, "line_numbers": true}),
        )
        .await;
        assert!(obs.content.contains("2\tline2"), "content: {}", obs.content);
    }

    // --- write_file ---------------------------------------------------------
    // `pre` seeds an existing file (overwrite cases). Asserts on-disk == expected +
    // a "wrote N bytes" acknowledgement.
    #[rstest]
    #[case::positive_writes_contents(None, json!({"path": "out.txt", "content": "hi"}), "out.txt", "hi")]
    #[case::positive_creates_parent_dirs(None, json!({"path": "nested/dir/out.txt", "content": "deep"}), "nested/dir/out.txt", "deep")]
    #[case::boundary_writes_empty_content(None, json!({"path": "out.txt", "content": ""}), "out.txt", "")]
    #[case::corner_writes_unicode(None, json!({"path": "out.txt", "content": "café π"}), "out.txt", "café π")]
    #[case::positive_overwrites_existing(Some("before"), json!({"path": "f.txt", "content": "after"}), "f.txt", "after")]
    #[tokio::test]
    async fn write_file_positive_cases(
        #[case] pre: Option<&str>,
        #[case] args: Value,
        #[case] check_path: &str,
        #[case] expected_file: &str,
    ) {
        let dir = tempdir();
        if let Some(content) = pre {
            std::fs::write(dir.join(check_path), content).unwrap();
        }
        let obs = run(&dir, &WriteFileTool, args).await;
        assert!(!obs.is_error, "unexpected error: {}", obs.content);
        assert!(obs.content.contains("wrote"), "ack: {}", obs.content);
        assert_eq!(
            std::fs::read_to_string(dir.join(check_path)).unwrap(),
            expected_file
        );
    }

    // Atomic write leaves no `.tmp` sibling behind and overwrites cleanly.
    #[tokio::test]
    async fn write_file_is_atomic_no_temp_leftover() {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), "old contents here").unwrap();
        let obs = run(
            &dir,
            &WriteFileTool,
            json!({"path": "f.txt", "content": "new"}),
        )
        .await;
        assert!(!obs.is_error, "{}", obs.content);
        assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), "new");
        // The staging temp (`.f.txt.tmp`) must be gone after the rename.
        assert!(
            !dir.join(".f.txt.tmp").exists(),
            "temp file should not survive"
        );
    }

    // Rejections must error and write nothing.
    #[rstest]
    #[case::negative_path_escape(json!({"path": "../evil", "content": "x"}), "escape")]
    #[case::negative_absolute_path(json!({"path": "/tmp/evil-agent-seddon", "content": "x"}), "absolute")]
    #[case::negative_missing_content_arg(json!({"path": "f.txt"}), "missing string argument")]
    #[tokio::test]
    async fn write_file_reject_cases(#[case] args: Value, #[case] err_substr: &str) {
        let dir = tempdir();
        let obs = run(&dir, &WriteFileTool, args).await;
        assert!(obs.is_error, "expected error, got ok: {}", obs.content);
        assert!(
            obs.content.contains(err_substr),
            "error `{}` missing `{err_substr}`",
            obs.content
        );
    }

    // --- bash ---------------------------------------------------------------
    // `Ok(substr)` ⇒ non-error observation containing `substr`; `Err(substr)` ⇒
    // error observation containing `substr`.
    #[rstest]
    // happy path / output capture
    #[case::positive_simple_stdout(json!({"command": "echo hello"}), Ok("hello"))]
    #[case::positive_runs_in_cwd(json!({"command": "pwd"}), Ok("agent-testkit-"))]
    #[case::positive_multiline_stdout(json!({"command": "printf 'a\\nb\\nc\\n'"}), Ok("a\nb\nc"))]
    #[case::corner_unicode_roundtrip(json!({"command": "printf 'héllo π'"}), Ok("héllo π"))]
    // exit-code semantics
    #[case::negative_nonzero_exit_is_error(json!({"command": "exit 3"}), Err("exit code 3"))]
    #[case::corner_zero_exit_empty_output(json!({"command": "true"}), Ok("no output, exit code 0"))]
    #[case::negative_false_is_error(json!({"command": "false"}), Err("exit code 1"))]
    // stderr framing
    #[case::corner_stderr_marker(json!({"command": "echo out; echo err 1>&2"}), Ok("[stderr]\nerr"))]
    #[case::corner_stderr_only_on_success(json!({"command": "echo warn 1>&2; true"}), Ok("[stderr]\nwarn"))]
    // argument validation
    #[case::negative_missing_command_arg(json!({}), Err("missing string argument `command`"))]
    #[tokio::test]
    async fn bash_output_cases(
        #[case] args: Value,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        let dir = tempdir();
        let obs = run(&dir, &BashTool, args).await;
        match expected {
            Ok(substr) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "output `{}` missing `{substr}`",
                    obs.content
                );
            }
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

    // Shell output over the cap is truncated with the marker (pins that `bash`
    // applies `truncate`, char-boundary safe, on real command output).
    #[tokio::test]
    async fn bash_boundary_output_truncated_at_cap() {
        let dir = tempdir();
        let obs = run(&dir, &BashTool, json!({"command": "yes x | head -c 20000"})).await;
        assert!(!obs.is_error, "{}", obs.content);
        assert!(obs.content.ends_with("[output truncated]"));
        assert!(obs.content.len() <= crate::MAX_OUTPUT + "\n...[output truncated]".len());
    }

    // A trailing newline is preserved verbatim, not miscounted or stripped.
    #[tokio::test]
    async fn bash_corner_trailing_newline_preserved() {
        let dir = tempdir();
        let obs = run(&dir, &BashTool, json!({"command": "printf 'one\\n'"})).await;
        assert!(!obs.is_error);
        assert_eq!(obs.content, "one\n");
    }

    // A command that outlives the (test-lowered 1s) ceiling is killed and reported
    // as a model-visible timeout error, not a hang.
    #[tokio::test]
    async fn bash_boundary_timeout_kills_and_reports() {
        let dir = tempdir();
        let obs = run(&dir, &BashTool, json!({"command": "sleep 5"})).await;
        assert!(obs.is_error, "expected timeout error, got: {}", obs.content);
        assert!(
            obs.content.contains("timed out"),
            "message: {}",
            obs.content
        );
    }

    // Pin the concurrency contract: bash is NOT parallel-safe (shared cwd + FS
    // side effects), unlike the default-true trait impl.
    #[test]
    fn bash_corner_parallel_safe_is_false() {
        assert!(!BashTool.parallel_safe());
    }
}
