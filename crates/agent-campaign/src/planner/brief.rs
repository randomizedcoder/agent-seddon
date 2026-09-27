//! The repo brief (`03-decomposition.md` step 2 item 4): what the planner knows
//! about the repository beyond the node. RK-12 (`agent repo brief`) is the real
//! source (CP-07); until it lands, [`FallbackBrief`] reads the first part of
//! `docs/architecture.md` plus the `## Conventions` and `## Security` sections of
//! `CLAUDE.md` from the checkout.
//!
//! The brief is **not** injection-screened: `CLAUDE.md` legitimately discusses
//! injection phrases, and the brief is operator-owned text from the checkout, not
//! model output. RK-12 screens its own.

use super::prompt::cut_bytes;
use agent_core::campaign::Task;
use async_trait::async_trait;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The whole brief is cut to this many bytes.
pub const MAX_BRIEF_BYTES: usize = 6 * 1024;
/// The `CLAUDE.md` sections get at most half of it; `docs/architecture.md` the rest.
pub const MAX_SECTIONS_BYTES: usize = MAX_BRIEF_BYTES / 2;
/// A source file is read up to here before any processing (cap before buffering).
const MAX_SOURCE_BYTES: u64 = 64 * 1024;
/// The `CLAUDE.md` sections the fallback quotes, by heading prefix.
pub const SECTION_HEADINGS: [&str; 2] = ["## Conventions", "## Security"];

/// Where the brief comes from.
#[async_trait]
pub trait BriefSource: Send + Sync {
    /// The brief for `node`, at most [`MAX_BRIEF_BYTES`]; `Err` is a source failure
    /// the planner reports in the prompt (`[brief unavailable: …]`) and continues.
    async fn brief(&self, node: &Task) -> Result<String, String>;
}

/// A fixed brief (tests, or an operator-supplied text).
#[derive(Debug, Clone)]
pub struct StaticBrief(pub String);

#[async_trait]
impl BriefSource for StaticBrief {
    async fn brief(&self, _node: &Task) -> Result<String, String> {
        Ok(cut_bytes(&self.0, MAX_BRIEF_BYTES).to_string())
    }
}

/// The pre-RK-12 fallback over a checkout at `root`.
#[derive(Debug, Clone)]
pub struct FallbackBrief {
    root: PathBuf,
}

impl FallbackBrief {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        FallbackBrief { root: root.into() }
    }

    /// Read at most [`MAX_SOURCE_BYTES`] of `rel` under the root; `None` when the file
    /// is missing or unreadable.
    fn read_capped(&self, rel: &str) -> Option<String> {
        let path = self.root.join(rel);
        let f = std::fs::File::open(&path).ok()?;
        let mut buf = Vec::new();
        f.take(MAX_SOURCE_BYTES).read_to_end(&mut buf).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Both sources as one brief, or `Err` when neither exists.
    pub fn compose_from(root: &Path) -> Result<String, String> {
        let me = FallbackBrief::new(root);
        let architecture = me.read_capped("docs/architecture.md");
        let claude = me.read_capped("CLAUDE.md");
        if architecture.is_none() && claude.is_none() {
            return Err(format!(
                "no brief source under {}: neither docs/architecture.md nor CLAUDE.md",
                root.display()
            ));
        }
        let sections = claude
            .as_deref()
            .map(|c| sections(c, &SECTION_HEADINGS))
            .unwrap_or_default();
        Ok(compose(architecture.as_deref().unwrap_or(""), &sections))
    }
}

#[async_trait]
impl BriefSource for FallbackBrief {
    async fn brief(&self, _node: &Task) -> Result<String, String> {
        // Two small reads from the checkout; not worth a blocking-pool hop.
        FallbackBrief::compose_from(&self.root)
    }
}

