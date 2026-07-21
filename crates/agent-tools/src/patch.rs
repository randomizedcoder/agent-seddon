//! `tool-patch` — apply a multi-file unified-diff ("V4A") patch in one call.
//!
//! Where [`edit`](crate::EditTool) replaces one string in one file, `apply_patch`
//! carries a *batch* — **add** new files, **update** existing files across one or
//! more `@@`-anchored hunks, and **delete** files — in a single `*** Begin Patch`
//! … `*** End Patch` envelope. It is **atomic on validation**: every operation is
//! checked against the current tree first (parse OK, add-targets absent,
//! update/delete-targets present, every hunk locates its context); if any check
//! fails, *nothing is written*. The commit phase then applies sequentially and
//! reports a per-file `A`/`M`/`D` summary.
//!
//! Every target is resolved through the shared [`confine`] guard, exactly
//! like `edit`/`write_file`, and the summary is [`truncate`]d like every builtin.

use crate::{arg_str, confine, truncate};
use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "apply_patch".into(),
            description: "Apply a batch of file edits in one call using a unified-diff envelope. \
                 Format: `*** Begin Patch` … `*** End Patch`, containing one or more of \
                 `*** Add File: <path>` (body of `+` lines), `*** Update File: <path>` (one or \
                 more `@@ [context] @@` hunks of ` ` context / `-` remove / `+` add lines; \
                 `*** End of File` anchors a hunk to the end), and `*** Delete File: <path>`. \
                 All-or-nothing: if any operation fails validation, nothing is written."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "patch": { "type": "string", "description": "The full patch envelope." }
                },
                "required": ["patch"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let patch = arg_str(&args, "patch")?;
        let ops = match parse(patch) {
            Ok(ops) => ops,
            Err(e) => return Ok(Observation::error(e)),
        };

        // ---- validation phase: plan every write, touch nothing on disk --------
        let mut plans: Vec<Plan> = Vec::new();
        let mut changes = 0usize;
        for op in &ops {
            match op {
                Op::Add { path, lines } => {
                    let full = match confine(&ctx.cwd, path) {
                        Ok(p) => p,
                        Err(e) => return Ok(Observation::error(e)),
                    };
                    if full.exists() {
                        return Ok(Observation::error(format!(
                            "validation failed: `{path}`: file exists (use Update File, not Add File)"
                        )));
                    }
                    let mut content = lines.join("\n");
                    content.push('\n');
                    changes += 1;
                    plans.push(Plan::Add {
                        path: path.clone(),
                        full,
                        content,
                    });
                }
                Op::Update { path, hunks } => {
                    let full = match confine(&ctx.cwd, path) {
                        Ok(p) => p,
                        Err(e) => return Ok(Observation::error(e)),
                    };
                    let raw = match tokio::fs::read_to_string(&full).await {
                        Ok(c) => c,
                        Err(_) => {
                            return Ok(Observation::error(format!(
                                "validation failed: `{path}`: file not found"
                            )))
                        }
                    };
                    let (bom, body) = split_bom(&raw);
                    let trailing_nl = body.ends_with('\n');
                    let mut file_lines = split_lines(body);
                    let mut changed_here = false;
                    for (n, hunk) in hunks.iter().enumerate() {
                        match apply_hunk(&mut file_lines, hunk) {
                            Ok(true) => changed_here = true,
                            Ok(false) => {}
                            Err(()) => {
                                return Ok(Observation::error(format!(
                                    "validation failed: `{path}`: hunk {}: context not found",
                                    n + 1
                                )))
                            }
                        }
                    }
                    if changed_here {
                        changes += 1;
                    }
                    plans.push(Plan::Update {
                        path: path.clone(),
                        full,
                        content: reassemble(bom, &file_lines, trailing_nl),
                    });
                }
                Op::Delete { path } => {
                    let full = match confine(&ctx.cwd, path) {
                        Ok(p) => p,
                        Err(e) => return Ok(Observation::error(e)),
                    };
                    let raw = match tokio::fs::read_to_string(&full).await {
                        Ok(c) => c,
                        Err(_) => {
                            return Ok(Observation::error(format!(
                                "validation failed: `{path}`: file not found"
                            )))
                        }
                    };
                    changes += 1;
                    plans.push(Plan::Delete {
                        path: path.clone(),
                        full,
                        removed: raw,
                    });
                }
            }
        }
        if changes == 0 {
            return Ok(Observation::error(
                "no changes: the patch produced no edits (only context lines?)",
            ));
        }

        // ---- commit phase: apply sequentially --------------------------------
        let mut summary = String::new();
        for plan in plans {
            match plan {
                Plan::Add {
                    path,
                    full,
                    content,
                } => {
                    if let Some(parent) = full.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            return Ok(Observation::error(format!(
                                "committed {summary}but failed creating `{path}`: {e}"
                            )));
                        }
                    }
                    if let Err(e) = tokio::fs::write(&full, content).await {
                        return Ok(Observation::error(format!(
                            "committed {summary}but failed writing `{path}`: {e}"
                        )));
                    }
                    summary.push_str(&format!("A {path}\n"));
                }
                Plan::Update {
                    path,
                    full,
                    content,
                } => {
                    if let Err(e) = tokio::fs::write(&full, content).await {
                        return Ok(Observation::error(format!(
                            "committed {summary}but failed writing `{path}`: {e}"
                        )));
                    }
                    summary.push_str(&format!("M {path}\n"));
                }
                Plan::Delete {
                    path,
                    full,
                    removed,
                } => {
                    if let Err(e) = tokio::fs::remove_file(&full).await {
                        return Ok(Observation::error(format!(
                            "committed {summary}but failed deleting `{path}`: {e}"
                        )));
                    }
                    summary.push_str(&format!("D {path}\n"));
                    for line in removed.lines() {
                        summary.push('-');
                        summary.push_str(line);
                        summary.push('\n');
                    }
                }
            }
        }
        Ok(Observation::ok(truncate(summary)))
    }
}

