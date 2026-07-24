//! `StyleCollector` — a **deterministic** fingerprint of the repo's house style
//! (component 07): comment density, indentation, line length, function length,
//! naming case, and commit-message conventions. Every fact is *counted*, never
//! judged — no model. The point is that a review can respect existing conventions
//! (snake_case in a snake_case repo, sparse comments in a sparse-comment repo)
//! instead of imposing generic taste.
//!
//! Pure computation over the confined file set (`RepoBackend::read_file` at head)
//! and `RepoBackend::log`. Fail-soft and bounded: a missing/binary/huge file is
//! skipped, the commit sample and per-file work are capped, and a facet with no
//! data reads `unknown` rather than a guess. Distributions and ratios only — no
//! source, paths, or identifiers are retained.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{is_noisy, lang_of};
use agent_core::{CommitStyleFacts, NamingFacts, Revision, StyleFacts};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Baseline file sample cap (representative, bounded blob reads).
const MAX_FILES: usize = 200;
/// Skip a blob larger than this (a minified/generated monster mustn't dominate).
const MAX_BLOB_BYTES: u64 = 1_000_000;
/// Longest line length tracked in the histogram (longer clamps to this).
const MAX_LINE: usize = 500;

pub(crate) struct StyleCollector {
    pub commit_sample: usize,
}

#[async_trait::async_trait]
impl FactCollector for StyleCollector {
    fn name(&self) -> &'static str {
        "style"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        // The repo's source file set (index-backed, else an index-free scan) — the
        // baseline the house style is measured over.
        let files = file_paths(ctx).await;
        let mut baseline = Acc::default();
        let mut scanned = 0u32;
        for path in files.into_iter().filter(|p| is_source(p)).take(MAX_FILES) {
            if let Some(text) = read_text(ctx, &ctx.head, &path).await {
                let lang = lang_of(&path);
                baseline.scan(&text, &lang);
                scanned += 1;
            }
        }

        // The changed files' style, to answer "does the PR follow the repo's own
        // conventions?" (recompute the cached diff — collectors run in parallel).
        let mut change = Acc::default();
        if let Ok(d) = ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            for f in d.files {
                let Some(path) = f.new_path.or(f.old_path) else {
                    continue;
                };
                if !is_source(&path) {
                    continue;
                }
                if let Some(text) = read_text(ctx, &ctx.head, &path).await {
                    change.scan(&text, &lang_of(&path));
                }
            }
        }

        if scanned == 0 {
            return CollectorOutput::skipped("no readable source files");
        }

        let mut facts = baseline.finish();
        facts.files_scanned = scanned;
        facts.commits = commit_style(ctx, self.commit_sample).await;
        // Conformance: benefit of the doubt unless we have both sides and they
        // disagree on indentation or function-naming.
        facts.diff_matches_style = match change.finish_if_seen() {
            Some(cs) => {
                cs.indent_tabs == facts.indent_tabs && cs.naming.functions == facts.naming.functions
            }
            None => true,
        };

        CollectorOutput::ok(FactFragment::Style { facts })
    }
}

