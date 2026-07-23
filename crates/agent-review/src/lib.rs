//! Code-review flow: task-mode detection + grounded fact collection.
//!
//! Implements the `agent-core` review seams — [`agent_core::TaskClassifier`]
//! ([`HybridClassifier`]) and [`agent_core::ReviewCollector`]
//! ([`ReviewOrchestrator`]). Everything here is deterministic over injected trait
//! objects; the model is only ever asked to *vote* on the task mode, never to
//! supply a fact. See `docs/design/code-review/`.

mod classifier;
mod collector;
mod orchestrator;
mod repo_facts;
mod util;

pub use classifier::HybridClassifier;
pub use orchestrator::{ReviewEvent, ReviewObserver, ReviewOrchestrator};

use agent_core::ReviewFacts;
use util::is_noisy;

/// The largest number of changed files listed verbatim in a rendered bundle; the
/// rest are summarized as a count (bounded output over an attacker-influenced diff).
const MAX_LISTED_FILES: usize = 200;
/// Default rendered-context byte budget (diff hunks fill what remains after the
/// always-included facts). `0` ⇒ unbounded. Config: `[review] context_budget_bytes`.
pub const DEFAULT_CONTEXT_BUDGET: usize = 24_000;

/// Render a grounded fact bundle as a human/model-readable block — hard facts
/// first, clearly labelled tool-derived, at the default byte budget. Injected as
/// context in-loop, or printed by `agent --review`. Never emits the raw remote URL.
pub fn render_facts(facts: &ReviewFacts) -> String {
    render_facts_with(facts, DEFAULT_CONTEXT_BUDGET)
}

/// Render, capping total size at `budget_bytes` (`0` ⇒ unbounded). The repo line,
/// the commits, and the changed-file list are always included; the **diff hunks**
/// fill the remaining budget, and any that don't fit are omitted with an honest
/// count — so a huge change degrades gracefully instead of flooding the window.
pub fn render_facts_with(facts: &ReviewFacts, budget_bytes: usize) -> String {
    let gs = &facts.git_state;
    let ch = &facts.change;
    let mut out = String::new();
    out.push_str("## Grounded review facts (tool-derived — not model-generated)\n\n");

    // Condensed repo line (identity hash / host / tracked-count live in the struct,
    // not the rendered text — low signal for a human reviewer).
    out.push_str(&format!(
        "Repo: {} · {} · default branch `{}`\n",
        gs.project.as_str(),
        gs.relationship.as_str(),
        if gs.default_branch.is_empty() {
            "unknown"
        } else {
            &gs.default_branch
        },
    ));
    out.push_str(&format!(
        "Change: `{}`..`{}` — {} changed file(s)\n",
        ch.base_rev,
        ch.head_rev,
        ch.files.len(),
    ));

    // Commits (intent). Summary lines + the head commit's body.
    if !ch.commits.is_empty() {
        out.push_str(&format!("\nCommits ({}):\n", ch.commits.len()));
        for c in &ch.commits {
            out.push_str(&format!(
                "  {} {} ({}d, {})\n",
                c.short, c.summary, c.age_days, c.author
            ));
        }
        if let Some(head) = ch.commits.first() {
            if !head.body.is_empty() {
                out.push_str("  ---\n");
                for line in head.body.lines() {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }

    // The changed-file list (always included; bounded count).
    out.push_str("\nFiles:\n");
    for f in ch.files.iter().take(MAX_LISTED_FILES) {
        out.push_str(&format!(
            "  {:<9} {} (+{}/-{}){}\n",
            f.change.serialize_label(),
            f.path.display(),
            f.additions,
            f.deletions,
            file_note(f),
        ));
    }
    if ch.files.len() > MAX_LISTED_FILES {
        out.push_str(&format!(
            "  … and {} more (omitted from the listing)\n",
            ch.files.len() - MAX_LISTED_FILES
        ));
    }

    // Diff hunks — fill the remaining budget; omit (with a count) what doesn't fit.
    let with_patch: Vec<&agent_core::ChangedFile> =
        ch.files.iter().filter(|f| !f.patch.is_empty()).collect();
    if !with_patch.is_empty() {
        out.push_str("\nDiffs:\n");
        let mut omitted = 0usize;
        for f in with_patch {
            let block = format!("### {}\n{}\n", f.path.display(), f.patch);
            if budget_bytes != 0 && out.len() + block.len() > budget_bytes {
                omitted += 1;
                continue;
            }
            out.push_str(&block);
        }
        if omitted > 0 {
            out.push_str(&format!(
                "  … {omitted} file diff(s) omitted (context budget {budget_bytes} bytes)\n"
            ));
        }
    }
    out
}

/// A short note after a file's header explaining an absent diff.
fn file_note(f: &agent_core::ChangedFile) -> &'static str {
    if f.is_binary {
        " [binary]"
    } else if is_noisy(&f.path) {
        " [generated/lockfile — diff omitted]"
    } else {
        ""
    }
}

/// A small display label for a change kind (avoids leaking serde internals).
trait ChangeLabel {
    fn serialize_label(&self) -> &'static str;
}
impl ChangeLabel for agent_core::ChangeKind {
    fn serialize_label(&self) -> &'static str {
        use agent_core::ChangeKind::*;
        match self {
            Added => "added",
            Modified => "modified",
            Deleted => "deleted",
            Renamed => "renamed",
            Copied => "copied",
            TypeChange => "typechange",
        }
    }
}
