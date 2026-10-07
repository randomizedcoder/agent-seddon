//! doc-links — the first-party documentation link checker.
//!
//! Walks the first-party Markdown (`docs/` + the root `README.md` / `DESIGN.md` /
//! `CLAUDE.md`) and reports every **relative** link whose target does not exist, so a
//! rename like `server.rs` → `server/mod.rs` or `sqlite.rs` → `store.rs` cannot silently
//! rot the docs (docs/gap-analysis/README.md §9.2). The constants-sync / buf duality: the
//! SAME entrypoint (`crate::find_broken_links`) backs both `nix run .#doc-links` (report)
//! and the `doc-links` gate (`--gate` → non-zero on any finding).
//!
//! Scope decisions, each one deliberate so the gate neither misses a real break nor fails
//! on something it does not own:
//!
//! - **Only relative links are checked.** `http(s)` / `mailto` / `tel` / `#anchor` /
//!   protocol-relative `//` targets are skipped — reachability of the live web is not a
//!   build concern.
//! - **Links that escape the repo root are external, not broken.** The `docs/parity/`
//!   specs deliberately cite sibling peer-clone checkouts (`../../../codex` / `../../../pi`);
//!   those live outside this repo and outside the hermetic nix sandbox, so they are
//!   classified [`Class::External`] and never fail the gate.
//! - **Anchors are not resolved** — `file.md#section` is checked as `file.md` only (anchor
//!   validation is a separate, larger feature).
//! - **Fenced code blocks are skipped** so an illustrative `[x](y)` inside a ``` fence is
//!   never mistaken for a live link.
//!
//! The link text the extractor ingests is untrusted (a doc is attacker-authorable), so the
//! classifier is purely lexical + a read-only `exists` probe: a traversal target resolves to
//! a path that escapes the root (→ `external`), never an out-of-tree read or a panic. See the
//! `adversarial_` cases.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

/// The first-party Markdown we own. Vendored eval-tool trees (`test/swebench`,
/// `test/inspect`, …) carry their own upstream links and are intentionally out of scope.
pub const SCAN_ROOTS: &[&str] = &["docs", "README.md", "DESIGN.md", "CLAUDE.md"];

// Inline link / image: `[text](target)` and `![alt](target)` (the image's `!` is outside
// the capture, so the same pattern catches both). `target` is everything up to the first
// `)`; a trailing `"title"` is stripped by `normalize_target`.
static INLINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!?\[[^\]]*\]\(([^)]+)\)").unwrap());
// Reference-style definition at line start: `[label]: target "title"`.
static REFDEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\[[^\]]+\]:\s*(\S+)").unwrap());
// A URI scheme prefix — `http:`, `mailto:`, `tel:`, `file:`, … (skip: not a build concern).
static SCHEME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").unwrap());

/// How a normalized link target resolves against the repo root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Escapes the repo root — a peer-clone citation; never a finding.
    External,
    /// An in-repo path that exists.
    Present,
    /// An in-repo path that is missing — the rot a rename leaves behind.
    Broken,
}

/// A missing in-repo link: the Markdown file, 1-indexed line, the normalized target, and
/// the repo-root-relative path that did not exist. Ordered by `(file, line, target,
/// resolved)` so `find_broken_links` output is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub file: String,
    pub line: usize,
    pub target: String,
    pub resolved: String,
}

/// Yield raw link targets on one line (inline links first, then a ref-definition).
fn targets_in_line(line: &str) -> Vec<String> {
    let mut out: Vec<String> = INLINE
        .captures_iter(line)
        .map(|c| c[1].to_string())
        .collect();
    if let Some(c) = REFDEF.captures(line) {
        out.push(c[1].to_string());
    }
    out
}

