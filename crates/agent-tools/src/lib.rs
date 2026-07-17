//! Built-in tools behind the `Tool` seam: `bash`, `read_file`, `write_file`.
//!
//! Each tool is self-describing (its JSON Schema is what the model sees) and
//! executes against a `ToolContext` (the working directory). Custom tools plug
//! into the same `ToolRegistry` without touching the loop.

use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Component, PathBuf};
use std::sync::Arc;

/// Cap tool output so a runaway command can't blow the context window.
const MAX_OUTPUT: usize = 12_000;

/// Wall-clock ceiling for a single `bash` invocation, so a hung or looping
/// command can't stall the agent indefinitely.
const BASH_TIMEOUT_SECS: u64 = 120;

/// Resolve a caller-supplied path against the working directory, rejecting any
/// path that would escape it (absolute paths, `..` traversal). Lexical only —
/// it does not follow symlinks, so `bash` remains the unconfined escape hatch by
/// design; this is defense-in-depth for the file tools.
fn resolve_within(cwd: &std::path::Path, path: &str) -> std::result::Result<PathBuf, String> {
    let candidate = std::path::Path::new(path);
    if candidate.is_absolute() {
        return Err(format!("absolute paths are not allowed: `{path}`"));
    }
    let mut resolved = cwd.to_path_buf();
    for comp in candidate.components() {
        match comp {
            Component::Normal(c) => resolved.push(c),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("path is not allowed: `{path}`"));
            }
        }
    }
    if !resolved.starts_with(cwd) {
        return Err(format!("path escapes the working directory: `{path}`"));
    }
    Ok(resolved)
}

fn truncate(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        s.truncate(MAX_OUTPUT);
        s.push_str("\n...[output truncated]");
    }
    s
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| agent_core::Error::Tool(format!("missing string argument `{key}`")))
}

/// Convenience: build the default tool set.
pub fn default_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(BashTool),
        Arc::new(ReadFileTool),
        Arc::new(WriteFileTool),
    ]
}

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
    use super::resolve_within;
    use std::path::Path;

    #[test]
    fn allows_paths_inside_cwd() {
        let cwd = Path::new("/work/repo");
        assert_eq!(
            resolve_within(cwd, "src/main.rs").unwrap(),
            Path::new("/work/repo/src/main.rs")
        );
        assert_eq!(
            resolve_within(cwd, "./a/../b").unwrap(),
            Path::new("/work/repo/b")
        );
    }

    #[test]
    fn rejects_traversal_and_absolute() {
        let cwd = Path::new("/work/repo");
        assert!(resolve_within(cwd, "../../etc/passwd").is_err());
        assert!(resolve_within(cwd, "/etc/passwd").is_err());
        assert!(resolve_within(cwd, "a/../../secret").is_err());
    }
}
