//! Built-in tools behind the `Tool` seam.
//!
//! Tools are grouped by cargo feature so a build can include only what it needs:
//!   * `tool-core`   — `bash`, `read_file`, `write_file`
//!   * `tool-edit`   — `edit` (surgical string replace)
//!   * `tool-patch`  — `apply_patch` (multi-file unified-diff batch)
//!   * `tool-search` — `grep`, `find`, `ls` (gitignore-aware)
//!   * `tool-git`    — `git_read`/`git_diff`/`git_worktree`/… over the RepoBackend seam
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
#[doc(hidden)]
pub use edit::bench_apply;
#[cfg(feature = "tool-edit")]
pub use edit::EditTool;

#[cfg(feature = "tool-patch")]
mod patch;
#[cfg(feature = "tool-patch")]
#[doc(hidden)]
pub use patch::parse_op_count;
#[cfg(feature = "tool-patch")]
pub use patch::ApplyPatchTool;

#[cfg(feature = "tool-search")]
mod search;
#[cfg(feature = "tool-search")]
pub use search::{FindTool, GrepTool, LsTool};

#[cfg(feature = "tool-search-index")]
mod search_index;
#[cfg(feature = "tool-search-index")]
pub use search_index::{IndexLsTool, SearchTool};

#[cfg(feature = "tool-git")]
mod git;
#[cfg(feature = "tool-git")]
pub use git::{
    git_tools, GitBranchesTool, GitCheckpointTool, GitDiffTool, GitGrepTool, GitLogTool,
    GitReadTool, GitStatusTool, GitTreeTool, GitWorktreeTool,
};

#[cfg(feature = "tool-metrics")]
mod metrics;
#[cfg(feature = "tool-metrics")]
pub use metrics::MetricsTool;

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

/// Resolve a caller-supplied path within `cwd` **and defend against symlink escape**.
///
/// [`resolve_within`] is lexical only, so a symlink *inside* the working dir that
/// points outside it (planted e.g. via `bash`, or already present in a repo) slips
/// past: a model could then `read_file` a link to `/etc/passwd`, or `edit` /
/// `write_file` / `apply_patch` through a link to clobber a file outside the tree.
/// `confine` additionally canonicalizes the deepest existing prefix of the resolved
/// path (which resolves any symlink in it) and requires it to stay under the real
/// `cwd`; a symlink component that resolves — or dangles — outside is rejected. Used
/// by the file-opening tools (`bash` stays the unconfined escape hatch by design).
pub(crate) fn confine(cwd: &std::path::Path, path: &str) -> std::result::Result<PathBuf, String> {
    let candidate = resolve_within(cwd, path)?; // lexical: reject absolute / `..` escape
    let real_cwd = cwd
        .canonicalize()
        .map_err(|e| format!("cannot resolve working directory: {e}"))?;

    // Walk up to the deepest existing prefix; `canonicalize` resolves any symlink
    // along the way. If that real path leaves `cwd`, the path escapes via a symlink.
    let mut probe = candidate.clone();
    loop {
        match probe.canonicalize() {
            Ok(real) => {
                if real.starts_with(&real_cwd) {
                    return Ok(candidate);
                }
                return Err(format!(
                    "path escapes the working directory via a symlink: `{path}`"
                ));
            }
            Err(_) => {
                // A not-yet-existing component (a new file/dir being created). If it
                // is itself a symlink (a dangling link), reject — writing through it
                // could still land outside the tree.
                if std::fs::symlink_metadata(&probe)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    return Err(format!(
                        "path is a symlink that cannot be confined: `{path}`"
                    ));
                }
                match probe.parent() {
                    Some(p) if p != probe => probe = p.to_path_buf(),
                    _ => {
                        return Err(format!("path escapes the working directory: `{path}`"));
                    }
                }
            }
        }
    }
}