/// The repo's file set: the search index when present (fast), else an index-free
/// `Manifest::scan` off the async path. Mirrors `repo_facts::file_paths`.
async fn file_paths(ctx: &CollectCtx) -> Vec<PathBuf> {
    if let Some(search) = &ctx.search {
        if let Ok(files) = search.list_files(&[]).await {
            if !files.is_empty() {
                return files;
            }
        }
    }
    let root = ctx.repo_root.clone();
    tokio::task::spawn_blocking(move || {
        agent_search::Manifest::scan(&root)
            .entries
            .into_keys()
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

async fn read_text(ctx: &CollectCtx, rev: &Revision, path: &Path) -> Option<String> {
    match ctx.repo.read_file(rev, path).await {
        Ok(b) if !b.is_binary && b.bytes_len <= MAX_BLOB_BYTES => Some(b.text),
        _ => None,
    }
}

/// A source file worth fingerprinting (a known language, not a lockfile/generated).
fn is_source(path: &Path) -> bool {
    if is_noisy(path) {
        return false;
    }
    matches!(
        lang_of(path).as_str(),
        "go" | "rust" | "python" | "typescript" | "javascript" | "c" | "cpp" | "java"
    )
}

/// A running tally over one or more files. Line lengths and function lengths use
/// fixed-size histograms so hostile input can't blow up memory.
struct Acc {
    code_lines: u64,
    comment_lines: u64,
    leading_comments: u64,
    tab_indents: u64,
    space_indents: u64,
    line_hist: [u64; MAX_LINE + 1],
    fn_hist: Vec<u32>,
    // Case tallies: [camel, pascal, snake, screaming_snake, mixed].
    fn_case: [u64; 5],
    var_case: [u64; 5],
    const_case: [u64; 5],
    fn_total: u64,
    fn_exported: u64,
    seen: bool,
}

impl Default for Acc {
    fn default() -> Self {
        Acc {
            code_lines: 0,
            comment_lines: 0,
            leading_comments: 0,
            tab_indents: 0,
            space_indents: 0,
            line_hist: [0; MAX_LINE + 1],
            fn_hist: Vec::new(),
            fn_case: [0; 5],
            var_case: [0; 5],
            const_case: [0; 5],
            fn_total: 0,
            fn_exported: 0,
            seen: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Code,
    Comment,
    Blank,
}

impl Acc {
    fn scan(&mut self, text: &str, lang: &str) {
        self.seen = true;
        let line_tok = line_comment(lang);
        let block = block_comment(lang);
        let lines: Vec<&str> = text.lines().collect();

        // Classify every line (comment / code / blank), tracking block-comment state.
        let mut kinds = Vec::with_capacity(lines.len());
        let mut in_block = false;
        for raw in &lines {
            let t = raw.trim_start();
            let kind = if in_block {
                if block.map(|(_, close)| t.contains(close)).unwrap_or(false) {
                    in_block = false;
                }
                Kind::Comment
            } else if t.is_empty() {
                Kind::Blank
            } else if line_tok.map(|tok| t.starts_with(tok)).unwrap_or(false) {
                Kind::Comment
            } else if let Some((open, close)) = block {
                if t.starts_with(open) {
                    if !t.contains(close) {
                        in_block = true;
                    }
                    Kind::Comment
                } else {
                    Kind::Code
                }
            } else {
                Kind::Code
            };
            kinds.push(kind);
        }

        for (i, (raw, &kind)) in lines.iter().zip(&kinds).enumerate() {
            match kind {
                Kind::Blank => {}
                Kind::Comment => {
                    self.comment_lines += 1;
                    // A "leading" (doc) comment is one whose next non-blank line is code.
                    if next_nonblank(&kinds, i + 1) == Some(Kind::Code) {
                        self.leading_comments += 1;
                    }
                }
                Kind::Code => {
                    self.code_lines += 1;
                    let len = raw.chars().count().min(MAX_LINE);
                    self.line_hist[len] += 1;
                    match raw.chars().next() {
                        Some('\t') => self.tab_indents += 1,
                        Some(' ') => self.space_indents += 1,
                        _ => {}
                    }
                }
            }
        }

        self.scan_names(text, lang);
        self.scan_fn_lengths(text, lang);
    }

    fn scan_names(&mut self, text: &str, lang: &str) {
        for (name, exported) in functions(text, lang) {
            tally(&mut self.fn_case, &name);
            self.fn_total += 1;
            if exported {
                self.fn_exported += 1;
            }
        }
        for name in variables(text, lang) {
            tally(&mut self.var_case, &name);
        }
        for name in constants(text, lang) {
            tally(&mut self.const_case, &name);
        }
    }

    /// Brace-balanced function-body line spans (Go/Rust only) → the length histogram.
    fn scan_fn_lengths(&mut self, text: &str, lang: &str) {
        if lang != "go" && lang != "rust" {
            return;
        }
        let re = fn_decl_re(lang);
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if re.is_match(lines[i]) {
                // Walk to the opening brace, then to the matching close.
                let mut depth = 0i32;
                let mut opened = false;
                let mut j = i;
                while j < lines.len() {
                    for ch in lines[j].chars() {
                        match ch {
                            '{' => {
                                depth += 1;
                                opened = true;
                            }
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                    if opened && depth <= 0 {
                        break;
                    }
                    j += 1;
                    if j - i > 5000 {
                        break; // runaway guard
                    }
                }
                if opened {
                    self.fn_hist.push((j - i + 1).min(u32::MAX as usize) as u32);
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
    }

    fn finish(&self) -> StyleFacts {
        StyleFacts {
            comment_density: ratio(self.comment_lines, self.code_lines),
            doccomment_ratio: ratio(self.leading_comments, self.comment_lines),
            indent_tabs: self.tab_indents >= self.space_indents && self.tab_indents > 0,
            line_len_p95: percentile_hist(&self.line_hist, 95),
            fn_len_median: median(&self.fn_hist),
            naming: NamingFacts {
                functions: majority(&self.fn_case),
                variables: majority(&self.var_case),
                constants: majority(&self.const_case),
                exported_ratio: ratio(self.fn_exported, self.fn_total),
            },
            commits: CommitStyleFacts::default(),
            diff_matches_style: false,
            files_scanned: 0,
        }
    }

    fn finish_if_seen(&self) -> Option<StyleFacts> {
        self.seen.then(|| self.finish())
    }
}

fn ratio(num: u64, den: u64) -> f32 {
    if den == 0 {
        0.0
    } else {
        (num as f64 / den as f64) as f32
    }
}

fn next_nonblank(kinds: &[Kind], from: usize) -> Option<Kind> {
    kinds[from..].iter().copied().find(|k| *k != Kind::Blank)
}

/// The p-th percentile line length from the fixed-size histogram.
fn percentile_hist(hist: &[u64], p: u64) -> u32 {
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return 0;
    }
    let target = (total * p).div_ceil(100);
    let mut cum = 0u64;
    for (len, &count) in hist.iter().enumerate() {
        cum += count;
        if cum >= target {
            return len as u32;
        }
    }
    (hist.len() - 1) as u32
}

fn median(vals: &[u32]) -> u32 {
    if vals.is_empty() {
        return 0;
    }
    let mut v = vals.to_vec();
    v.sort_unstable();
    v[v.len() / 2]
}

// ---- case classification -------------------------------------------------------

const CAMEL: usize = 0;
const PASCAL: usize = 1;
const SNAKE: usize = 2;
const SCREAMING: usize = 3;
const MIXED: usize = 4;

fn tally(counts: &mut [u64; 5], name: &str) {
    counts[case_of(name)] += 1;
}

/// Classify an identifier's case. Coarse but stable under majority voting.
fn case_of(name: &str) -> usize {
    let letters: String = name.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return MIXED;
    }
    let has_upper = letters.chars().any(|c| c.is_uppercase());
    let has_lower = letters.chars().any(|c| c.is_lowercase());
    let first_upper = name
        .chars()
        .find(|c| c.is_alphabetic())
        .map(|c| c.is_uppercase())
        .unwrap_or(false);
    if name.contains('_') {
        if !has_upper {
            SNAKE
        } else if !has_lower {
            SCREAMING
        } else {
            MIXED
        }
    } else if !has_upper {
        SNAKE // single lowercase word (ambiguous camel/snake; smoothed by voting)
    } else if first_upper {
        if has_lower {
            PASCAL
        } else {
            SCREAMING
        }
    } else {
        CAMEL
    }
}

fn majority(counts: &[u64; 5]) -> String {
    let (idx, &max) = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .unwrap_or((MIXED, &0));
    if max == 0 {
        return "unknown".into();
    }
    match idx {
        CAMEL => "camel",
        PASCAL => "pascal",
        SNAKE => "snake",
        SCREAMING => "screaming_snake",
        _ => "mixed",
    }
    .into()
}

// ---- per-language token patterns -----------------------------------------------

fn line_comment(lang: &str) -> Option<&'static str> {
    match lang {
        "python" => Some("#"),
        "go" | "rust" | "c" | "cpp" | "java" | "typescript" | "javascript" => Some("//"),
        _ => None,
    }
}

fn block_comment(lang: &str) -> Option<(&'static str, &'static str)> {
    match lang {
        "go" | "rust" | "c" | "cpp" | "java" | "typescript" | "javascript" => Some(("/*", "*/")),
        _ => None,
    }
}

fn functions(text: &str, lang: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    match lang {
        "go" => {
            for cap in go_fn_re().captures_iter(text) {
                if let Some(m) = cap.name("name") {
                    let name = m.as_str().to_string();
                    let exported = name
                        .chars()
                        .next()
                        .map(|c| c.is_uppercase())
                        .unwrap_or(false);
                    out.push((name, exported));
                }
            }
        }
        "rust" => {
            for cap in rust_fn_re().captures_iter(text) {
                if let Some(m) = cap.name("name") {
                    out.push((m.as_str().to_string(), cap.name("pub").is_some()));
                }
            }
        }
        _ => {}
    }
    out
}

fn variables(text: &str, lang: &str) -> Vec<String> {
    let re = match lang {
        "go" => go_var_re(),
        "rust" => rust_var_re(),
        _ => return Vec::new(),
    };
    re.captures_iter(text)
        .filter_map(|c| c.name("name").map(|m| m.as_str().to_string()))
        .collect()
}

fn constants(text: &str, lang: &str) -> Vec<String> {
    let re = match lang {
        "go" => go_const_re(),
        "rust" => rust_const_re(),
        _ => return Vec::new(),
    };
    re.captures_iter(text)
        .filter_map(|c| c.name("name").map(|m| m.as_str().to_string()))
        .collect()
}

fn fn_decl_re(lang: &str) -> &'static regex::Regex {
    if lang == "go" {
        go_fn_re()
    } else {
        rust_fn_re()
    }
}

macro_rules! re {
    ($f:ident, $p:expr) => {
        fn $f() -> &'static regex::Regex {
            static RE: OnceLock<regex::Regex> = OnceLock::new();
            RE.get_or_init(|| regex::Regex::new($p).expect("valid regex"))
        }
    };
}

re!(
    go_fn_re,
    r"(?m)^func\s+(?:\([^)]*\)\s*)?(?P<name>[A-Za-z_]\w*)"
);
re!(
    rust_fn_re,
    r"(?m)^\s*(?P<pub>pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+\x22[^\x22]*\x22\s+)?fn\s+(?P<name>[A-Za-z_]\w*)"
);
re!(
    go_var_re,
    r"(?m)(?:^\s*var\s+|\b)(?P<name>[a-zA-Z_]\w*)\s*:="
);
re!(
    rust_var_re,
    r"(?m)\blet\s+(?:mut\s+)?(?P<name>[a-zA-Z_]\w*)"
);
re!(go_const_re, r"(?m)^\s*const\s+(?P<name>[A-Za-z_]\w*)");
re!(
    rust_const_re,
    r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+(?:mut\s+)?(?P<name>[A-Za-z_]\w*)"
);

// ---- commit-message conventions ------------------------------------------------

async fn commit_style(ctx: &CollectCtx, sample: usize) -> CommitStyleFacts {
    let sample = sample.clamp(1, 200);
    let commits = match ctx.repo.log(&ctx.head, None, sample).await {
        Ok(c) => c,
        Err(_) => return CommitStyleFacts::default(),
    };
    if commits.is_empty() {
        return CommitStyleFacts::default();
    }
    let n = commits.len() as u32;
    let mut conventional = 0u64;
    let mut body_present = 0u64;
    let mut lens: Vec<u32> = Vec::with_capacity(commits.len());
    for c in &commits {
        if conventional_re().is_match(&c.summary) {
            conventional += 1;
        }
        if !c.body.trim().is_empty() {
            body_present += 1;
        }
        lens.push(c.summary.chars().count().min(u32::MAX as usize) as u32);
    }
    lens.sort_unstable();
    CommitStyleFacts {
        conventional_ratio: ratio(conventional, n as u64),
        subject_len_p50: pct(&lens, 50),
        subject_len_p95: pct(&lens, 95),
        body_present_ratio: ratio(body_present, n as u64),
        sampled_commits: n,
    }
}

fn pct(sorted: &[u32], p: usize) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() * p).div_ceil(100)).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

re!(conventional_re, r"^[a-z][a-z0-9]*(?:\([^)]*\))?!?:\s");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_go_fingerprint_comments_indent_naming() {
        let src = "package p\n\n// Doc for Foo.\nfunc Foo() int {\n\treturn 1\n}\n\nfunc bar() {\n\thelperThing()\n}\n";
        let mut a = Acc::default();
        a.scan(src, "go");
        let f = a.finish();
        assert!(f.indent_tabs, "tab-indented bodies");
        assert!(f.comment_density > 0.0);
        // Foo (pascal/exported) + bar (snake single word) → tally has both.
        assert!(!f.naming.functions.is_empty());
        assert!(
            (f.naming.exported_ratio - 0.5).abs() < 0.01,
            "1 of 2 exported"
        );
    }

    #[test]
    fn positive_case_classification() {
        assert_eq!(case_of("fooBar"), CAMEL);
        assert_eq!(case_of("FooBar"), PASCAL);
        assert_eq!(case_of("foo_bar"), SNAKE);
        assert_eq!(case_of("FOO_BAR"), SCREAMING);
        assert_eq!(case_of("Foo_bar"), MIXED);
    }

    #[test]
    fn positive_percentile_and_median() {
        let mut hist = [0u64; MAX_LINE + 1];
        for len in [10u64, 20, 30, 40, 100] {
            hist[len as usize] += 1;
        }
        assert_eq!(percentile_hist(&hist, 95), 100);
        assert_eq!(median(&[3, 1, 2]), 2);
        assert_eq!(median(&[]), 0);
    }

    #[test]
    fn positive_rust_naming_and_fn_length() {
        let src = "pub fn do_thing(x: u32) -> u32 {\n    let count = x;\n    count\n}\n\nfn helper() {}\n";
        let mut a = Acc::default();
        a.scan(src, "rust");
        let f = a.finish();
        assert_eq!(f.naming.functions, "snake");
        assert!((f.naming.exported_ratio - 0.5).abs() < 0.01);
        assert!(f.fn_len_median >= 1);
        assert!(!f.indent_tabs, "space-indented");
    }

    #[test]
    fn corner_conventional_commit_detection() {
        assert!(conventional_re().is_match("feat(review): add style"));
        assert!(conventional_re().is_match("fix: bug"));
        assert!(!conventional_re().is_match("Add a thing"));
        assert!(!conventional_re().is_match("WIP"));
    }

    #[test]
    fn adversarial_hostile_lines_bounded_no_oom() {
        // A single 10 MB line must clamp into the histogram, not allocate per-char.
        let huge = "x".repeat(10_000_000);
        let src = format!("fn f() {{\nlet a = 1;\n{huge}\n}}\n");
        let mut a = Acc::default();
        a.scan(&src, "rust");
        let f = a.finish();
        assert_eq!(f.line_len_p95, MAX_LINE as u32, "clamped to the cap");
    }

    #[test]
    fn boundary_empty_and_unknown() {
        let a = Acc::default();
        let f = a.finish();
        assert_eq!(f.comment_density, 0.0);
        assert_eq!(f.naming.functions, "unknown");
        assert_eq!(f.line_len_p95, 0);
        assert!(a.finish_if_seen().is_none(), "never scanned ⇒ None");
    }
}
