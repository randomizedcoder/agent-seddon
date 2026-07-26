//! Externalized per-mode compaction lens instructions.
//!
//! The destination-mode lens tells the summarizer what to keep and what to drop
//! for the [`TaskMode`] being entered (adaptive-cognition 02). These were once
//! compiled `&'static str` constants; they are now *editable data*. An operator
//! file at `<prompts_dir>/lens/<mode>.md` **overrides** the compiled default, so
//! the prompts are viewable and editable (through the `PromptStore` seam) — while
//! behaviour is unchanged out of the box: with no files present the compiled
//! defaults are used byte-for-byte, so `nix flake check` (which has no operator
//! files) stays green.
//!
//! The instruction remains a **fixed, non-model-derived** string: whether it comes
//! from a compiled default or an operator file, it is written by a human, never by
//! the model or the repo under analysis. The summarizer *output* is still screened
//! by the caller ([`crate::ModeAwareWindow`]) before it re-enters context.
//!
//! An override may be a single file (`lens/<mode>.md`, the shipped shape) or a
//! **directory** of ordered fragments (`lens/<mode>/NNNN_*.md`), the same
//! multi-fragment form the situational system prompts use (`docs/design/prompts/`);
//! the directory form wins over the single file, which wins over the compiled default.

use agent_core::TaskMode;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Every [`TaskMode`], for iteration (there is no variant-iteration helper on the
/// enum). Order is the management-surface listing order.
pub const ALL_MODES: [TaskMode; 6] = [
    TaskMode::Implement,
    TaskMode::Debug,
    TaskMode::Review,
    TaskMode::Design,
    TaskMode::Explain,
    TaskMode::Other,
];

/// The mode-agnostic compaction instruction — the ordinary (non-switch) summary,
/// and the `Other`-mode lens default. `summarizing.rs` re-exports this as
/// `DEFAULT_INSTRUCTION` so there is a single source of truth.
pub const GENERIC: &str =
    "You compress conversation history. Summarize the excerpt below concisely, \
     preserving key facts, decisions, file paths, and tool outcomes. Output only \
     the summary.";

const IMPLEMENT: &str =
    "Summarize the earlier conversation for an IMPLEMENTATION phase. Keep the chosen \
     approach, the target file paths, and the goal. Drop exploration dead-ends, rejected \
     files, and raw directory/grep listings already acted on. Output only the summary.";

const DEBUG: &str =
    "Summarize the earlier conversation for DEBUGGING. Keep the failing test or error, the \
     most recent changes, and the goal. Drop verbose build logs and superseded output. \
     Output only the summary.";

const REVIEW: &str =
    "Summarize the earlier conversation for a CODE REVIEW. Keep the change and its intent \
     and the changed-file set. Drop the step-by-step build process and intermediate \
     broken states. Output only the summary.";

const DESIGN: &str =
    "Summarize the earlier conversation for a DESIGN phase. Keep the constraints, the goal, \
     and the decisions made. Drop low-level implementation detail and rejected \
     alternatives. Output only the summary.";

const EXPLAIN: &str =
    "Summarize the earlier conversation to EXPLAIN what was done. Keep the goal and the \
     answer-relevant facts. Drop tool noise and process detail. Output only the summary.";

/// The compiled default lens for a mode — the fallback when no operator file exists.
/// Keying on the *destination* mode is the "useful in one mode, noise in the next"
/// rule from the before/after table (docs/design/adaptive-cognition/02-compaction.md).
pub fn builtin_instruction(mode: TaskMode) -> &'static str {
    match mode {
        TaskMode::Implement => IMPLEMENT,
        TaskMode::Debug => DEBUG,
        TaskMode::Review => REVIEW,
        TaskMode::Design => DESIGN,
        TaskMode::Explain => EXPLAIN,
        TaskMode::Other => GENERIC,
    }
}

/// Per-mode lens resolver: an operator file override, else the compiled default.
///
/// Reads happen *per lookup* (not at construction) so an edit through `PromptStore`
/// is picked up by the next switch-compaction with no restart. When no directory is
/// configured the lookup is allocation-free (`Cow::Borrowed`), which keeps the
/// default path off the heap for the mode-partition bench + leak budget.
pub struct LensPrompts {
    /// `<prompts_dir>/lens`, or `None` for defaults-only (no I/O).
    lens_dir: Option<PathBuf>,
}

impl LensPrompts {
    /// Defaults-only resolver — no directory, no I/O.
    pub fn defaults() -> Self {
        Self { lens_dir: None }
    }

