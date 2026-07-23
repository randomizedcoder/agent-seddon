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

/// The largest number of changed files listed verbatim in a rendered bundle; the
/// rest are summarized as a count (bounded output over an attacker-influenced diff).
const MAX_LISTED_FILES: usize = 100;

/// Render a grounded fact bundle as a human/model-readable block, hard facts
/// first and clearly labelled tool-derived. Injected as context in-loop, or
/// printed by `agent review`. Never emits the raw remote URL (only its hash).
pub fn render_facts(facts: &ReviewFacts) -> String {
    let gs = &facts.git_state;
    let ch = &facts.change;
    let mut out = String::new();
    out.push_str("## Grounded review facts (tool-derived — not model-generated)\n\n");

    out.push_str(&format!(
        "Repo: {} project · {} · host {} · default branch `{}` · identity {}\n",
        gs.project.as_str(),
        gs.relationship.as_str(),
        gs.host.as_str(),
        if gs.default_branch.is_empty() {
            "unknown"
        } else {
            &gs.default_branch
        },
        if facts.meta.repo_hash.is_empty() {
            "unknown"
        } else {
            &facts.meta.repo_hash
        },
    ));

    out.push_str(&format!(
        "Change: `{}`..`{}` — {} changed file(s) of {} tracked\n",
        ch.base_rev,
        ch.head_rev,
        ch.files.len(),
        ch.repo_file_count,
    ));

    for f in ch.files.iter().take(MAX_LISTED_FILES) {
        out.push_str(&format!(
            "  {:<9} {} (+{}/-{}){}\n",
            f.change.serialize_label(),
            f.path.display(),
            f.additions,
            f.deletions,
            if f.is_binary { " [binary]" } else { "" },
        ));
    }
    if ch.files.len() > MAX_LISTED_FILES {
        out.push_str(&format!(
            "  … and {} more (omitted from the listing)\n",
            ch.files.len() - MAX_LISTED_FILES
        ));
    }

    out.push_str(&format!("Collection: {} ms — ", facts.meta.total_ms));
    let statuses: Vec<String> = facts
        .meta
        .collectors
        .iter()
        .map(|c| format!("{}={}({}ms)", c.collector, c.status.as_str(), c.duration_ms))
        .collect();
    out.push_str(&statuses.join(", "));
    out.push('\n');
    out
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