/// Return `[(lineno, target), …]` for every relative link outside fenced code.
///
/// `lineno` is 1-indexed. Fenced blocks (``` or `~~~`) are skipped wholesale so example
/// links inside them are not treated as live.
#[must_use]
pub fn extract_links(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None; // the fence marker that opened the current block
    for (i, raw) in text.split('\n').enumerate() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        let stripped = raw.trim_start();
        if let Some(marker) = fence {
            if stripped.starts_with(marker) {
                fence = None;
            }
            continue;
        }
        if stripped.starts_with("```") {
            fence = Some("```");
            continue;
        }
        if stripped.starts_with("~~~") {
            fence = Some("~~~");
            continue;
        }
        for target in targets_in_line(raw) {
            out.push((i + 1, target));
        }
    }
    out
}

/// Reduce a raw link target to the filesystem path to check, or `None` to skip.
///
/// Strips `<>` wrappers, a trailing `"title"`, and a `#anchor`; returns `None` for empty,
/// inline-code, scheme, protocol-relative and pure-anchor targets.
#[must_use]
pub fn normalize_target(target: &str) -> Option<String> {
    let mut t = target.trim();
    if t.starts_with('<') && t.ends_with('>') {
        t = t[1..t.len() - 1].trim();
    }
    // A title after whitespace: `path "Title"` — keep only the path token.
    let t = t.split_whitespace().next().unwrap_or("");
    if t.is_empty() {
        return None;
    }
    if t.starts_with('`') {
        // an inline-code span in prose, not a link destination
        return None;
    }
    if t.starts_with('#') {
        // same-document anchor
        return None;
    }
    if t.starts_with("//") {
        // protocol-relative URL
        return None;
    }
    if SCHEME.is_match(t) {
        // http:, mailto:, tel:, …
        return None;
    }
    // Drop the anchor; keep the path (empty only if it was `#frag`, handled above).
    let path = t.split('#').next().unwrap_or("");
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

/// Collapse `.` / `..` lexically (never touching the filesystem), like `os.path.normpath`.
/// A `..` that would climb above the root is dropped, so a traversal target clamps at `/`.
fn lexical_normalize(p: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // Can't climb above a root/prefix — drop it (normpath semantics).
                Some(Component::RootDir | Component::Prefix(_)) => {}
                // A leading `..` on a relative path is kept.
                _ => out.push(comp),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        return PathBuf::from(".");
    }
    let mut pb = PathBuf::new();
    for c in out {
        pb.push(c.as_os_str());
    }
    pb
}

/// Make `p` absolute (joining the cwd if relative) and normalize it lexically.
fn abs_normalized(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    lexical_normalize(&abs)
}

/// Classify a normalized link target found in `md_abspath`.
///
/// Returns [`Class::External`] when the target escapes `repo_root` (a peer-clone citation),
/// [`Class::Present`] when it exists in-repo, or [`Class::Broken`] when it is an in-repo path
/// that is missing. The returned `String` is repo-root-relative when in-repo, else the
/// absolute resolved path.
#[must_use]
pub fn classify(repo_root: &Path, md_abspath: &Path, target: &str) -> (Class, String) {
    let repo_root = abs_normalized(repo_root);
    let base = md_abspath.parent().unwrap_or(Path::new("."));
    let resolved = lexical_normalize(&base.join(target));
    match resolved.strip_prefix(&repo_root) {
        Ok(rel) => {
            let rel = if rel.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                rel.to_path_buf()
            };
            let class = if resolved.exists() {
                Class::Present
            } else {
                Class::Broken
            };
            (class, rel.to_string_lossy().into_owned())
        }
        Err(_) => (Class::External, resolved.to_string_lossy().into_owned()),
    }
}

/// Yield absolute paths of the in-scope Markdown files that exist under `repo_root`.
#[must_use]
pub fn iter_markdown(repo_root: &Path) -> Vec<PathBuf> {
    let repo_root = abs_normalized(repo_root);
    let mut out = Vec::new();
    for entry in SCAN_ROOTS {
        let path = repo_root.join(entry);
        if path.is_dir() {
            walk_markdown(&path, &mut out);
        } else if path.is_file() && is_markdown(&path) {
            out.push(path);
        }
    }
    out
}

