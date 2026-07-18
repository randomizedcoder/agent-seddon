//! `tool-search` — gitignore-aware `grep`, `find`, and `ls`.
//!
//! All three use ripgrep's `ignore` crate to walk the tree (respecting
//! `.gitignore`) and stay within the working directory. The walk is synchronous,
//! so it runs on a blocking thread. Output is capped like the other tools.

use crate::{arg_bool, arg_str, arg_str_opt, resolve_within, truncate};
use agent_core::{Error, Observation, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Stop after this many matches/entries to bound output.
const MAX_HITS: usize = 300;

/// Resolve the optional `path` argument (default ".") within the working dir.
fn resolve_root(cwd: &Path, args: &Value) -> std::result::Result<PathBuf, String> {
    resolve_within(cwd, arg_str_opt(args, "path").unwrap_or("."))
}

/// Path relative to `cwd`, for stable, short output.
fn rel(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).display().to_string()
}

// --- grep -----------------------------------------------------------------

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "grep".into(),
            description: "Search file contents by regex (gitignore-aware). Returns \
                          `path:line:text` for each match."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regular expression to search for." },
                    "path": { "type": "string", "description": "Directory or file to search (default '.')." },
                    "case_insensitive": { "type": "boolean", "description": "Case-insensitive match (default false)." }
                },
                "required": ["pattern"]
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let pattern = arg_str(&args, "pattern")?.to_string();
        let ci = arg_bool(&args, "case_insensitive", false);
        let root = match resolve_root(&ctx.cwd, &args) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        let re = match RegexBuilder::new(&pattern).case_insensitive(ci).build() {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(format!("invalid regex: {e}"))),
        };
        let cwd = ctx.cwd.clone();
        let out = tokio::task::spawn_blocking(move || grep_walk(&root, &cwd, &re))
            .await
            .map_err(|e| Error::Tool(format!("search task failed: {e}")))?;
        Ok(Observation::ok(truncate(out)))
    }
}

fn grep_walk(root: &Path, cwd: &Path, re: &regex::Regex) -> String {
    let mut out = String::new();
    let mut hits = 0usize;
    for result in WalkBuilder::new(root).build() {
        if hits >= MAX_HITS {
            break;
        }
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue, // skip binary / unreadable files
        };
        let rel_path = rel(entry.path(), cwd);
        for (i, line) in content.lines().enumerate() {
            if re.is_match(line) {
                out.push_str(&format!("{rel_path}:{}:{}\n", i + 1, line.trim_end()));
                hits += 1;
                if hits >= MAX_HITS {
                    out.push_str("...[more matches truncated]\n");
                    break;
                }
            }
        }
    }
    if out.is_empty() {
        "(no matches)".into()
    } else {
        out
    }
}

// --- find -----------------------------------------------------------------

pub struct FindTool;

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "find".into(),
            description:
                "Find files whose path matches a regex (gitignore-aware). Returns matching \
                          paths."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex matched against each relative path." },
                    "path": { "type": "string", "description": "Root directory to search (default '.')." }
                },
                "required": ["pattern"]
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let pattern = arg_str(&args, "pattern")?.to_string();
        let root = match resolve_root(&ctx.cwd, &args) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        let re = match RegexBuilder::new(&pattern).build() {
            Ok(r) => r,
            Err(e) => return Ok(Observation::error(format!("invalid regex: {e}"))),
        };
        let cwd = ctx.cwd.clone();
        let out = tokio::task::spawn_blocking(move || find_walk(&root, &cwd, &re))
            .await
            .map_err(|e| Error::Tool(format!("search task failed: {e}")))?;
        Ok(Observation::ok(truncate(out)))
    }
}

fn find_walk(root: &Path, cwd: &Path, re: &regex::Regex) -> String {
    let mut out = String::new();
    let mut hits = 0usize;
    for result in WalkBuilder::new(root).build() {
        if hits >= MAX_HITS {
            out.push_str("...[more matches truncated]\n");
            break;
        }
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        let rel_path = rel(entry.path(), cwd);
        if re.is_match(&rel_path) {
            out.push_str(&rel_path);
            out.push('\n');
            hits += 1;
        }
    }
    if out.is_empty() {
        "(no matches)".into()
    } else {
        out
    }
}

