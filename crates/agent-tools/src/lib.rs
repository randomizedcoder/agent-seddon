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

#[cfg(feature = "tool-pty")]
mod pty_tool;
#[cfg(feature = "tool-pty")]
pub use pty_tool::PtyTool;

#[cfg(feature = "tool-schedule")]
mod schedule_tool;
#[cfg(feature = "tool-schedule")]
pub use schedule_tool::ScheduleTool;

#[cfg(feature = "tool-forge")]
mod forge;
#[cfg(feature = "tool-forge")]
pub use forge::ForgeTool;

#[cfg(feature = "tool-skill-write")]
mod skill_write;
#[cfg(feature = "tool-skill-write")]
pub use skill_write::SkillWriteTool;

#[cfg(feature = "tool-session-export")]
mod session_export;
#[cfg(feature = "tool-web")]
mod web;
#[cfg(feature = "tool-session-export")]
pub use session_export::SessionExportTool;

/// Load a session transcript from `<dir>/<id>.jsonl`. Mirrors the runtime's
/// session store loader; kept here so the tool crate does not depend on the
/// runtime (which depends on it).
#[cfg(feature = "tool-session-export")]
pub(crate) fn session_export_load(
    dir: &std::path::Path,
    id: &str,
) -> std::io::Result<Vec<agent_core::Message>> {
    let raw = std::fs::read_to_string(dir.join(format!("{id}.jsonl")))?;
    Ok(raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

#[cfg(feature = "tool-web-search")]
mod web_search;
#[cfg(feature = "tool-web")]
#[doc(hidden)]
pub use web::bench_sanitize;
#[cfg(feature = "tool-web")]
pub use web::WebFetchTool;
#[cfg(feature = "tool-web-search")]
pub use web_search::WebSearchTool;

#[cfg(feature = "tool-todo")]
mod todo;
#[cfg(feature = "tool-todo")]
pub use todo::TodoWriteTool;

#[cfg(feature = "tool-lsp")]
mod lsp;
#[cfg(feature = "tool-lsp")]
pub use lsp::LspTool;

/// Cap tool output so a runaway command can't blow the context window.
pub(crate) const MAX_OUTPUT: usize = 12_000;

// Path confinement lives in `agent-core` so every model-facing path consumer —
// the file tools here and the `@`-reference resolver in `agent-reference` — shares
// one canonicalizing implementation. Re-exported under the original names so the
// tool modules (and the `confine_symlink_cases` table below) are unchanged.
// Which of the two a given build actually calls depends on the enabled tool
// features (`confine` from core/edit/patch, `resolve_within` from search), so a
// reduced feature set legitimately leaves one unused.
#[allow(unused_imports)]
pub(crate) use agent_core::{confine, resolve_within};

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
        std::sync::Arc::new(BashTool::default()),
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
