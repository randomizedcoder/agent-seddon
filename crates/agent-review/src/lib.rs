//! Code-review flow: task-mode detection + grounded fact collection.
//!
//! Implements the `agent-core` review seams — [`agent_core::TaskClassifier`]
//! ([`HybridClassifier`]) and [`agent_core::ReviewCollector`]
//! ([`ReviewOrchestrator`]). Everything here is deterministic over injected trait
//! objects; the model is only ever asked to *vote* on the task mode, never to
//! supply a fact. See `docs/design/code-review/`.

mod analyzer;
mod callgraph;
mod classifier;
mod collector;
mod orchestrator;
mod repo_facts;
mod signatures;
mod style;
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

    // Changed function signatures — the structural "what APIs moved" fact, read
    // before the findings and the raw hunks.
    render_signatures(&mut out, &facts.signatures);

    // Call graph — the blast radius of the changed functions (who calls them).
    render_callgraph(&mut out, &facts.callgraph);

    // Static-analysis findings — higher-signal than raw hunks, so rendered *before*
    // the diffs. Per-tool run summary, then findings with changed-file hits first.
    render_analysis(&mut out, &facts.analysis);

    // House-style fingerprint — so the review respects the repo's conventions.
    render_style(&mut out, &facts.style);

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

/// Render the changed-signature section, grouped by file: `~` modified (before →
/// after), `+` added, `-` removed. Nothing is emitted if extraction found nothing.
fn render_signatures(out: &mut String, report: &agent_core::SignatureReport) {
    if report.changes.is_empty() {
        return;
    }
    out.push_str(&format!(
        "\nAPI signature changes ({}):\n",
        report.changes.len()
    ));
    let mut last_file = "";
    for c in &report.changes {
        if c.file != last_file {
            out.push_str(&format!("  {}\n", c.file));
            last_file = &c.file;
        }
        match c.kind.as_str() {
            "modified" => {
                out.push_str(&format!("    ~ {}  {}  →  {}\n", c.name, c.before, c.after))
            }
            "added" => out.push_str(&format!("    + {}  {}\n", c.name, c.after)),
            _ => out.push_str(&format!("    - {}  {}\n", c.name, c.before)),
        }
    }
    if report.truncated {
        out.push_str("    … more signature changes omitted (cap reached)\n");
    }
}

/// Render the house-style fingerprint as compact facts (ratios + verdicts). Nothing
/// is emitted if the collector didn't run (`files_scanned == 0`).
fn render_style(out: &mut String, s: &agent_core::StyleFacts) {
    if s.files_scanned == 0 {
        return;
    }
    let indent = if s.indent_tabs { "tabs" } else { "spaces" };
    out.push_str(&format!(
        "\nCode style ({} files sampled):\n",
        s.files_scanned
    ));
    out.push_str(&format!(
        "  indent {} · comment density {:.2} (doc {:.0}%) · line p95 {} · fn median {} lines\n",
        indent,
        s.comment_density,
        s.doccomment_ratio * 100.0,
        s.line_len_p95,
        s.fn_len_median,
    ));
    out.push_str(&format!(
        "  naming: fn {} · var {} · const {} · {:.0}% exported\n",
        s.naming.functions,
        s.naming.variables,
        s.naming.constants,
        s.naming.exported_ratio * 100.0,
    ));
    if s.commits.sampled_commits > 0 {
        out.push_str(&format!(
            "  commits ({} sampled): {:.0}% conventional · subject p50/p95 {}/{} · {:.0}% with body\n",
            s.commits.sampled_commits,
            s.commits.conventional_ratio * 100.0,
            s.commits.subject_len_p50,
            s.commits.subject_len_p95,
            s.commits.body_present_ratio * 100.0,
        ));
    }
    out.push_str(&format!(
        "  change conforms to repo style: {}\n",
        if s.diff_matches_style { "yes" } else { "no" }
    ));
}

/// The most changed functions rendered with their blast radius.
const MAX_BLAST_FNS: usize = 40;
/// The most callers listed per changed function.
const MAX_CALLERS: usize = 8;

