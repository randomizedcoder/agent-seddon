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
    use agent_testkit::tempdir;
    use rstest::rstest;
    use serde_json::json;

    fn ctx(dir: &Path) -> ToolContext {
        ToolContext {
            cwd: dir.to_path_buf(),
        }
    }

    /// A dir with `a.txt` (foo/bar/foobar), `src/main.rs`, and `README.md`.
    fn fixture() -> PathBuf {
        let dir = tempdir();
        std::fs::write(dir.join("a.txt"), "foo\nbar\nfoobar").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "x").unwrap();
        std::fs::write(dir.join("README.md"), "x").unwrap();
        dir
    }

    // --- rel: relative-path formatting (pure) ------------------------------
    #[rstest]
    #[case::positive_inside("/work/repo/src/x.rs", "/work/repo", "src/x.rs")]
    #[case::boundary_cwd_itself("/work/repo", "/work/repo", "")]
    #[case::negative_outside("/other/x", "/work/repo", "/other/x")]
    fn rel_cases(#[case] path: &str, #[case] cwd: &str, #[case] expected: &str) {
        assert_eq!(rel(Path::new(path), Path::new(cwd)), expected);
    }

    // --- resolve_root: default "." + escape rejection ----------------------
    #[rstest]
    #[case::positive_default(json!({}), true)]
    #[case::positive_subdir(json!({"path": "src"}), true)]
    #[case::negative_escape(json!({"path": "../.."}), false)]
    #[case::negative_absolute(json!({"path": "/etc"}), false)]
    fn resolve_root_cases(#[case] args: Value, #[case] ok: bool) {
        assert_eq!(resolve_root(Path::new("/work/repo"), &args).is_ok(), ok);
    }

    // --- grep --------------------------------------------------------------
    #[rstest]
    #[case::positive_matches_with_line_numbers("foo", json!({}), Ok(vec!["a.txt:1:foo", "a.txt:3:foobar"]))]
    #[case::boundary_no_match("zzzznope", json!({}), Ok(vec![]))]
    #[case::negative_invalid_regex("(", json!({}), Err("invalid regex"))]
    #[case::negative_path_escape("x", json!({"path": "../.."}), Err("escape"))]
    #[tokio::test]
    async fn grep_cases(
        #[case] pattern: &str,
        #[case] extra: Value,
        #[case] expected: std::result::Result<Vec<&str>, &str>,
    ) {
        let dir = fixture();
        let mut args = json!({ "pattern": pattern });
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let obs = GrepTool.execute(args, &ctx(&dir)).await.unwrap();
        match expected {
            Ok(needles) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                for n in needles {
                    assert!(
                        obs.content.contains(n),
                        "output missing `{n}`:\n{}",
                        obs.content
                    );
                }
            }
            Err(substr) => {
                assert!(obs.is_error, "expected error, got: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "error missing `{substr}`: {}",
                    obs.content
                );
            }
        }
    }

    // --- find --------------------------------------------------------------
    #[rstest]
    #[case::positive_rs_files("\\.rs$", vec!["src/main.rs"], vec!["README.md", "a.txt"])]
    #[case::positive_all(".", vec!["a.txt", "src/main.rs", "README.md"], vec![])]
    #[case::boundary_no_match("\\.zzz$", vec![], vec!["a.txt", "src/main.rs"])]
    #[tokio::test]
    async fn find_cases(
        #[case] pattern: &str,
        #[case] present: Vec<&str>,
        #[case] absent: Vec<&str>,
    ) {
        let dir = fixture();
        let obs = FindTool
            .execute(json!({ "pattern": pattern }), &ctx(&dir))
            .await
            .unwrap();
        assert!(!obs.is_error, "{}", obs.content);
        for p in present {
            assert!(obs.content.contains(p), "missing `{p}`:\n{}", obs.content);
        }
        for a in absent {
            assert!(
                !obs.content.contains(a),
                "should not contain `{a}`:\n{}",
                obs.content
            );
        }
    }

    // --- ls ----------------------------------------------------------------
    #[tokio::test]
    async fn ls_marks_dirs_with_trailing_slash() {
        let dir = fixture();
        let obs = LsTool.execute(json!({}), &ctx(&dir)).await.unwrap();
        assert!(!obs.is_error);
        assert!(
            obs.content.contains("src/"),
            "dirs get a trailing slash:\n{}",
            obs.content
        );
        assert!(obs.content.contains("a.txt"));
    }
}
