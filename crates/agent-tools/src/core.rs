//! `tool-core` — the always-useful trio: `bash`, `read_file`, `write_file`.

use crate::{arg_str, resolve_within, truncate};
use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Wall-clock ceiling for a single `bash` invocation, so a hung or looping
/// command can't stall the agent indefinitely.
const BASH_TIMEOUT_SECS: u64 = 120;

// --- bash -----------------------------------------------------------------

pub struct BashTool;

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
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
            description: "Read a UTF-8 text file (relative to the working directory) and return its contents.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file." }
                },
                "required": ["path"]
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let path = arg_str(&args, "path")?;
        let full = match resolve_within(&ctx.cwd, path) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        match tokio::fs::read_to_string(&full).await {
            Ok(content) => Ok(Observation::ok(truncate(content))),
            Err(e) => Ok(Observation::error(format!("could not read `{path}`: {e}"))),
        }
    }
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
        let full = match resolve_within(&ctx.cwd, path) {
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
        match tokio::fs::write(&full, content).await {
            Ok(()) => Ok(Observation::ok(format!(
                "wrote {} bytes to `{path}`",
                content.len()
            ))),
            Err(e) => Ok(Observation::error(format!("could not write `{path}`: {e}"))),
        }
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

    // read_to_string is UTF-8 only: a non-UTF-8 file is a model-visible error that
    // names the path (pins the intentional UTF-8-only contract).
    #[tokio::test]
    async fn read_file_negative_non_utf8_is_model_error() {
        let dir = tempdir();
        std::fs::write(dir.join("blob.bin"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let obs = run(&dir, &ReadFileTool, json!({"path": "blob.bin"})).await;
        assert!(obs.is_error);
        assert!(obs.content.contains("blob.bin"), "message: {}", obs.content);
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
}
