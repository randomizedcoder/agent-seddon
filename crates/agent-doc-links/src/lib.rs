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
}
