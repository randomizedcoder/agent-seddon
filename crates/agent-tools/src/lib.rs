//! Built-in tools behind the `Tool` seam.
//!
//! Tools are grouped by cargo feature so a build can include only what it needs:
//!   * `tool-core`   — `bash`, `read_file`, `write_file`
//!   * `tool-edit`   — `edit` (surgical string replace)
//!   * `tool-search` — `grep`, `find`, `ls` (gitignore-aware)
//!
//! The shared path-safety / output-capping helpers live here and are reused by
//! every tool module. Custom tools plug into the same `ToolRegistry` (see
//! `docs/extending.md`) without touching the loop.

use agent_core::{Error, Result};
use serde_json::Value;
use std::path::{Component, PathBuf};

#[cfg(feature = "tool-core")]
mod core;
#[cfg(feature = "tool-core")]
pub use core::{BashTool, ReadFileTool, WriteFileTool};

#[cfg(feature = "tool-edit")]
mod edit;
#[cfg(feature = "tool-edit")]
pub use edit::EditTool;

#[cfg(feature = "tool-search")]
mod search;
#[cfg(feature = "tool-search")]
pub use search::{FindTool, GrepTool, LsTool};

/// Cap tool output so a runaway command can't blow the context window.
pub(crate) const MAX_OUTPUT: usize = 12_000;

/// Resolve a caller-supplied path against the working directory, rejecting any
/// path that would escape it (absolute paths, `..` traversal). Lexical only —
/// it does not follow symlinks, so `bash` remains the unconfined escape hatch by
/// design; this is defense-in-depth for the file tools.
pub(crate) fn resolve_within(
    cwd: &std::path::Path,
    path: &str,
) -> std::result::Result<PathBuf, String> {
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

pub(crate) fn truncate(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        s.truncate(MAX_OUTPUT);
        s.push_str("\n...[output truncated]");
    }
    s
}

pub(crate) fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Tool(format!("missing string argument `{key}`")))
}

/// Optional string argument (returns `None` if absent or not a string).
#[cfg(feature = "tool-search")]
pub(crate) fn arg_str_opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Optional bool argument (defaults to `default` when absent).
#[cfg(any(feature = "tool-edit", feature = "tool-search"))]
pub(crate) fn arg_bool(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// Convenience: the default `tool-core` tool set as trait objects. The registry
/// (`agent-runtime`) is the primary wiring path; this stays for embedders.
#[cfg(feature = "tool-core")]
pub fn default_tools() -> Vec<std::sync::Arc<dyn agent_core::Tool>> {
    vec![
        std::sync::Arc::new(BashTool),
        std::sync::Arc::new(ReadFileTool),
        std::sync::Arc::new(WriteFileTool),
    ]
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