/// Render the call graph as a **blast radius**: for each changed function, its
/// direct in-repo callers and its callee count. Nothing is emitted if the graph is
/// empty (helper off/absent, or no Go source changed).
fn render_callgraph(out: &mut String, g: &agent_core::CallGraph) {
    if g.nodes.is_empty() {
        return;
    }
    let by_id: std::collections::HashMap<u32, &agent_core::CallGraphNode> =
        g.nodes.iter().map(|n| (n.id, n)).collect();
    let qual = |n: &agent_core::CallGraphNode| -> String {
        if n.package.is_empty() {
            n.name.clone()
        } else {
            format!("{}.{}", n.package, n.name)
        }
    };

    out.push_str(&format!(
        "\nCall graph — {} fn(s), {} edge(s) across {} package(s); blast radius of {} changed fn(s):\n",
        g.nodes.len(),
        g.edges.len(),
        g.packages.len(),
        g.changed_fns.len(),
    ));
    for id in g.changed_fns.iter().take(MAX_BLAST_FNS) {
        let Some(node) = by_id.get(id) else { continue };
        let callers: Vec<String> = g
            .edges
            .iter()
            .filter(|e| e.callee_id == *id && e.caller_id != *id)
            .filter_map(|e| by_id.get(&e.caller_id))
            .map(|n| qual(n))
            .collect();
        let calls = g.edges.iter().filter(|e| e.caller_id == *id).count();
        if callers.is_empty() {
            out.push_str(&format!(
                "  {}  ← no in-repo callers  · calls {}\n",
                qual(node),
                calls
            ));
        } else {
            let shown: Vec<String> = callers.iter().take(MAX_CALLERS).cloned().collect();
            let more = callers.len().saturating_sub(shown.len());
            let extra = if more > 0 {
                format!(" (+{more} more)")
            } else {
                String::new()
            };
            out.push_str(&format!(
                "  {}  ← called by {}{}  · calls {}\n",
                qual(node),
                shown.join(", "),
                extra,
                calls
            ));
        }
    }
    if g.changed_fns.len() > MAX_BLAST_FNS {
        out.push_str(&format!(
            "  … and {} more changed fn(s) (omitted)\n",
            g.changed_fns.len() - MAX_BLAST_FNS
        ));
    }
    if g.truncated {
        out.push_str("  (graph truncated — size cap reached)\n");
    }
}

/// The most findings rendered verbatim; the rest are summarized as a count.
const MAX_RENDERED_FINDINGS: usize = 80;

/// Render the static-analysis section: a one-line-per-tool run summary, then the
/// findings (changed-file hits first, capped). Nothing is emitted if the analyzer
/// never ran (`analyze = false` ⇒ empty report).
fn render_analysis(out: &mut String, report: &agent_core::AnalysisReport) {
    if report.runs.is_empty() {
        return; // analyzer disabled or not wired — say nothing rather than "0"
    }
    out.push_str("\nAnalysis (static):\n");
    for r in &report.runs {
        let reason = if r.reason.is_empty() {
            String::new()
        } else {
            format!(" — {}", r.reason)
        };
        out.push_str(&format!(
            "  {}: {} ({} finding(s), {} ms){}\n",
            r.tool, r.status, r.finding_count, r.duration_ms, reason,
        ));
    }

    if report.findings.is_empty() {
        return;
    }
    // Changed-file findings first (higher signal), stable within each group.
    let mut ordered: Vec<&agent_core::AnalysisFinding> = report.findings.iter().collect();
    ordered.sort_by_key(|f| !f.in_change);
    out.push_str("Findings:\n");
    for f in ordered.iter().take(MAX_RENDERED_FINDINGS) {
        let scope = if f.in_change { "" } else { " [pre-existing]" };
        out.push_str(&format!(
            "  {} {}/{} {}:{} — {}{}\n",
            f.severity, f.tool, f.rule, f.file, f.line, f.message, scope,
        ));
    }
    if ordered.len() > MAX_RENDERED_FINDINGS {
        out.push_str(&format!(
            "  … and {} more finding(s) (omitted from the listing)\n",
            ordered.len() - MAX_RENDERED_FINDINGS
        ));
    }
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
