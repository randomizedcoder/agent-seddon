//! Situational system-prompt fragments, selected by a [`PromptContext`].
//!
//! A *fragment* is a small markdown file that is appended to the system prompt when
//! the situation matches it (`docs/design/prompts/`). This resolver handles the
//! **situational** fragments — the ones that carry a tag — not the always-on base
//! (that stays with `agent_prompt::resolve_system_prompt`, so it is never
//! duplicated). In the file backend a fragment's tag comes from its directory:
//!
//! ```text
//! <prompts_dir>/modes/<mode>/NNNN_*.md   ⇒ tag `mode:<mode>`   (dir form; multi-fragment)
//! <prompts_dir>/modes/<mode>.md          ⇒ tag `mode:<mode>`   (single-file form; back-compat)
//! ```
//!
//! [`SystemFragments::select`] returns the concatenation of every fragment whose
//! tags are a subset of the context (`docs/design/prompts/04-selection.md`), ordered
//! by the `NNNN_` filename prefix. Only the `mode:` directory tag is a selector today;
//! frontmatter tags (a wider catalog) land in a later increment.
//!
//! Reads happen **per lookup** (not at construction) so an edit through the
//! `PromptStore` seam is picked up on the next turn with no restart — mirroring
//! [`crate::lens::LensPrompts`]. With no directory (or nothing selected) the lookup
//! is allocation-free (`Cow::Borrowed("")`), so the per-turn cost when the feature is
//! unused is nil and behaviour is byte-identical to today (the gate has no files).

use agent_core::PromptContext;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Resolver for situational system fragments rooted at a `prompts` directory.
pub struct SystemFragments {
    /// The `<prompts_dir>` root, or `None` for a no-op resolver (no I/O).
    root: Option<PathBuf>,
}

impl SystemFragments {
    /// A no-op resolver — no directory, no I/O; `select` always returns `""`.
    pub fn defaults() -> Self {
        Self { root: None }
    }

    /// Resolver rooted at `<prompts_dir>`. An empty/`None` dir ⇒ the no-op resolver.
    pub fn new(prompts_dir: Option<&str>) -> Self {
        let root = prompts_dir.filter(|d| !d.is_empty()).map(PathBuf::from);
        Self { root }
    }

    /// The concatenated situational fragments selected by `ctx`: every `modes/<mode>/`
    /// (or single-file `modes/<mode>.md`) whose `mode:<mode>` tag is present in the
    /// context, in mode-name order, each in `NNNN_` order, joined by a blank line.
    ///
    /// Returns `Cow::Borrowed("")` when nothing is configured or nothing matches, so
    /// the caller adds no message and the request is byte-identical to today.
    pub fn select(&self, ctx: &PromptContext) -> Cow<'static, str> {
        let Some(root) = &self.root else {
            return Cow::Borrowed("");
        };
        let modes_dir = root.join("modes");

        // Every candidate mode name, from a subdirectory or a single `.md` file.
        // Sorted + de-duplicated for deterministic composition.
        let mut names: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&modes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = if path.is_dir() {
                    entry.file_name().into_string().ok()
                } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                } else {
                    None
                };
                if let Some(name) = name {
                    if !name.is_empty() {
                        names.push(name);
                    }
                }
            }
        }
        names.sort();
        names.dedup();

        let mut parts: Vec<String> = Vec::new();
        for name in names {
            // The directory encodes the fragment's tag; the matcher stays tag-opaque.
            let tag = format!("mode:{name}");
            if !ctx.covers([tag.as_str()]) {
                continue;
            }
            // Directory form wins over the single-file form (the resolution ladder).
            let dir = modes_dir.join(&name);
            if dir.is_dir() {
                read_dir_fragments(&dir, &mut parts);
            } else if let Some(s) = read_nonempty(&modes_dir.join(format!("{name}.md"))) {
                parts.push(s);
            }
        }

        if parts.is_empty() {
            Cow::Borrowed("")
        } else {
            Cow::Owned(parts.join("\n\n"))
        }
    }
}

