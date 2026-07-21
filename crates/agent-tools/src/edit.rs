//! `tool-edit` — surgical string replacement, single or batched.
//!
//! Replaces `old_string` with `new_string` in a file. By default the match must
//! be unique (so the model can't silently edit the wrong occurrence); set
//! `replace_all` to replace every occurrence. Far cheaper and safer than
//! rewriting whole files via `write_file`.
//!
//! Beyond the exact single replace it also handles the things that trip up a
//! model editing a file it read earlier:
//! - **CRLF/BOM fidelity** — an `\n`-only `old_string` matches a CRLF file, and
//!   the file's dominant line ending + a leading UTF-8 BOM are preserved on write.
//! - **Multi-edit** — an `edits` array applies a *batch* against the original
//!   content, atomically (all-or-nothing), rejecting overlapping targets.
//! - **Fuzzy fallback** (opt-in, `fuzzy: true`) — when the exact match fails,
//!   retry line-wise ignoring trailing whitespace / smart quotes / unicode dashes
//!   / NBSP / fullwidth forms; exact always wins.
//! - **Stale guard** — refuse to write if the file changed on disk since we read
//!   it (best-effort TOCTOU protection).

use crate::{arg_bool, arg_str, confine, truncate};
use agent_core::{Observation, Result, Tool, ToolContext, ToolSchema};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct EditTool;

/// One replacement.
struct Edit {
    old: String,
    new: String,
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "edit".into(),
            description: "Replace exact text in a file. Provide `old_string`/`new_string` for a \
                 single replacement (must occur once unless `replace_all` is true), or an `edits` \
                 array of `{old_string,new_string}` applied as one atomic batch against the \
                 original content. `\\n`-only strings match CRLF files; a leading BOM and the \
                 file's line endings are preserved. Set `fuzzy` to allow a whitespace/quote/dash \
                 -insensitive fallback when the exact text isn't found."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file." },
                    "old_string": { "type": "string", "description": "Exact text to replace (single-edit mode)." },
                    "new_string": { "type": "string", "description": "Replacement text (single-edit mode)." },
                    "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." },
                    "fuzzy": { "type": "boolean", "description": "Allow a fuzzy fallback when the exact text isn't found (default false)." },
                    "edits": {
                        "type": "array",
                        "description": "Batch mode: replacements applied atomically against the original content.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "old_string": { "type": "string" },
                                "new_string": { "type": "string" }
                            },
                            "required": ["old_string", "new_string"]
                        }
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Observation> {
        let path = arg_str(&args, "path")?;
        let replace_all = arg_bool(&args, "replace_all", false);
        let fuzzy = arg_bool(&args, "fuzzy", false);

        // Gather the edit(s): a multi-edit `edits` array, or a single old/new pair.
        let (edits, multi) = match args.get("edits") {
            Some(v) => match parse_multi(v) {
                Ok(e) => (e, true),
                Err(msg) => return Ok(Observation::error(msg)),
            },
            None => {
                let old = arg_str(&args, "old_string")?.to_string();
                let new = arg_str(&args, "new_string")?.to_string();
                if let Err(msg) = validate_edit(&old, &new) {
                    return Ok(Observation::error(msg));
                }
                (vec![Edit { old, new }], false)
            }
        };

        let full = match confine(&ctx.cwd, path) {
            Ok(p) => p,
            Err(e) => return Ok(Observation::error(e)),
        };
        let raw = match tokio::fs::read_to_string(&full).await {
            Ok(c) => c,
            Err(e) => return Ok(Observation::error(io_err_msg("read", path, &e))),
        };

        // Preserve a leading BOM and the file's dominant line ending; match against
        // an `\n`-normalized copy so an LF `old_string` finds CRLF content.
        let (bom, body) = split_bom(&raw);
        let crlf = body.contains("\r\n");
        let normalized = body.replace("\r\n", "\n");

        let outcome = if multi {
            apply_multi(&normalized, &edits)
        } else {
            apply_single(&normalized, &edits[0], replace_all, fuzzy)
        };
        let (new_norm, n) = match outcome {
            Ok(x) => x,
            Err(msg) => return Ok(Observation::error(format!("{msg} in `{path}`"))),
        };

        let out_body = if crlf {
            new_norm.replace('\n', "\r\n")
        } else {
            new_norm
        };
        let out = if bom {
            format!("\u{feff}{out_body}")
        } else {
            out_body
        };

        // Best-effort stale guard: if the file changed on disk since we read it,
        // refuse to write rather than clobber the newer content.
        if let Ok(current) = tokio::fs::read_to_string(&full).await {
            if current != raw {
                return Ok(Observation::error(format!(
                    "`{path}` changed on disk since it was read; re-read it before editing"
                )));
            }
        }
        if let Err(e) = tokio::fs::write(&full, out).await {
            return Ok(Observation::error(io_err_msg("write", path, &e)));
        }
        Ok(Observation::ok(truncate(format!(
            "edited `{path}` ({n} replacement{})",
            if n == 1 { "" } else { "s" }
        ))))
    }
}