// ---------------------------------------------------------------------------
// Parsed representation
// ---------------------------------------------------------------------------

enum Op {
    Add { path: String, lines: Vec<String> },
    Update { path: String, hunks: Vec<Hunk> },
    Delete { path: String },
}

struct Hunk {
    hint: Option<String>,
    /// `(' '|'-'|'+', text)` in order.
    lines: Vec<(char, String)>,
    /// `*** End of File` — match the block from the end of the file.
    eof: bool,
}

enum Plan {
    Add {
        path: String,
        full: std::path::PathBuf,
        content: String,
    },
    Update {
        path: String,
        full: std::path::PathBuf,
        content: String,
    },
    Delete {
        path: String,
        full: std::path::PathBuf,
        removed: String,
    },
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Benchmark hook: parse `patch` and return the number of operations (0 on
/// error). Exposed so `benches/patch.rs` can exercise the parser's hot path with a
/// deterministic in-memory input; `Op` is private, so this returns a plain count.
#[doc(hidden)]
pub fn parse_op_count(patch: &str) -> usize {
    parse(patch).map(|ops| ops.len()).unwrap_or(0)
}

fn parse(patch: &str) -> std::result::Result<Vec<Op>, String> {
    if patch.trim().is_empty() {
        return Err("empty patch".into());
    }
    let all: Vec<&str> = patch.lines().collect();
    let mut i = 0;
    while i < all.len() && all[i].trim() != "*** Begin Patch" {
        i += 1;
    }
    if i >= all.len() {
        return Err("missing `*** Begin Patch` header".into());
    }
    i += 1;

    let mut ops = Vec::new();
    while i < all.len() {
        let line = all[i];
        let t = line.trim_end();
        if t == "*** End Patch" {
            break;
        }
        if let Some(p) = t.strip_prefix("*** Add File: ") {
            let path = p.trim().to_string();
            i += 1;
            let mut lines = Vec::new();
            while i < all.len() && !all[i].starts_with("*** ") {
                match all[i].strip_prefix('+') {
                    Some(rest) => lines.push(rest.to_string()),
                    None => {
                        return Err(format!(
                            "Invalid add file line in `{path}` (expected `+`): `{}`",
                            all[i]
                        ))
                    }
                }
                i += 1;
            }
            ops.push(Op::Add { path, lines });
        } else if let Some(p) = t.strip_prefix("*** Update File: ") {
            let path = p.trim().to_string();
            i += 1;
            let mut hunks: Vec<Hunk> = Vec::new();
            let mut cur: Option<Hunk> = None;
            while i < all.len() {
                let l = all[i];
                let lt = l.trim_end();
                if lt == "*** End Patch"
                    || lt.starts_with("*** Add File:")
                    || lt.starts_with("*** Update File:")
                    || lt.starts_with("*** Delete File:")
                {
                    break;
                }
                if lt.starts_with("*** Move to:") {
                    return Err("moves are not supported".into());
                }
                if lt == "*** End of File" {
                    if let Some(h) = cur.as_mut() {
                        h.eof = true;
                    }
                    i += 1;
                    continue;
                }
                if let Some(rest) = l.strip_prefix("@@") {
                    if let Some(h) = cur.take() {
                        hunks.push(h);
                    }
                    let hint = rest.trim().trim_end_matches("@@").trim();
                    cur = Some(Hunk {
                        hint: (!hint.is_empty()).then(|| hint.to_string()),
                        lines: Vec::new(),
                        eof: false,
                    });
                    i += 1;
                    continue;
                }
                let entry = match l.chars().next() {
                    Some('+') => ('+', l[1..].to_string()),
                    Some('-') => ('-', l[1..].to_string()),
                    Some(' ') => (' ', l[1..].to_string()),
                    _ => {
                        return Err(format!("Invalid patch line in `{path}`: `{l}`"));
                    }
                };
                cur.get_or_insert_with(|| Hunk {
                    hint: None,
                    lines: Vec::new(),
                    eof: false,
                })
                .lines
                .push(entry);
                i += 1;
            }
            if let Some(h) = cur.take() {
                hunks.push(h);
            }
            if hunks.is_empty() {
                return Err(format!(
                    "`{path}`: expected at least one @@ chunk (or hunk line) in an Update File"
                ));
            }
            ops.push(Op::Update { path, hunks });
        } else if let Some(p) = t.strip_prefix("*** Delete File: ") {
            ops.push(Op::Delete {
                path: p.trim().to_string(),
            });
            i += 1;
        } else if t.is_empty() {
            i += 1;
        } else {
            return Err(format!("unexpected patch line: `{line}`"));
        }
    }
    Ok(ops)
}

// ---------------------------------------------------------------------------
// Applier
// ---------------------------------------------------------------------------

/// Apply one hunk to `file_lines` in place. `Ok(true)` if it changed anything,
/// `Ok(false)` for a context-only (no-op) hunk, `Err(())` if the context could
/// not be located.
fn apply_hunk(file_lines: &mut Vec<String>, hunk: &Hunk) -> std::result::Result<bool, ()> {
    let has_edit = hunk.lines.iter().any(|(k, _)| *k == '-' || *k == '+');
    if !has_edit {
        return Ok(false);
    }
    let before: Vec<String> = hunk
        .lines
        .iter()
        .filter(|(k, _)| *k == ' ' || *k == '-')
        .map(|(_, t)| t.clone())
        .collect();
    let after: Vec<String> = hunk
        .lines
        .iter()
        .filter(|(k, _)| *k == ' ' || *k == '+')
        .map(|(_, t)| t.clone())
        .collect();

    if before.is_empty() {
        // Pure addition: insert after the hint anchor, else append at EOF.
        let pos = match &hunk.hint {
            Some(h) => match file_lines.iter().position(|l| l.contains(h.as_str())) {
                Some(idx) => idx + 1,
                None => return Err(()),
            },
            None => file_lines.len(),
        };
        // Splice all inserted lines in one shift, not one `insert()` per line
        // (which re-shifts the tail O(n) times).
        file_lines.splice(pos..pos, after);
        return Ok(true);
    }

    let idx = find_block(file_lines, &before, hunk.eof, hunk.hint.as_deref())?;
    file_lines.splice(idx..idx + before.len(), after);
    Ok(true)
}

/// Find `needle` as a contiguous block in `hay`. A `hint` biases toward the first
/// match at/after a line containing it; `eof` prefers the last match, else the
/// first.
fn find_block(
    hay: &[String],
    needle: &[String],
    eof: bool,
    hint: Option<&str>,
) -> std::result::Result<usize, ()> {
    if needle.is_empty() || needle.len() > hay.len() {
        return Err(());
    }
    let matches: Vec<usize> = (0..=hay.len() - needle.len())
        .filter(|&i| hay[i..i + needle.len()] == *needle)
        .collect();
    if matches.is_empty() {
        return Err(());
    }
    if let Some(h) = hint {
        if let Some(anchor) = hay.iter().position(|l| l.contains(h)) {
            if let Some(&m) = matches.iter().find(|&&i| i >= anchor) {
                return Ok(m);
            }
        }
    }
    if eof {
        Ok(*matches.last().unwrap())
    } else {
        Ok(matches[0])
    }
}

fn split_bom(s: &str) -> (bool, &str) {
    match s.strip_prefix('\u{feff}') {
        Some(rest) => (true, rest),
        None => (false, s),
    }
}

fn split_lines(body: &str) -> Vec<String> {
    if body.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<String> = body.split('\n').map(str::to_string).collect();
    if body.ends_with('\n') {
        v.pop(); // the trailing "" — the newline is tracked separately
    }
    v
}

fn reassemble(bom: bool, lines: &[String], trailing_nl: bool) -> String {
    let mut s = String::new();
    if bom {
        s.push('\u{feff}');
    }
    s.push_str(&lines.join("\n"));
    if trailing_nl && !lines.is_empty() {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolContext;
    use agent_testkit::tempdir;
    use rstest::rstest;
    use std::path::Path;

    async fn run(dir: &Path, args: Value) -> Observation {
        ApplyPatchTool
            .execute(
                args,
                &ToolContext {
                    cwd: dir.to_path_buf(),
                },
            )
            .await
            .unwrap()
    }

    fn envelope(body: &str) -> Value {
        json!({ "patch": format!("*** Begin Patch\n{body}\n*** End Patch") })
    }

    fn seed(dir: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let full = dir.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(full, contents).unwrap();
        }
    }

    /// `Ok(checks)` ⇒ patch applies and each `(path, Some(content))` must equal
    /// `content`, each `(path, None)` must be gone. `Err(substr)` ⇒ error contains
    /// `substr` and the seed tree is left untouched.
    #[rstest]
    // ---- happy path ------------------------------------------------------------
    #[case::multi_op_add_update_delete(
        &[("update.txt", "before\n"), ("remove.txt", "remove\n")],
        envelope("*** Add File: nested/new.txt\n+created\n*** Update File: update.txt\n@@\n-before\n+after\n*** Delete File: remove.txt"),
        Ok(vec![("nested/new.txt", Some("created\n")), ("update.txt", Some("after\n")), ("remove.txt", None)]),
    )]
    #[case::update_with_context_hint(
        &[("src/main.py", "def greet():\n    print(\"hello\")\n")],
        envelope("*** Update File: src/main.py\n@@ def greet @@\n def greet():\n-    print(\"hello\")\n+    print(\"hi\")"),
        Ok(vec![("src/main.py", Some("def greet():\n    print(\"hi\")\n"))]),
    )]
    #[case::add_only_hunk_appends_at_eof(
        &[("app.py", "existing = True\n")],
        envelope("*** Update File: app.py\n+def new_func():\n+    return True"),
        Ok(vec![("app.py", Some("existing = True\ndef new_func():\n    return True\n"))]),
    )]
    #[case::eof_anchored_chunk(
        &[("f.txt", "marker\nmiddle\nmarker\nend\n")],
        envelope("*** Update File: f.txt\n@@\n-marker\n+marker changed\n end\n*** End of File"),
        Ok(vec![("f.txt", Some("marker\nmiddle\nmarker changed\nend\n"))]),
    )]
    #[case::preserves_bom(
        &[("bom.txt", "\u{feff}old\n")],
        envelope("*** Update File: bom.txt\n@@\n-old\n+new"),
        Ok(vec![("bom.txt", Some("\u{feff}new\n"))]),
    )]
    #[case::add_new_file(
        &[],
        envelope("*** Add File: pkg/mod.rs\n+pub fn a() {}\n+pub fn b() {}"),
        Ok(vec![("pkg/mod.rs", Some("pub fn a() {}\npub fn b() {}\n"))]),
    )]
    // ---- atomic batch / validation --------------------------------------------
    #[case::invalid_hunk_writes_nothing(
        &[("a.py", "def good():\n    return 1\n"), ("b.py", "completely different\n")],
        envelope("*** Update File: a.py\n@@\n-    return 1\n+    return 2\n*** Update File: b.py\n@@\n-THIS LINE DOES NOT EXIST\n+new"),
        Err("validation failed"),
    )]
    #[case::later_update_missing_blocks_earlier_add(
        &[],
        envelope("*** Add File: created.txt\n+created\n*** Update File: missing.txt\n@@\n-before\n+after"),
        Err("missing.txt"),
    )]
    #[case::hunk_number_in_error(
        &[("a.py", "first = 1\n")],
        envelope("*** Update File: a.py\n@@ first @@\n-first = 1\n+first = 2\n@@ missing @@\n-does_not_exist = 1\n+does_not_exist = 2"),
        Err("hunk 2"),
    )]
    #[case::only_context_hunks_no_changes(
        &[("a.py", "anchor\n")],
        envelope("*** Update File: a.py\n@@ anchor @@\n anchor"),
        Err("no changes"),
    )]
    // ---- rejections ------------------------------------------------------------
    #[case::add_existing_file_rejected(
        &[("existing.txt", "sentinel\n")],
        envelope("*** Add File: existing.txt\n+replacement"),
        Err("exists"),
    )]
    #[case::move_rejected(
        &[("old.txt", "before\n")],
        envelope("*** Update File: old.txt\n*** Move to: moved.txt\n@@\n-before\n+after"),
        Err("moves are not supported"),
    )]
    #[case::malformed_add_line(
        &[],
        envelope("*** Add File: add.txt\nmissing plus"),
        Err("Invalid add file line"),
    )]
    #[case::update_without_chunk(
        &[("update.txt", "x\n")],
        envelope("*** Update File: update.txt"),
        Err("at least one @@ chunk"),
    )]
    #[case::empty_patch(&[], json!({ "patch": "" }), Err("empty"))]
    #[case::path_escape_rejected(&[], envelope("*** Add File: ../secret\n+x"), Err("escape"))]
    #[tokio::test]
    async fn apply_patch_cases(
        #[case] files: &[(&str, &str)],
        #[case] args: Value,
        #[case] expected: std::result::Result<Vec<(&str, Option<&str>)>, &str>,
    ) {
        let dir = tempdir();
        seed(&dir, files);
        let obs = run(&dir, args).await;
        match expected {
            Ok(checks) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                for (path, want) in checks {
                    match want {
                        Some(content) => assert_eq!(
                            std::fs::read_to_string(dir.join(path)).unwrap().as_str(),
                            content,
                            "file `{path}`"
                        ),
                        None => assert!(!dir.join(path).exists(), "`{path}` should be gone"),
                    }
                }
            }
            Err(substr) => {
                assert!(obs.is_error, "expected error, got ok: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "error `{}` missing `{substr}`",
                    obs.content
                );
                // Atomic: the seed tree must be exactly as it was.
                for &(path, orig) in files {
                    assert_eq!(
                        std::fs::read_to_string(dir.join(path)).unwrap(),
                        orig,
                        "seed `{path}` was modified despite a failed patch"
                    );
                }
            }
        }
    }

    // A failed batch must not commit an *earlier* op (add) that validated before a
    // later op (update of a missing file) failed.
    #[tokio::test]
    async fn atomic_earlier_add_not_committed_on_later_failure() {
        let dir = tempdir();
        let obs = run(
            &dir,
            envelope(
                "*** Add File: created.txt\n+created\n*** Update File: missing.txt\n@@\n-a\n+b",
            ),
        )
        .await;
        assert!(obs.is_error);
        assert!(
            !dir.join("created.txt").exists(),
            "add committed despite atomic failure"
        );
    }

    // Delete reports the real removed lines, not a placeholder.
    #[tokio::test]
    async fn delete_reports_removed_lines() {
        let dir = tempdir();
        seed(&dir, &[("old.py", "def old_func():\n    return 42\n")]);
        let obs = run(&dir, envelope("*** Delete File: old.py")).await;
        assert!(!obs.is_error, "{}", obs.content);
        assert!(!dir.join("old.py").exists());
        assert!(obs.content.contains("D old.py"));
        assert!(
            obs.content.contains("-def old_func():"),
            "summary: {}",
            obs.content
        );
    }

    // A hunk deep in a large file must apply without truncating the file.
    #[tokio::test]
    async fn big_file_not_truncated() {
        let dir = tempdir();
        let mut lines: Vec<String> = (0..2500).map(|i| format!("line_{i}")).collect();
        lines[2200] = "old_value".into();
        let content = lines.join("\n") + "\n";
        seed(&dir, &[("big.py", content.as_str())]);
        let obs = run(
            &dir,
            envelope("*** Update File: big.py\n@@\n line_2199\n-old_value\n+new_value"),
        )
        .await;
        assert!(!obs.is_error, "{}", obs.content);
        let out = std::fs::read_to_string(dir.join("big.py")).unwrap();
        assert_eq!(out.lines().count(), 2500, "file was truncated");
        assert!(out.contains("new_value") && !out.contains("old_value"));
    }
}