pub(crate) fn truncate(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        // Cut on a char boundary — `String::truncate` panics if `MAX_OUTPUT` lands
        // inside a multi-byte char, which real tool output (UTF-8) can trigger.
        let mut cut = MAX_OUTPUT;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
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
#[cfg(any(feature = "tool-core", feature = "tool-edit", feature = "tool-search"))]
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
    use super::*;
    use rstest::rstest;
    use serde_json::json;
    use std::path::Path;

    // --- resolve_within: path safety ---------------------------------------
    // `Some(expected)` ⇒ resolves to that path; `None` ⇒ rejected.
    #[rstest]
    #[case::positive_relative("src/main.rs", Some("/work/repo/src/main.rs"))]
    #[case::positive_nested("a/b/c.txt", Some("/work/repo/a/b/c.txt"))]
    #[case::positive_curdir(".", Some("/work/repo"))]
    #[case::positive_normalized_in_bounds("./a/../b", Some("/work/repo/b"))]
    #[case::boundary_empty("", Some("/work/repo"))]
    #[case::boundary_bare_parent("..", None)]
    #[case::negative_parent_escape("../../etc/passwd", None)]
    #[case::negative_absolute("/etc/passwd", None)]
    #[case::negative_mixed_escape("a/../../secret", None)]
    #[case::corner_double_slash("a//b", Some("/work/repo/a/b"))]
    #[case::corner_trailing_slash("a/b/", Some("/work/repo/a/b"))]
    #[case::corner_unicode("café/π.rs", Some("/work/repo/café/π.rs"))]
    fn resolve_within_cases(#[case] path: &str, #[case] expected: Option<&str>) {
        let cwd = Path::new("/work/repo");
        match (resolve_within(cwd, path), expected) {
            (Ok(got), Some(exp)) => assert_eq!(got, Path::new(exp), "path `{path}`"),
            (Err(_), None) => {}
            (got, exp) => panic!("path `{path}`: got {got:?}, expected {exp:?}"),
        }
    }

    // --- confine: symlink-escape defense (real filesystem) -----------------
    // Each case builds a scenario under a temp working dir and asserts whether
    // `confine` allows the path. The adversarial cases are the confirmed escape
    // vectors: a leaf/parent-dir/dangling symlink pointing OUTSIDE the tree.
    #[cfg(unix)]
    #[derive(Clone, Copy)]
    enum Scenario {
        RealFileInRepo,
        NewFileInRepo,
        InternalSymlink,         // link → target inside the repo (allowed)
        LeafSymlinkOutside,      // link → file outside the repo
        ParentDirSymlinkOutside, // linkdir → dir outside; write linkdir/x
        DanglingSymlinkOutside,  // link → nonexistent outside path
    }

    #[cfg(unix)]
    #[rstest]
    #[case::positive_real_file(Scenario::RealFileInRepo, true)]
    #[case::positive_new_file(Scenario::NewFileInRepo, true)]
    #[case::corner_internal_symlink_allowed(Scenario::InternalSymlink, true)]
    #[case::adversarial_leaf_symlink_escapes(Scenario::LeafSymlinkOutside, false)]
    #[case::adversarial_parent_dir_symlink_escapes(Scenario::ParentDirSymlinkOutside, false)]
    #[case::adversarial_dangling_symlink_escapes(Scenario::DanglingSymlinkOutside, false)]
    fn confine_symlink_cases(#[case] scenario: Scenario, #[case] expect_ok: bool) {
        use std::os::unix::fs::symlink;
        let cwd = agent_testkit::tempdir();
        let outside = agent_testkit::tempdir();
        std::fs::write(outside.join("secret.txt"), "top secret").unwrap();

        let path: String = match scenario {
            Scenario::RealFileInRepo => {
                std::fs::write(cwd.join("file.txt"), "hi").unwrap();
                "file.txt".into()
            }
            Scenario::NewFileInRepo => "brand-new.txt".into(),
            Scenario::InternalSymlink => {
                std::fs::write(cwd.join("target.txt"), "hi").unwrap();
                symlink(cwd.join("target.txt"), cwd.join("link")).unwrap();
                "link".into()
            }
            Scenario::LeafSymlinkOutside => {
                symlink(outside.join("secret.txt"), cwd.join("link")).unwrap();
                "link".into()
            }
            Scenario::ParentDirSymlinkOutside => {
                symlink(&outside, cwd.join("linkdir")).unwrap();
                "linkdir/pwned.txt".into()
            }
            Scenario::DanglingSymlinkOutside => {
                symlink(outside.join("does-not-exist"), cwd.join("dead")).unwrap();
                "dead".into()
            }
        };

        let got = confine(&cwd, &path);
        assert_eq!(
            got.is_ok(),
            expect_ok,
            "scenario allowed={:?} for path `{path}` (result: {got:?})",
            got.is_ok()
        );
    }

    // --- truncate: output capping ------------------------------------------
    #[rstest]
    #[case::positive_short("hello", "hello")]
    #[case::boundary_empty("", "")]
    fn truncate_passthrough_when_under_limit(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(truncate(input.to_string()), expected);
    }

    #[test]
    fn truncate_boundary_exactly_at_limit_is_unchanged() {
        let s = "a".repeat(MAX_OUTPUT);
        assert_eq!(truncate(s.clone()), s);
    }

    #[test]
    fn truncate_over_limit_appends_marker() {
        let out = truncate("a".repeat(MAX_OUTPUT + 500));
        assert!(out.starts_with(&"a".repeat(MAX_OUTPUT)));
        assert!(out.ends_with("[output truncated]"));
    }

    #[test]
    fn truncate_corner_does_not_split_multibyte_char() {
        // Byte index MAX_OUTPUT lands inside the 2-byte 'é' — must not panic.
        let mut s = "a".repeat(MAX_OUTPUT - 1);
        s.push('é');
        s.push_str("tail");
        let out = truncate(s);
        assert!(out.ends_with("[output truncated]"));
        // The kept prefix is valid UTF-8 (no torn char): the 'é' was dropped whole.
        assert!(out.starts_with(&"a".repeat(MAX_OUTPUT - 1)));
    }

    // --- arg extractors -----------------------------------------------------
    #[rstest]
    #[case::positive_present(json!({"k": "v"}), true)]
    #[case::positive_empty_string(json!({"k": ""}), true)]
    #[case::negative_missing(json!({}), false)]
    #[case::negative_wrong_type_number(json!({"k": 5}), false)]
    #[case::negative_wrong_type_bool(json!({"k": true}), false)]
    #[case::corner_null(json!({"k": null}), false)]
    fn arg_str_cases(#[case] args: serde_json::Value, #[case] ok: bool) {
        assert_eq!(arg_str(&args, "k").is_ok(), ok);
    }

    #[cfg(feature = "tool-search")]
    #[rstest]
    #[case::positive_present(json!({"k": "v"}), Some("v"))]
    #[case::negative_missing(json!({}), None)]
    #[case::negative_wrong_type(json!({"k": 5}), None)]
    #[case::corner_null(json!({"k": null}), None)]
    fn arg_str_opt_cases(#[case] args: serde_json::Value, #[case] expected: Option<&str>) {
        assert_eq!(arg_str_opt(&args, "k"), expected);
    }

    #[cfg(any(feature = "tool-edit", feature = "tool-search"))]
    #[rstest]
    #[case::positive_true(json!({"k": true}), false, true)]
    #[case::positive_false(json!({"k": false}), true, false)]
    #[case::boundary_missing_uses_default(json!({}), true, true)]
    #[case::corner_wrong_type_uses_default(json!({"k": "yes"}), false, false)]
    #[case::corner_null_uses_default(json!({"k": null}), true, true)]
    fn arg_bool_cases(
        #[case] args: serde_json::Value,
        #[case] default: bool,
        #[case] expected: bool,
    ) {
        assert_eq!(arg_bool(&args, "k", default), expected);
    }
}