/// The sections of `markdown` whose `## ` heading starts with one of `headings`,
/// each running to the next `## ` heading (or the end), in document order.
pub fn sections(markdown: &str, headings: &[&str]) -> String {
    let mut out = String::new();
    let mut keep = false;
    for line in markdown.lines() {
        if line.starts_with("## ") {
            keep = headings.iter().any(|h| line.starts_with(h));
        }
        if keep {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `architecture` (cut to what is left) followed by `sections` (cut to
/// [`MAX_SECTIONS_BYTES`]), together at most [`MAX_BRIEF_BYTES`].
pub fn compose(architecture: &str, sections: &str) -> String {
    let sections = cut_bytes(sections, MAX_SECTIONS_BYTES);
    let separator = if sections.is_empty() { "" } else { "\n\n" };
    let room = MAX_BRIEF_BYTES.saturating_sub(sections.len() + separator.len());
    let architecture = cut_bytes(architecture, room);
    let mut out = String::with_capacity(architecture.len() + separator.len() + sections.len());
    out.push_str(architecture);
    if !architecture.is_empty() {
        out.push_str(separator);
    }
    out.push_str(sections);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;

    const CLAUDE: &str = "# CLAUDE.md\n\nintro\n\n## Environment\n\nenv text\n\n## Conventions\n\n- tests are table-driven\n\n## Security: the model is untrusted\n\nfail closed\n\n## Other\n\nnot quoted\n";

    #[test]
    fn positive_sections_by_heading_prefix() {
        let s = sections(CLAUDE, &SECTION_HEADINGS);
        assert!(s.starts_with("## Conventions\n"));
        assert!(s.contains("- tests are table-driven\n"));
        assert!(s.contains("## Security: the model is untrusted\n\nfail closed\n"));
        assert!(!s.contains("env text"));
        assert!(!s.contains("## Other"));
        assert!(!s.contains("not quoted"));
    }

    #[test]
    fn corner_sections_missing_heading_is_empty() {
        assert_eq!(sections(CLAUDE, &["## Nope"]), "");
        assert_eq!(sections("", &SECTION_HEADINGS), "");
    }

    #[test]
    fn boundary_compose_caps_total_and_sections() {
        let arch = "A".repeat(20 * 1024);
        let secs = "S".repeat(20 * 1024);
        let out = compose(&arch, &secs);
        assert_eq!(out.len(), MAX_BRIEF_BYTES);
        assert_eq!(out.matches('S').count(), MAX_SECTIONS_BYTES);
        assert_eq!(
            out.matches('A').count(),
            MAX_BRIEF_BYTES - MAX_SECTIONS_BYTES - 2
        );
        assert!(out.contains("A\n\nS"));
    }

    #[test]
    fn corner_compose_small_inputs_untouched() {
        assert_eq!(compose("arch", "## S\nx\n"), "arch\n\n## S\nx\n");
        assert_eq!(compose("arch", ""), "arch");
        assert_eq!(compose("", "## S\n"), "## S\n");
        assert_eq!(compose("", ""), "");
    }

    #[test]
    fn corner_compose_multibyte_boundary() {
        let arch = "é".repeat(4 * 1024);
        let out = compose(&arch, "");
        assert!(out.len() <= MAX_BRIEF_BYTES);
        assert!(out.chars().all(|c| c == 'é'));
    }

    #[tokio::test]
    async fn corner_fallback_brief_markers() {
        let root = tempdir();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        // Markers at 5 KiB and 7 KiB: the first survives the cut, the second does not.
        let mut arch = "a".repeat(5 * 1024);
        arch.push_str("MARK5");
        arch.push_str(&"b".repeat(2 * 1024 - 5));
        arch.push_str("MARK7");
        arch.push_str(&"c".repeat(4 * 1024));
        std::fs::write(root.join("docs/architecture.md"), &arch).unwrap();
        std::fs::write(root.join("CLAUDE.md"), CLAUDE).unwrap();
        let node = crate::planner::tests_support::any_task();
        let brief = FallbackBrief::new(&root).brief(&node).await.unwrap();
        assert!(brief.len() <= MAX_BRIEF_BYTES);
        assert!(brief.contains("MARK5"));
        assert!(!brief.contains("MARK7"));
        assert!(brief.contains("## Conventions"));
        assert!(brief.contains("## Security: the model is untrusted"));
        assert!(!brief.contains("## Other"));
    }

    #[tokio::test]
    async fn negative_fallback_brief_no_sources() {
        let root = tempdir();
        let node = crate::planner::tests_support::any_task();
        let err = FallbackBrief::new(&root).brief(&node).await.unwrap_err();
        assert!(err.contains("no brief source"), "{err}");
    }

    #[tokio::test]
    async fn corner_fallback_brief_one_source_missing() {
        let root = tempdir();
        std::fs::write(root.join("CLAUDE.md"), CLAUDE).unwrap();
        let node = crate::planner::tests_support::any_task();
        let brief = FallbackBrief::new(&root).brief(&node).await.unwrap();
        assert!(brief.starts_with("## Conventions"));
    }

    #[tokio::test]
    async fn adversarial_fallback_brief_huge_source_capped() {
        let root = tempdir();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/architecture.md"), "x".repeat(5 << 20)).unwrap();
        let node = crate::planner::tests_support::any_task();
        let brief = FallbackBrief::new(&root).brief(&node).await.unwrap();
        assert_eq!(brief.len(), MAX_BRIEF_BYTES);
    }

    #[tokio::test]
    async fn boundary_static_brief_cut() {
        let node = crate::planner::tests_support::any_task();
        let b = StaticBrief("s".repeat(7 * 1024))
            .brief(&node)
            .await
            .unwrap();
        assert_eq!(b.len(), MAX_BRIEF_BYTES);
    }
}