    /// Resolver rooted at `<prompts_dir>/lens`. An empty/`None` dir ⇒ defaults-only.
    pub fn new(prompts_dir: Option<&str>) -> Self {
        let lens_dir = prompts_dir
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join("lens"));
        Self { lens_dir }
    }

    /// The instruction for `mode`, resolving in order: the **directory** form
    /// `lens/<mode>/NNNN_*.md` (concatenated, `docs/design/prompts/`), else the
    /// shipped **single-file** form `lens/<mode>.md`, else the compiled default.
    /// Best-effort — an unreadable/empty file falls back silently.
    pub fn instruction(&self, mode: TaskMode) -> Cow<'static, str> {
        if let Some(dir) = &self.lens_dir {
            // Directory form: multiple ordered fragments compose one instruction.
            let mode_dir = dir.join(mode.as_str());
            if mode_dir.is_dir() {
                let mut parts = Vec::new();
                crate::system_fragments::read_dir_fragments(&mode_dir, &mut parts);
                if !parts.is_empty() {
                    return Cow::Owned(parts.join("\n\n"));
                }
            }
            // Single-file form (the shipped shape).
            if let Some(s) =
                crate::system_fragments::read_nonempty(&dir.join(format!("{}.md", mode.as_str())))
            {
                return Cow::Owned(s);
            }
        }
        Cow::Borrowed(builtin_instruction(mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    // --- builtin_instruction: one distinctive marker per mode --------------
    #[rstest]
    #[case::positive_implement(TaskMode::Implement, "IMPLEMENTATION")]
    #[case::positive_debug(TaskMode::Debug, "DEBUGGING")]
    #[case::positive_review(TaskMode::Review, "CODE REVIEW")]
    #[case::positive_design(TaskMode::Design, "DESIGN")]
    #[case::positive_explain(TaskMode::Explain, "EXPLAIN")]
    #[case::corner_other_is_generic(TaskMode::Other, "compress conversation history")]
    fn builtin_instruction_markers(#[case] mode: TaskMode, #[case] marker: &str) {
        assert!(builtin_instruction(mode).contains(marker));
    }

    // --- defaults(): no dir ⇒ borrowed compiled default, no I/O ------------
    #[test]
    fn positive_defaults_returns_builtin_borrowed() {
        let lens = LensPrompts::defaults();
        for mode in ALL_MODES {
            let got = lens.instruction(mode);
            assert!(matches!(got, Cow::Borrowed(_)), "default must not allocate");
            assert_eq!(got, Cow::Borrowed(builtin_instruction(mode)));
        }
    }

    // --- override file wins over the compiled default ----------------------
    #[test]
    fn positive_override_file_wins() {
        let root = tempdir();
        let lens_dir = root.join("lens");
        std::fs::create_dir_all(&lens_dir).unwrap();
        std::fs::write(lens_dir.join("debug.md"), "CUSTOM DEBUG LENS\n").unwrap();
        let lens = LensPrompts::new(Some(root.to_str().unwrap()));
        assert_eq!(lens.instruction(TaskMode::Debug), "CUSTOM DEBUG LENS");
        // A mode without a file still gets its compiled default.
        assert!(lens.instruction(TaskMode::Review).contains("CODE REVIEW"));
    }

    // --- negative_: empty/whitespace override falls back to the default -----
    #[test]
    fn negative_empty_override_falls_back() {
        let root = tempdir();
        let lens_dir = root.join("lens");
        std::fs::create_dir_all(&lens_dir).unwrap();
        std::fs::write(lens_dir.join("implement.md"), "   \n  ").unwrap();
        let lens = LensPrompts::new(Some(root.to_str().unwrap()));
        assert!(lens
            .instruction(TaskMode::Implement)
            .contains("IMPLEMENTATION"));
    }

    // --- boundary_: a configured-but-missing dir behaves like defaults ------
    #[test]
    fn boundary_missing_dir_uses_defaults() {
        let lens = LensPrompts::new(Some("/nonexistent/prompts/xyz"));
        assert!(lens.instruction(TaskMode::Design).contains("DESIGN"));
    }

    // --- directory form: lens/<mode>/NNNN_*.md compose, and win over single-file
    #[test]
    fn positive_dir_form_composes_and_wins() {
        let root = tempdir();
        let review_dir = root.join("lens/review");
        std::fs::create_dir_all(&review_dir).unwrap();
        std::fs::write(review_dir.join("0001_a.md"), "FIRST\n").unwrap();
        std::fs::write(review_dir.join("0002_b.md"), "SECOND\n").unwrap();
        // A single-file form for the same mode must lose to the directory.
        std::fs::write(root.join("lens/review.md"), "FROM-FILE\n").unwrap();
        let lens = LensPrompts::new(Some(root.to_str().unwrap()));
        assert_eq!(lens.instruction(TaskMode::Review), "FIRST\n\nSECOND");
        // A mode with neither form still gets its compiled default.
        assert!(lens.instruction(TaskMode::Debug).contains("DEBUGGING"));
    }

    // --- an empty directory form falls back to the single-file / default ----
    #[test]
    fn negative_empty_dir_form_falls_back_to_single_file() {
        let root = tempdir();
        std::fs::create_dir_all(root.join("lens/debug")).unwrap(); // dir exists, no fragments
        std::fs::write(root.join("lens/debug.md"), "SINGLE DEBUG\n").unwrap();
        let lens = LensPrompts::new(Some(root.to_str().unwrap()));
        assert_eq!(lens.instruction(TaskMode::Debug), "SINGLE DEBUG");
    }
}