/// Append every non-empty `*.md` fragment in `dir`, ordered by the `NNNN_` numeric
/// prefix (files without one sort last, by name). Best-effort: an unreadable dir or
/// file is skipped silently. Shared with [`crate::lens`] for its directory form.
pub(crate) fn read_dir_fragments(dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut files: Vec<(u64, String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if let Some(content) = read_nonempty(&path) {
            files.push((numeric_prefix(&name), name, content));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    out.extend(files.into_iter().map(|(_, _, content)| content));
}

/// Read a file, returning its trimmed content, or `None` if missing/unreadable/blank.
pub(crate) fn read_nonempty(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Leading run of ASCII digits as an ordering key (files without one sort last),
/// mirroring `agent-prompt`/`context_files` (each crate copies its own — the
/// convention for these small helpers).
fn numeric_prefix(name: &str) -> u64 {
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;

    fn ctx(tags: &[&str]) -> PromptContext {
        let mut c = PromptContext::new();
        for t in tags {
            c.insert(*t);
        }
        c
    }

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    // --- defaults(): no dir ⇒ borrowed empty, no I/O ------------------------
    #[test]
    fn positive_defaults_selects_nothing_borrowed() {
        let sf = SystemFragments::defaults();
        let got = sf.select(&ctx(&["mode:review"]));
        assert!(matches!(got, Cow::Borrowed(_)), "no-op must not allocate");
        assert_eq!(got, "");
    }

    // --- positive_: the matching mode's fragments are selected, in order ----
    #[test]
    fn positive_selects_matching_mode_in_order() {
        let root = tempdir();
        write(&root.join("modes/review/0002_output.md"), "SECOND\n");
        write(&root.join("modes/review/0001_focus.md"), "FIRST\n");
        write(&root.join("modes/debug/0001_method.md"), "DEBUG-ONLY\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));

        let got = sf.select(&ctx(&["mode:review"]));
        assert_eq!(got, "FIRST\n\nSECOND");
        assert!(
            !got.contains("DEBUG-ONLY"),
            "a non-matching mode must not leak"
        );
    }

    // --- positive_: a superset context still selects (fragment.tags ⊆ context)
    #[test]
    fn positive_superset_context_selects() {
        let root = tempdir();
        write(&root.join("modes/review/0001_focus.md"), "REVIEW\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));
        // Extra unrelated tags in the context do not prevent the match.
        assert_eq!(sf.select(&ctx(&["mode:review", "language:rust"])), "REVIEW");
    }

    // --- positive_: the single-file form is honoured (back-compat) ----------
    #[test]
    fn positive_single_file_mode_form() {
        let root = tempdir();
        write(&root.join("modes/explain.md"), "EXPLAIN-FRAGMENT\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));
        assert_eq!(sf.select(&ctx(&["mode:explain"])), "EXPLAIN-FRAGMENT");
    }

    // --- corner_: the directory form wins over the single-file form ---------
    #[test]
    fn corner_dir_form_wins_over_single_file() {
        let root = tempdir();
        write(&root.join("modes/review/0001_focus.md"), "FROM-DIR\n");
        write(&root.join("modes/review.md"), "FROM-FILE\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));
        assert_eq!(sf.select(&ctx(&["mode:review"])), "FROM-DIR");
    }

    // --- negative_: no matching tag ⇒ nothing selected ----------------------
    #[test]
    fn negative_no_match_selects_nothing() {
        let root = tempdir();
        write(&root.join("modes/review/0001_focus.md"), "REVIEW\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));
        let got = sf.select(&ctx(&["mode:debug"]));
        assert_eq!(got, "");
        // Empty context selects nothing situational (only the base, handled elsewhere).
        assert_eq!(sf.select(&ctx(&[])), "");
    }

    // --- negative_: an empty/whitespace fragment file is skipped ------------
    #[test]
    fn negative_blank_fragment_skipped() {
        let root = tempdir();
        write(&root.join("modes/review/0001_blank.md"), "   \n\t\n");
        write(&root.join("modes/review/0002_real.md"), "REAL\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));
        assert_eq!(sf.select(&ctx(&["mode:review"])), "REAL");
    }

    // --- boundary_: a configured-but-missing dir behaves like defaults ------
    #[test]
    fn boundary_missing_dir_selects_nothing() {
        let sf = SystemFragments::new(Some("/nonexistent/prompts/xyz"));
        assert_eq!(sf.select(&ctx(&["mode:review"])), "");
    }

    // --- boundary_: files without a numeric prefix sort last ----------------
    #[test]
    fn boundary_unprefixed_fragment_sorts_last() {
        let root = tempdir();
        write(&root.join("modes/design/notes.md"), "LAST\n");
        write(&root.join("modes/design/0001_frame.md"), "FIRST\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));
        assert_eq!(sf.select(&ctx(&["mode:design"])), "FIRST\n\nLAST");
    }

    // --- adversarial_: a mode dir whose tag is not in the context never leaks
    #[test]
    fn adversarial_unmatched_mode_never_leaks() {
        let root = tempdir();
        // A directory named to look like a traversal is still just a `mode:<name>`
        // tag that the context does not carry, so it selects nothing.
        write(&root.join("modes/review/0001_focus.md"), "SECRET-REVIEW\n");
        let sf = SystemFragments::new(Some(root.to_str().unwrap()));
        let got = sf.select(&ctx(&["mode:..", "mode:/etc/passwd"]));
        assert_eq!(got, "");
        assert!(!got.contains("SECRET-REVIEW"));
    }
}