/// `Ok(())` if the pair is a usable replacement; `Err(msg)` otherwise.
fn validate_edit(old: &str, new: &str) -> std::result::Result<(), String> {
    if old.is_empty() {
        return Err("old_string must not be empty".into());
    }
    if old == new {
        return Err("old_string and new_string are identical".into());
    }
    Ok(())
}

fn parse_multi(v: &Value) -> std::result::Result<Vec<Edit>, String> {
    let arr = v.as_array().ok_or("`edits` must be an array")?;
    if arr.is_empty() {
        return Err("`edits` must contain at least one replacement".into());
    }
    let mut edits = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let old = item
            .get("old_string")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("edit {}: missing `old_string`", i + 1))?;
        let new = item
            .get("new_string")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("edit {}: missing `new_string`", i + 1))?;
        validate_edit(old, new).map_err(|m| format!("edit {}: {m}", i + 1))?;
        edits.push(Edit {
            old: old.into(),
            new: new.into(),
        });
    }
    Ok(edits)
}

/// Benchmark hook: run the pure editing computation (exact + optional fuzzy) and
/// return the result length. Exposed so `benches/edit.rs` can exercise the
/// deterministic hot path (`Edit` is private, so this takes plain strings).
#[doc(hidden)]
pub fn bench_apply(content: &str, old: &str, new: &str, fuzzy: bool) -> usize {
    apply_single(
        content,
        &Edit {
            old: old.to_string(),
            new: new.to_string(),
        },
        false,
        fuzzy,
    )
    .map(|(s, _)| s.len())
    .unwrap_or(0)
}

/// Single exact replace on `\n`-normalized `content`, with an optional fuzzy
/// fallback. Returns `(new_content, replacements)`.
fn apply_single(
    content: &str,
    edit: &Edit,
    replace_all: bool,
    fuzzy: bool,
) -> std::result::Result<(String, usize), String> {
    let old = edit.old.replace("\r\n", "\n");
    let new = edit.new.replace("\r\n", "\n");
    let count = content.matches(&old).count();
    if count == 0 {
        if fuzzy {
            return fuzzy_replace(content, &old, &new);
        }
        return Err("old_string not found".into());
    }
    if replace_all {
        Ok((content.replace(&old, &new), count))
    } else if count > 1 {
        Err(format!(
            "old_string is not unique ({count} occurrences); add surrounding context or set replace_all"
        ))
    } else {
        Ok((content.replacen(&old, &new, 1), 1))
    }
}

/// Graduated line-wise fuzzy replace. Tries progressively looser per-line
/// normalizations and applies the *first* level that locates the block **uniquely**
/// — never loosening past an ambiguity, so a looser rule can't silently pick the
/// wrong site. Exact matching is tried first by the caller, so this is a fallback.
///
/// Levels:
///  - `Fold`: strip trailing whitespace + fold smart quotes / dashes / NBSP /
///    fullwidth to ASCII (handles copy-paste unicode drift).
///  - `Collapse`: additionally collapse *all* whitespace (so differing indentation
///    and internal spacing/tabs match); the replacement is re-indented to the
///    file's matched block so the file's own indentation is preserved.
fn fuzzy_replace(
    content: &str,
    old: &str,
    new: &str,
) -> std::result::Result<(String, usize), String> {
    let lines: Vec<&str> = content.split('\n').collect();
    for level in [FuzzLevel::Fold, FuzzLevel::Collapse] {
        // Normalize every file line once per level (not once per window position).
        let norm_lines: Vec<String> = lines.iter().map(|l| norm_line(l, level)).collect();
        let want: Vec<String> = old.split('\n').map(|l| norm_line(l, level)).collect();
        if want.is_empty() || want.len() > lines.len() {
            return Err("old_string not found (even with fuzzy matching)".into());
        }
        let hits: Vec<usize> = (0..=norm_lines.len() - want.len())
            .filter(|&i| norm_lines[i..i + want.len()] == want[..])
            .collect();
        match hits.as_slice() {
            [] => continue, // nothing at this level — loosen and retry
            [i] => {
                let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
                let repl = match level {
                    FuzzLevel::Fold => new.split('\n').map(str::to_string).collect(),
                    // Collapse ignored indentation to match, so re-anchor the
                    // replacement to the file block's indentation.
                    FuzzLevel::Collapse => reindent(new, leading_ws(lines[*i])),
                };
                out.splice(*i..*i + want.len(), repl);
                return Ok((out.join("\n"), 1));
            }
            many => {
                return Err(format!(
                    "old_string is not unique after fuzzy normalization ({} matches); add surrounding context",
                    many.len()
                ))
            }
        }
    }
    Err("old_string not found (even with fuzzy matching)".into())
}