// --- ls -------------------------------------------------------------------

pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "ls".into(),
            description: "List a directory. Directories are suffixed with '/'. Set `recursive` to \
                          walk the whole tree (gitignore-aware)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to list (default '.')." },
                    "recursive": { "type": "boolean", "description": "Walk the whole tree (default false)." }
                }
            }),
        }
    }
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let recursive = arg_bool(&args, "recursive", false);
        let root = match resolve_root(&ctx.cwd, &args) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        let cwd = ctx.cwd.clone();
        let out = tokio::task::spawn_blocking(move || ls_walk(&root, &cwd, recursive))
            .await
            .map_err(|e| Error::Tool(format!("ls task failed: {e}")))?;
        Ok(Observation::ok(truncate(out)))
    }
}

fn ls_walk(root: &Path, cwd: &Path, recursive: bool) -> String {
    if recursive {
        let mut out = String::new();
        let mut hits = 0usize;
        for result in WalkBuilder::new(root).build() {
            if hits >= MAX_HITS {
                out.push_str("...[truncated]\n");
                break;
            }
            let entry = match result {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.path() == root {
                continue;
            }
            let mut line = rel(entry.path(), cwd);
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                line.push('/');
            }
            out.push_str(&line);
            out.push('\n');
            hits += 1;
        }
        return if out.is_empty() {
            "(empty)".into()
        } else {
            out
        };
    }

    let mut names: Vec<String> = Vec::new();
    let read = match std::fs::read_dir(root) {
        Ok(r) => r,
        Err(e) => return format!("could not list `{}`: {e}", rel(root, cwd)),
    };
    for entry in read.flatten() {
        let mut name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            name.push('/');
        }
        names.push(name);
    }
    names.sort();
    if names.is_empty() {
        "(empty)".into()
    } else {
        names.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolContext;
    use serde_json::json;

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("agent-search-test-{nanos}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ctx(dir: &Path) -> ToolContext {
        ToolContext {
            cwd: dir.to_path_buf(),
        }
    }

    #[tokio::test]
    async fn grep_finds_matches() {
        let dir = tempdir();
        std::fs::write(dir.join("a.txt"), "foo\nbar\nfoobar").unwrap();
        let obs = GrepTool
            .execute(json!({"pattern": "foo"}), &ctx(&dir))
            .await
            .unwrap();
        assert!(!obs.is_error);
        assert!(obs.content.contains("a.txt:1:foo"));
        assert!(obs.content.contains("a.txt:3:foobar"));
        assert!(!obs.content.contains(":2:"));
    }

    #[tokio::test]
    async fn grep_invalid_regex_errors() {
        let dir = tempdir();
        let obs = GrepTool
            .execute(json!({"pattern": "("}), &ctx(&dir))
            .await
            .unwrap();
        assert!(obs.is_error);
        assert!(obs.content.contains("invalid regex"));
    }

    #[tokio::test]
    async fn find_matches_paths() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "x").unwrap();
        std::fs::write(dir.join("README.md"), "x").unwrap();
        let obs = FindTool
            .execute(json!({"pattern": "\\.rs$"}), &ctx(&dir))
            .await
            .unwrap();
        assert!(obs.content.contains("src/main.rs"));
        assert!(!obs.content.contains("README.md"));
    }

    #[tokio::test]
    async fn ls_lists_entries_with_dir_suffix() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("file.txt"), "x").unwrap();
        let obs = LsTool.execute(json!({}), &ctx(&dir)).await.unwrap();
        assert!(obs.content.contains("sub/"));
        assert!(obs.content.contains("file.txt"));
    }

    #[tokio::test]
    async fn path_escape_rejected() {
        let dir = tempdir();
        let obs = GrepTool
            .execute(json!({"pattern": "x", "path": "../.."}), &ctx(&dir))
            .await
            .unwrap();
        assert!(obs.is_error);
    }
}