fn is_markdown(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Recursively collect `*.md` files under `dir`, visiting entries in sorted order so the
/// walk is deterministic (the final findings are sorted regardless, but a stable walk keeps
/// the checker's behaviour reproducible).
fn walk_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk_markdown(&path, out);
        } else if is_markdown(&path) {
            out.push(path);
        }
    }
}

/// Return the sorted list of in-repo broken-link [`Finding`]s under `repo_root`.
#[must_use]
pub fn find_broken_links(repo_root: &Path) -> Vec<Finding> {
    let repo_root = abs_normalized(repo_root);
    let mut findings = Vec::new();
    for md in iter_markdown(&repo_root) {
        let text = match std::fs::read(&md) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => continue,
        };
        let rel_md = md
            .strip_prefix(&repo_root)
            .unwrap_or(&md)
            .to_string_lossy()
            .into_owned();
        for (lineno, raw_target) in extract_links(&text) {
            let Some(norm) = normalize_target(&raw_target) else {
                continue;
            };
            let (class, resolved) = classify(&repo_root, &md, &norm);
            if class == Class::Broken {
                findings.push(Finding {
                    file: rel_md.clone(),
                    line: lineno,
                    target: norm,
                    resolved,
                });
            }
        }
    }
    findings.sort();
    findings
}

// --- orphan gate ---------------------------------------------------------------------------
//
// The companion half of docs/gap-analysis/README.md §9. §9.2 (broken links) is `find_broken_links`
// above; §9.1 (discoverability) is here: a first-party doc nobody links to is an orphan — it exists
// but no reader walking out from `README.md` will ever find it. The gate recomputes that reachability
// walk and fails on any tracked orphan that is not deliberately allowlisted, so a new design doc that
// is never linked into its track README cannot silently rot out of reach.

/// The root of the discoverability graph: reachability is measured as a walk out from here, the way
/// a reader (or the gap-analysis sweep) starts at the repo's front door.
pub const DISCOVERY_ROOT: &str = "README.md";

/// Repo-relative path of the committed allowlist of *intentional* orphans (governance data, read at
/// runtime so the same binary audits any tree — the `test/mt-audit/manifest.toml` shape). Missing =
/// empty allowlist.
pub const ORPHAN_ALLOWLIST: &str = "test/doc-links/orphans.allow";

/// Reachability + allowlist verdict for the in-scope docs under a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanReport {
    /// Tracked in-scope `.md` files not reachable from [`DISCOVERY_ROOT`] and not allowlisted —
    /// the findings the gate fails on. Sorted.
    pub orphans: Vec<String>,
    /// Allowlist entries that are no longer orphans (now reachable, or the file is gone). A stale
    /// exemption is itself a finding so the allowlist cannot quietly drift out of sync. Sorted.
    pub stale_allow: Vec<String>,
}

impl OrphanReport {
    /// True when the tree is clean: no un-allowlisted orphan and no stale exemption.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.orphans.is_empty() && self.stale_allow.is_empty()
    }
}