/// How aggressively [`norm_line`] normalizes when fuzzy-matching.
#[derive(Clone, Copy)]
enum FuzzLevel {
    /// Trailing-whitespace + unicode look-alike folding.
    Fold,
    /// Also collapse all whitespace (indentation- and spacing-insensitive).
    Collapse,
}

/// Normalize one line for fuzzy matching at the given level.
fn norm_line(line: &str, level: FuzzLevel) -> String {
    let folded: String = line
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2013}' | '\u{2014}' => '-',
            '\u{00A0}' => ' ',
            c if ('\u{FF01}'..='\u{FF5E}').contains(&c) => {
                char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
            }
            c => c,
        })
        .collect();
    match level {
        FuzzLevel::Fold => folded.trim_end().to_string(),
        // Collapse every run of whitespace to a single space and trim — makes the
        // match blind to indentation width and internal spacing/tab differences.
        FuzzLevel::Collapse => folded.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// The leading-whitespace prefix of a line (its indentation).
fn leading_ws(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Re-indent a replacement block to the file's indentation. Detects the block's
/// own base indent (from its first non-blank line) and rewrites every line so that
/// base becomes `file_indent`, preserving relative nesting deeper than the base.
fn reindent(new: &str, file_indent: &str) -> Vec<String> {
    let base = new
        .split('\n')
        .find(|l| !l.trim().is_empty())
        .map(leading_ws)
        .unwrap_or("");
    new.split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else if let Some(rest) = line.strip_prefix(base) {
                format!("{file_indent}{rest}")
            } else {
                // Line less-indented than the detected base — anchor it directly.
                format!("{file_indent}{}", line.trim_start())
            }
        })
        .collect()
}

/// Multi-edit: each `old` must occur exactly once in the *original* content; the
/// matched spans must not overlap; all replacements are spliced in one pass.
fn apply_multi(content: &str, edits: &[Edit]) -> std::result::Result<(String, usize), String> {
    let mut spans: Vec<(usize, usize, String)> = Vec::with_capacity(edits.len());
    for (i, e) in edits.iter().enumerate() {
        let old = e.old.replace("\r\n", "\n");
        // Count occurrences and grab the first position in a single scan.
        let mut indices = content.match_indices(&old);
        let Some((start, _)) = indices.next() else {
            return Err(format!("edit {}: old_string not found", i + 1));
        };
        if indices.next().is_some() {
            let count = 2 + indices.count();
            return Err(format!(
                "edit {}: old_string is not unique ({count} occurrences)",
                i + 1
            ));
        }
        spans.push((start, start + old.len(), e.new.replace("\r\n", "\n")));
    }
    spans.sort_by_key(|s| s.0);
    for w in spans.windows(2) {
        if w[1].0 < w[0].1 {
            return Err("edits overlap (a later edit's target overlaps an earlier one)".into());
        }
    }
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    for (start, end, new) in &spans {
        result.push_str(&content[cursor..*start]);
        result.push_str(new);
        cursor = *end;
    }
    result.push_str(&content[cursor..]);
    Ok((result, spans.len()))
}

fn split_bom(s: &str) -> (bool, &str) {
    match s.strip_prefix('\u{feff}') {
        Some(rest) => (true, rest),
        None => (false, s),
    }
}