/// Parse the allowlist text: one repo-relative path per line; `#` starts a comment (whole-line or a
/// trailing reason), blank lines ignored. Paths are returned verbatim (forward-slash, repo-relative).
#[must_use]
pub fn parse_allowlist(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Load and parse [`ORPHAN_ALLOWLIST`] from `repo_root`, or an empty set if it is absent.
#[must_use]
pub fn load_allowlist(repo_root: &Path) -> BTreeSet<String> {
    let repo_root = abs_normalized(repo_root);
    match std::fs::read(repo_root.join(ORPHAN_ALLOWLIST)) {
        Ok(bytes) => parse_allowlist(&String::from_utf8_lossy(&bytes)),
        Err(_) => BTreeSet::new(),
    }
}

/// The repo-relative paths of in-scope `.md` files reachable from [`DISCOVERY_ROOT`] by following
/// relative in-repo Markdown links (a breadth-first walk of the link graph).
///
/// Only `.md` targets that resolve [`Class::Present`] inside the in-scope universe are followed;
/// links to code, directories, external peer clones or out-of-scope trees are terminal. The root
/// itself is always included when it exists.
#[must_use]
pub fn reachable_docs(repo_root: &Path) -> BTreeSet<String> {
    let repo_root = abs_normalized(repo_root);
    let universe: BTreeSet<String> = iter_markdown(&repo_root)
        .iter()
        .map(|p| rel_str(&repo_root, p))
        .collect();

    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    if universe.contains(DISCOVERY_ROOT) {
        seen.insert(DISCOVERY_ROOT.to_string());
        queue.push_back(DISCOVERY_ROOT.to_string());
    }

    while let Some(rel) = queue.pop_front() {
        let md = repo_root.join(&rel);
        let Ok(bytes) = std::fs::read(&md) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for (_, raw_target) in extract_links(&text) {
            let Some(norm) = normalize_target(&raw_target) else {
                continue;
            };
            let (class, resolved) = classify(&repo_root, &md, &norm);
            // Follow only in-scope docs we have not visited; everything else is terminal.
            if class == Class::Present
                && universe.contains(&resolved)
                && seen.insert(resolved.clone())
            {
                queue.push_back(resolved);
            }
        }
    }
    seen
}

/// Audit docs discoverability under `repo_root` against the committed allowlist (§9.1).
#[must_use]
pub fn find_orphans(repo_root: &Path) -> OrphanReport {
    let repo_root = abs_normalized(repo_root);
    let universe: BTreeSet<String> = iter_markdown(&repo_root)
        .iter()
        .map(|p| rel_str(&repo_root, p))
        .collect();
    let reachable = reachable_docs(&repo_root);
    let allow = load_allowlist(&repo_root);

    let orphans: Vec<String> = universe
        .iter()
        .filter(|f| !reachable.contains(*f) && !allow.contains(*f))
        .cloned()
        .collect();
    // An allowlist entry earns its keep only while it is still a real orphan: gone from the tree, or
    // reachable again, means the exemption is stale and must be removed.
    let stale_allow: Vec<String> = allow
        .iter()
        .filter(|f| !universe.contains(*f) || reachable.contains(*f))
        .cloned()
        .collect();

    OrphanReport {
        orphans,
        stale_allow,
    }
}

/// Repo-relative, forward-slash string for an absolute in-repo path (falls back to the absolute
/// path if `p` is somehow not under `repo_root`).
fn rel_str(repo_root: &Path, p: &Path) -> String {
    p.strip_prefix(repo_root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstest::rstest;

    // A unique, freshly-created temp directory (no external dep): the name mixes the pid,
    // a nanosecond clock, and a per-process counter, so parallel tests never collide.
    fn tempdir() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "agent-doc-links-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&p).expect("create temp dir");
        p
    }

    fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).expect("create parent");
        fs::write(&path, text).expect("write fixture");
        path
    }

    fn broken_targets(root: &Path) -> Vec<String> {
        find_broken_links(root)
            .into_iter()
            .map(|f| f.target)
            .collect()
    }

    // --- extract_links -----------------------------------------------------------------

    #[test]
    fn positive_inline_and_image() {
        let text = "see [a](x.md) and ![img](y.png)\n";
        assert_eq!(
            extract_links(text),
            vec![(1, "x.md".to_string()), (1, "y.png".to_string())]
        );
    }

    #[test]
    fn positive_reference_definition() {
        assert_eq!(
            extract_links("[lbl]: ../z.md\n"),
            vec![(1, "../z.md".to_string())]
        );
    }

    #[test]
    fn corner_skips_fenced_block() {
        let text = "a [live](live.md)\n```\n[fake](fake.md)\n```\nb [also](also.md)\n";
        assert_eq!(
            extract_links(text),
            vec![(1, "live.md".to_string()), (5, "also.md".to_string())]
        );
    }

    #[test]
    fn corner_tilde_fence() {
        let text = "~~~\n[fake](fake.md)\n~~~\n[real](real.md)\n";
        assert_eq!(extract_links(text), vec![(4, "real.md".to_string())]);
    }

    #[test]
    fn boundary_empty_input() {
        assert_eq!(extract_links(""), Vec::<(usize, String)>::new());
    }

    // --- normalize_target --------------------------------------------------------------

    #[test]
    fn positive_plain_path() {
        assert_eq!(normalize_target("../a/b.md").as_deref(), Some("../a/b.md"));
    }

    #[test]
    fn positive_strips_title_and_anchor() {
        assert_eq!(
            normalize_target("foo.md#sec \"Title\"").as_deref(),
            Some("foo.md")
        );
    }

    #[test]
    fn positive_angle_brackets() {
        assert_eq!(normalize_target("<ab.md>").as_deref(), Some("ab.md"));
    }

    #[test]
    fn corner_pure_anchor_skipped() {
        assert_eq!(normalize_target("#section"), None);
    }

    #[test]
    fn corner_inline_code_target_skipped() {
        // `[`Attempt`]: `Done(T)` …` in prose is not a reference definition.
        assert_eq!(normalize_target("`Done(T)`"), None);
    }

    #[rstest]
    #[case::http("http://x")]
    #[case::https("https://x")]
    #[case::mailto("mailto:a@b.c")]
    #[case::tel("tel:123")]
    #[case::protocol_relative("//cdn/x")]
    fn corner_scheme_urls_skipped(#[case] target: &str) {
        assert_eq!(normalize_target(target), None, "{target}");
    }

    #[rstest]
    #[case::empty("")]
    #[case::whitespace("   ")]
    fn boundary_empty_skipped(#[case] target: &str) {
        assert_eq!(normalize_target(target), None);
    }

    // --- classify ----------------------------------------------------------------------

    #[test]
    fn positive_existing_in_repo() {
        let root = tempdir();
        write(&root, "crates/x/src/mod.rs", "// code");
        let md = write(&root, "docs/a.md", "");
        let (class, resolved) = classify(&root, &md, "../crates/x/src/mod.rs");
        assert_eq!(class, Class::Present);
        assert_eq!(resolved, Path::new("crates/x/src/mod.rs").to_string_lossy());
    }

    #[test]
    fn negative_missing_in_repo() {
        let root = tempdir();
        let md = write(&root, "docs/a.md", "");
        let (class, _) = classify(&root, &md, "../crates/x/src/gone.rs");
        assert_eq!(class, Class::Broken);
    }

    #[test]
    fn boundary_escapes_root_is_external() {
        let root = tempdir();
        let md = write(&root, "docs/parity/p.md", "");
        let (class, _) = classify(&root, &md, "../../../codex/codex-rs/lib.rs");
        assert_eq!(class, Class::External);
    }

    #[test]
    fn corner_self_directory_link() {
        let root = tempdir();
        write(&root, "docs/b.md", "");
        let md = write(&root, "docs/a.md", "");
        let (class, resolved) = classify(&root, &md, "b.md");
        assert_eq!(class, Class::Present);
        assert_eq!(resolved, Path::new("docs/b.md").to_string_lossy());
    }

    // --- find_broken_links (check-the-checks) ------------------------------------------
    // The pipeline must ACCEPT a clean tree AND REJECT a broken one — an always-clean
    // checker is a broken checker.

    #[test]
    fn positive_clean_tree_has_no_findings() {
        let root = tempdir();
        write(
            &root,
            "crates/agent-core/src/identity.rs",
            "// AGENT_IDENTITY",
        );
        write(&root, "docs/metrics.md", "# Metrics");
        write(
            &root,
            "docs/components/context.md",
            "the [metered decorator](../metrics.md) labels it\n",
        );
        write(
            &root,
            "docs/grpc.md",
            "the [task_local](../crates/agent-core/src/identity.rs) holds it\n",
        );
        assert_eq!(find_broken_links(&root), Vec::new());
    }

    #[test]
    fn negative_detects_moved_file() {
        // Mirrors the real §9.2 break: server.rs was split into server/mod.rs.
        let root = tempdir();
        write(
            &root,
            "crates/agent-grpc/src/server/mod.rs",
            "// with_reflection",
        );
        write(
            &root,
            "docs/parity/13.md",
            "[`with_reflection`](../../crates/agent-grpc/src/server.rs)\n",
        );
        let findings = find_broken_links(&root);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].file,
            Path::new("docs/parity/13.md").to_string_lossy()
        );
        assert_eq!(findings[0].line, 1);
    }

    #[test]
    fn boundary_external_peer_clone_not_reported() {
        let root = tempdir();
        write(
            &root,
            "docs/parity/34.md",
            "[`lib.rs`](../../../codex/codex-rs/sandboxing/src/lib.rs)\n",
        );
        assert_eq!(find_broken_links(&root), Vec::new());
    }

    #[test]
    fn corner_root_readme_scanned() {
        let root = tempdir();
        write(&root, "README.md", "[missing](docs/nope.md)\n");
        let findings = find_broken_links(&root);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, "README.md");
    }

    #[test]
    fn corner_fenced_example_link_ignored() {
        let root = tempdir();
        write(
            &root,
            "docs/a.md",
            "real text\n```md\n[example](does/not/exist.md)\n```\n",
        );
        assert_eq!(find_broken_links(&root), Vec::new());
    }

    #[test]
    fn adversarial_traversal_target_does_not_crash() {
        let root = tempdir();
        write(&root, "docs/a.md", "[x](../../../../../../etc/passwd)\n");
        // Escapes the repo root → external, never a finding, never a panic.
        assert_eq!(find_broken_links(&root), Vec::new());
    }

    #[test]
    fn adversarial_overlong_and_weird_targets() {
        let root = tempdir();
        let huge = "a".repeat(5000);
        write(
            &root,
            "docs/a.md",
            &format!("[x]({huge}.md) [y](   ) [z](<>) [w](#)\n"),
        );
        // Only the huge in-repo path is a real break; the empties/anchor are skipped.
        assert_eq!(broken_targets(&root), vec![format!("{huge}.md")]);
    }

    // --- parse_allowlist ---------------------------------------------------------------

    #[test]
    fn positive_allowlist_paths() {
        let got = parse_allowlist("docs/a.md\ndocs/b.md\n");
        assert_eq!(
            got,
            BTreeSet::from(["docs/a.md".to_string(), "docs/b.md".to_string()])
        );
    }

    #[test]
    fn corner_allowlist_comments_and_trailing_reason() {
        let text = "# header\n\ndocs/a.md  # a standalone log\n   # indented comment\n";
        assert_eq!(
            parse_allowlist(text),
            BTreeSet::from(["docs/a.md".to_string()])
        );
    }

    #[test]
    fn boundary_allowlist_empty() {
        assert!(parse_allowlist("").is_empty());
        assert!(parse_allowlist("# only comments\n\n").is_empty());
    }

    // --- reachable_docs ----------------------------------------------------------------

    #[test]
    fn positive_follows_link_chain() {
        // README -> index -> leaf; all three reachable.
        let root = tempdir();
        write(&root, "README.md", "see [index](docs/README.md)\n");
        write(&root, "docs/README.md", "and [leaf](leaf.md)\n");
        write(&root, "docs/leaf.md", "# leaf");
        assert_eq!(
            reachable_docs(&root),
            BTreeSet::from([
                "README.md".to_string(),
                "docs/README.md".to_string(),
                "docs/leaf.md".to_string(),
            ])
        );
    }

    #[test]
    fn corner_does_not_follow_code_or_external_targets() {
        // A link to a .rs file (not a doc) and to a peer clone are both terminal: neither
        // becomes a reachable doc, and neither crashes the walk.
        let root = tempdir();
        write(&root, "crates/x/src/lib.rs", "// code");
        write(
            &root,
            "README.md",
            "[code](crates/x/src/lib.rs) and [peer](../../../codex/x.md)\n",
        );
        assert_eq!(
            reachable_docs(&root),
            BTreeSet::from(["README.md".to_string()])
        );
    }

    #[test]
    fn boundary_no_readme_means_nothing_reachable() {
        let root = tempdir();
        write(&root, "docs/a.md", "# orphaned by construction");
        assert!(reachable_docs(&root).is_empty());
    }

    // --- find_orphans (check-the-checks) -----------------------------------------------
    // The gate must ACCEPT a fully-linked tree AND REJECT one with an unreachable doc — an
    // always-clean orphan gate is a broken gate.

    #[test]
    fn positive_clean_tree_has_no_orphans() {
        let root = tempdir();
        write(&root, "README.md", "[docs](docs/README.md)\n");
        write(&root, "docs/README.md", "[a](a.md)\n");
        write(&root, "docs/a.md", "# a");
        let report = find_orphans(&root);
        assert!(report.is_clean(), "{report:?}");
    }

    #[test]
    fn negative_detects_unlinked_doc() {
        let root = tempdir();
        write(&root, "README.md", "[docs](docs/README.md)\n");
        write(&root, "docs/README.md", "# index, links nothing else\n");
        write(&root, "docs/orphan.md", "# nobody links me");
        let report = find_orphans(&root);
        assert_eq!(report.orphans, vec!["docs/orphan.md".to_string()]);
        assert!(report.stale_allow.is_empty());
    }

    #[test]
    fn corner_allowlist_suppresses_a_real_orphan() {
        let root = tempdir();
        write(&root, "README.md", "# front door, links nothing\n");
        write(&root, "docs/standalone.md", "# deliberately standalone");
        write(
            &root,
            "test/doc-links/orphans.allow",
            "# intentional\ndocs/standalone.md  # a standalone artifact\n",
        );
        let report = find_orphans(&root);
        assert!(
            report.is_clean(),
            "allowlisted orphan should be suppressed: {report:?}"
        );
    }

    #[test]
    fn boundary_stale_allowlist_entry_is_reported() {
        // The allowlisted doc is now reachable → the exemption is stale and must be flagged,
        // so the allowlist cannot drift out of sync with the tree.
        let root = tempdir();
        write(&root, "README.md", "[a](docs/a.md)\n");
        write(&root, "docs/a.md", "# now linked");
        write(&root, "test/doc-links/orphans.allow", "docs/a.md\n");
        let report = find_orphans(&root);
        assert!(report.orphans.is_empty());
        assert_eq!(report.stale_allow, vec!["docs/a.md".to_string()]);
    }

    #[test]
    fn boundary_stale_allowlist_entry_for_missing_file() {
        let root = tempdir();
        write(&root, "README.md", "# links nothing\n");
        write(&root, "test/doc-links/orphans.allow", "docs/gone.md\n");
        let report = find_orphans(&root);
        assert_eq!(report.stale_allow, vec!["docs/gone.md".to_string()]);
    }

    #[test]
    fn adversarial_hostile_links_and_allowlist_do_not_crash() {
        // A doc-authored link graph is untrusted: a self-link, a traversal escape, a huge
        // target and an allowlist full of junk must neither panic nor read out of tree.
        let root = tempdir();
        let huge = "a".repeat(5000);
        write(
            &root,
            "README.md",
            &format!("[self](README.md) [esc](../../../../etc/passwd) [big]({huge}.md)\n"),
        );
        write(&root, "docs/real-orphan.md", "# unreachable");
        write(
            &root,
            "test/doc-links/orphans.allow",
            "../../../etc/passwd\n#\n   \n/absolute/nonsense\n",
        );
        let report = find_orphans(&root);
        // The real orphan is still found; the junk allowlist entries are all stale (not in the
        // universe), reported rather than silently trusted.
        assert_eq!(report.orphans, vec!["docs/real-orphan.md".to_string()]);
        assert_eq!(
            report.stale_allow,
            vec![
                "../../../etc/passwd".to_string(),
                "/absolute/nonsense".to_string(),
            ]
        );
    }
}