/// Distinguish the common I/O failures so the model gets an actionable signal.
fn io_err_msg(op: &str, path: &str, e: &std::io::Error) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => format!("could not {op} `{path}`: not found (ENOENT)"),
        ErrorKind::PermissionDenied => {
            format!("could not {op} `{path}`: permission denied (EACCES)")
        }
        _ => format!("could not {op} `{path}`: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolContext;
    use agent_testkit::tempdir;
    use rstest::rstest;
    use serde_json::json;
    use std::path::Path;

    async fn run(dir: &Path, args: Value) -> Observation {
        EditTool
            .execute(
                args,
                &ToolContext {
                    cwd: dir.to_path_buf(),
                },
            )
            .await
            .unwrap()
    }

    /// `Ok(final)` ⇒ file should end up as `final`; `Err(substr)` ⇒ error containing
    /// `substr`. The file `f.txt` starts as `initial`.
    #[rstest]
    #[case::positive_unique_replace(
        "hello world",
        json!({"path": "f.txt", "old_string": "world", "new_string": "rust"}),
        Ok("hello rust")
    )]
    #[case::positive_replace_all(
        "a a a",
        json!({"path": "f.txt", "old_string": "a", "new_string": "b", "replace_all": true}),
        Ok("b b b")
    )]
    #[case::corner_replace_with_empty(
        "abc",
        json!({"path": "f.txt", "old_string": "b", "new_string": ""}),
        Ok("ac")
    )]
    #[case::corner_unicode(
        "héllo",
        json!({"path": "f.txt", "old_string": "héllo", "new_string": "wörld"}),
        Ok("wörld")
    )]
    #[case::negative_non_unique_without_flag(
        "a a a",
        json!({"path": "f.txt", "old_string": "a", "new_string": "b"}),
        Err("not unique")
    )]
    #[case::negative_missing_string(
        "hello",
        json!({"path": "f.txt", "old_string": "zzz", "new_string": "x"}),
        Err("not found")
    )]
    #[case::negative_path_escape(
        "hello",
        json!({"path": "../secret", "old_string": "a", "new_string": "b"}),
        Err("escape")
    )]
    // CRLF: an LF old_string matches CRLF content; endings preserved.
    #[case::boundary_crlf_preserved(
        "first\r\nsecond\r\nthird\r\n",
        json!({"path": "f.txt", "old_string": "second", "new_string": "REPLACED"}),
        Ok("first\r\nREPLACED\r\nthird\r\n")
    )]
    #[case::boundary_lf_preserved(
        "first\nsecond\nthird\n",
        json!({"path": "f.txt", "old_string": "second", "new_string": "REPLACED"}),
        Ok("first\nREPLACED\nthird\n")
    )]
    // BOM preserved through the edit.
    #[case::boundary_bom_preserved(
        "\u{feff}old\n",
        json!({"path": "f.txt", "old_string": "old", "new_string": "new"}),
        Ok("\u{feff}new\n")
    )]
    // Fuzzy fallback (opt-in): smart quotes and trailing whitespace.
    #[case::corner_fuzzy_smart_quotes(
        "console.log(\u{2018}hi\u{2019});\n",
        json!({"path": "f.txt", "old_string": "console.log('hi');", "new_string": "console.log('bye');", "fuzzy": true}),
        Ok("console.log('bye');\n")
    )]
    #[case::corner_fuzzy_trailing_ws(
        "line one   \nline two\n",
        json!({"path": "f.txt", "old_string": "line one\nline two", "new_string": "replaced", "fuzzy": true}),
        Ok("replaced\n")
    )]
    // Fuzzy is opt-in: the same smart-quote input fails without the flag.
    #[case::negative_fuzzy_off_by_default(
        "console.log(\u{2018}hi\u{2019});\n",
        json!({"path": "f.txt", "old_string": "console.log('hi');", "new_string": "x"}),
        Err("not found")
    )]
    // Fuzzy Collapse level: indentation differs (file uses 4 spaces, old_string
    // uses 2) — the block still matches and the replacement is re-indented to the
    // file's 4-space block.
    #[case::corner_fuzzy_indentation_flexible(
        "def f():\n    if x:\n        go()\n",
        json!({"path": "f.txt", "old_string": "  if x:\n      go()", "new_string": "if y:\n    stop()", "fuzzy": true}),
        Ok("def f():\n    if y:\n        stop()\n")
    )]
    // Fuzzy Collapse level: interior whitespace differs (tabs vs spaced runs).
    #[case::corner_fuzzy_interior_whitespace(
        "let  x   =  1;\n",
        json!({"path": "f.txt", "old_string": "let x = 1;", "new_string": "let x = 2;", "fuzzy": true}),
        Ok("let x = 2;\n")
    )]
    // Fuzzy ambiguity: two normalized matches — reject with the count, don't guess.
    #[case::negative_fuzzy_ambiguous_reports_count(
        "a = 1\nb = 2\na = 1\n",
        json!({"path": "f.txt", "old_string": "a  =  1", "new_string": "a = 9", "fuzzy": true}),
        Err("2 matches")
    )]
    #[tokio::test]
    async fn edit_cases(
        #[case] initial: &str,
        #[case] args: Value,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), initial).unwrap();
        let obs = run(&dir, args).await;
        match expected {
            Ok(final_content) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                assert_eq!(
                    std::fs::read_to_string(dir.join("f.txt")).unwrap(),
                    final_content
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

    /// Multi-edit: `Ok(final)` / `Err(substr)`; on error the file must be untouched.
    #[rstest]
    #[case::positive_multi_disjoint(
        "alpha\nbeta\ngamma\ndelta\n",
        json!({"path": "f.txt", "edits": [
            {"old_string": "alpha", "new_string": "ALPHA"},
            {"old_string": "gamma", "new_string": "GAMMA"}]}),
        Ok("ALPHA\nbeta\nGAMMA\ndelta\n")
    )]
    #[case::corner_multi_against_original_not_incremental(
        "foo\nbar\nbaz\n",
        json!({"path": "f.txt", "edits": [
            {"old_string": "foo", "new_string": "foo bar"},
            {"old_string": "bar", "new_string": "BAR"}]}),
        Ok("foo bar\nBAR\nbaz\n")
    )]
    #[case::negative_multi_overlap(
        "one\ntwo\nthree\n",
        json!({"path": "f.txt", "edits": [
            {"old_string": "one\ntwo", "new_string": "X"},
            {"old_string": "two\nthree", "new_string": "Y"}]}),
        Err("overlap")
    )]
    #[case::negative_multi_empty(
        "hello\n",
        json!({"path": "f.txt", "edits": []}),
        Err("at least one")
    )]
    #[case::negative_multi_atomic_no_partial(
        "alpha\nbeta\n",
        json!({"path": "f.txt", "edits": [
            {"old_string": "alpha", "new_string": "ALPHA"},
            {"old_string": "missing", "new_string": "X"}]}),
        Err("not found")
    )]
    #[tokio::test]
    async fn multi_edit_cases(
        #[case] initial: &str,
        #[case] args: Value,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        let dir = tempdir();
        std::fs::write(dir.join("f.txt"), initial).unwrap();
        let obs = run(&dir, args).await;
        match expected {
            Ok(final_content) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                assert_eq!(
                    std::fs::read_to_string(dir.join("f.txt")).unwrap(),
                    final_content
                );
            }
            Err(substr) => {
                assert!(obs.is_error, "expected error: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "`{}` missing `{substr}`",
                    obs.content
                );
                // Atomic: a failed batch leaves the file exactly as it was.
                assert_eq!(std::fs::read_to_string(dir.join("f.txt")).unwrap(), initial);
            }
        }
    }

    // A missing file surfaces a distinct ENOENT signal (EACCES is symmetric in the
    // impl but not reliably testable under the root-uid nix build sandbox).
    #[tokio::test]
    async fn missing_file_reports_enoent() {
        let dir = tempdir();
        let obs = run(
            &dir,
            json!({"path": "nope.txt", "old_string": "a", "new_string": "b"}),
        )
        .await;
        assert!(obs.is_error);
        assert!(obs.content.contains("ENOENT"), "message: {}", obs.content);
    }

    // The `Fold` normalizer folds the documented character classes (trailing-ws +
    // unicode look-alikes) without touching interior spacing.
    #[rstest]
    #[case::trailing_ws("code   ", "code")]
    #[case::smart_single("it\u{2019}s", "it's")]
    #[case::smart_double("\u{201C}q\u{201D}", "\"q\"")]
    #[case::em_dash("a\u{2014}b", "a-b")]
    #[case::nbsp("a\u{00A0}b", "a b")]
    #[case::fullwidth("\u{FF21}\u{FF22}\u{FF23}", "ABC")]
    fn fold_normalizer(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(norm_line(input, FuzzLevel::Fold), expected);
    }

    // The `Collapse` normalizer additionally flattens indentation + interior runs.
    #[rstest]
    #[case::leading_indent("    a b", "a b")]
    #[case::interior_runs("a\t\tb   c", "a b c")]
    #[case::tabs_and_spaces("\t a  b ", "a b")]
    fn collapse_normalizer(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(norm_line(input, FuzzLevel::Collapse), expected);
    }

    // reindent re-anchors a replacement block to the file's indentation, keeping
    // relative nesting deeper than the block's own base indent.
    #[test]
    fn reindent_reanchors_to_file_indent() {
        let out = reindent("if x:\n    body\n", "        ");
        assert_eq!(out, vec!["        if x:", "            body", ""]);
    }
}
