//! The corpus a [`SearchBackend`](agent_core::SearchBackend) indexes.
//!
//! A backend does not care *where* its documents come from — only that it can
//! (a) take a freshness [`Manifest`] of the current corpus, (b) compare a stored
//! manifest against the live corpus, and (c) render one document by id. That is the
//! [`DocumentSource`] seam. The default [`FsDocumentSource`] walks a filesystem tree
//! (the code-search corpus); other sources — e.g. rendered session transcripts for
//! cross-session recall (parity spec 20) — reuse the same reindex/query/freshness
//! machinery without a bespoke index.
//!
//! Documents are keyed by a `Path` **id** stored in the index's `path` field and
//! returned as [`SearchHit::path`](agent_core::SearchHit): a repo-relative path for
//! the FS source, an opaque id (a session id) for others.

use crate::{manifest, Manifest, ScanLimits, ScanReport};
use agent_core::IndexState;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A rendered document ready to index: its searchable `text` and a coarse `lang`
/// label (stored in the `lang` field, used by the language filter).
pub struct SourceDoc {
    pub text: String,
    pub lang: String,
}

/// Where a backend's documents come from. All methods are **blocking** (they may
/// walk a tree, stat files, or read+render a transcript) — call from a blocking
/// context, as the tantivy backend does.
pub trait DocumentSource: Send + Sync {
    /// The current freshness manifest — one stamp per live document, keyed by id.
    fn scan(&self) -> Manifest;

    /// Cheap freshness comparison of `stored` (the manifest saved next to the
    /// index, or `None` if none exists) against the live corpus.
    fn compare(&self, stored: Option<&Manifest>) -> IndexState;

    /// Render one document by its manifest id. `None` ⇒ skip it (unreadable, gone,
    /// or not renderable), exactly as an unreadable file is skipped today.
    fn load(&self, id: &Path) -> Option<SourceDoc>;
}

/// The default corpus: every file under `root`, gitignore-aware. Reproduces the
/// code-search backend's original inline behaviour (walk → `read_to_string` →
/// extension-derived `lang`), now behind the [`DocumentSource`] seam.
///
/// [`with_limits`](Self::with_limits) caps the walk (file count, file size) for a corpus
/// that is not the operator's own tree, such as a fleet row's repo; the last scan's
/// [`ScanReport`] says what the caps left out.
pub struct FsDocumentSource {
    root: PathBuf,
    limits: ScanLimits,
    last_scan: Mutex<ScanReport>,
}

impl FsDocumentSource {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            limits: ScanLimits::UNLIMITED,
            last_scan: Mutex::new(ScanReport::default()),
        }
    }

    /// Cap the walk and each file read at `limits`.
    pub fn with_limits(mut self, limits: ScanLimits) -> Self {
        self.limits = limits;
        self
    }

    /// What the most recent [`scan`](DocumentSource::scan) left out.
    pub fn last_scan(&self) -> ScanReport {
        *self
            .last_scan
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl DocumentSource for FsDocumentSource {
    fn scan(&self) -> Manifest {
        let (manifest, report) = Manifest::scan_with(&self.root, &self.limits);
        *self
            .last_scan
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = report;
        manifest
    }

    fn compare(&self, stored: Option<&Manifest>) -> IndexState {
        manifest::compare_with(&self.root, stored, &self.limits)
    }

    fn load(&self, id: &Path) -> Option<SourceDoc> {
        // Binary/unreadable files are silently skipped (as before). The read is capped
        // too: a file that grew past the size cap after the scan is skipped, never
        // buffered whole. Past the deadline nothing more is read, so a long build ends
        // soon after it (the report says it expired).
        if self
            .limits
            .deadline
            .is_some_and(|d| std::time::Instant::now() >= d)
        {
            self.last_scan
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .expired = true;
            return None;
        }
        let file = std::fs::File::open(self.root.join(id)).ok()?;
        let mut text = String::new();
        file.take(self.limits.max_file_bytes.saturating_add(1))
            .read_to_string(&mut text)
            .ok()?;
        if text.len() as u64 > self.limits.max_file_bytes {
            return None;
        }
        Some(SourceDoc {
            text,
            lang: lang_of(id),
        })
    }
}

/// Map a file extension to a coarse language label (stored for the `lang` filter).
pub fn lang_of(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "nix" => "nix",
        "py" => "python",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" => "cpp",
        "java" => "java",
        "rb" => "ruby",
        "sh" | "bash" => "shell",
        "toml" => "toml",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" => "markdown",
        "txt" => "text",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    #[rstest]
    #[case::rs("a/b.rs", "rust")]
    #[case::nix("flake.nix", "nix")]
    #[case::md("README.md", "markdown")]
    #[case::unknown("x.zzz", "zzz")]
    #[case::none("Makefile", "")]
    fn lang_of_cases(#[case] path: &str, #[case] expected: &str) {
        assert_eq!(lang_of(Path::new(path)), expected);
    }

    // `FsDocumentSource` reproduces the old inline walk: scan stamps every file,
    // load renders its text + extension-lang, and an unreadable id yields `None`.
    #[test]
    fn positive_fs_source_scans_and_loads_files() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("notes.md"), "# hi\n").unwrap();
        let src = FsDocumentSource::new(dir.clone());

        let manifest = src.scan();
        assert!(manifest.entries.contains_key(Path::new("src/main.rs")));
        assert!(manifest.entries.contains_key(Path::new("notes.md")));

        let doc = src.load(Path::new("src/main.rs")).expect("readable");
        assert_eq!(doc.text, "fn main() {}\n");
        assert_eq!(doc.lang, "rust");
    }

    #[test]
    fn corner_fs_source_load_missing_is_none() {
        let dir = tempdir();
        let src = FsDocumentSource::new(dir);
        assert!(src.load(Path::new("does/not/exist.rs")).is_none());
    }

    #[test]
    fn positive_fs_source_compare_tracks_freshness() {
        let dir = tempdir();
        std::fs::write(dir.join("a.rs"), "fn a() {}").unwrap();
        let src = FsDocumentSource::new(dir.clone());
        assert_eq!(src.compare(None), IndexState::Missing);
        let m = src.scan();
        assert_eq!(src.compare(Some(&m)), IndexState::Fresh);
        std::fs::write(dir.join("b.rs"), "fn b() {}").unwrap();
        assert_eq!(src.compare(Some(&m)), IndexState::Stale);
    }

    // A capped source: the scan leaves out what the caps say and reports it, and a file
    // that grew past the size cap after the scan is not loaded.
    #[rstest]
    #[case::positive_small_file_loads("a file under the cap loads", 4, true)]
    #[case::boundary_exactly_cap_loads("a file exactly at the cap loads", 16, true)]
    #[case::adversarial_grown_past_cap_skipped(
        "a file that grew past the cap after the scan is skipped",
        17,
        false
    )]
    fn capped_source_load(
        #[case] description: &str,
        #[case] size: usize,
        #[case] expected_loaded: bool,
    ) {
        let dir = tempdir();
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let src = FsDocumentSource::new(dir.clone()).with_limits(ScanLimits {
            max_files: usize::MAX,
            max_file_bytes: 16,
            ..ScanLimits::UNLIMITED
        });
        assert!(src.scan().entries.contains_key(Path::new("a.txt")));
        std::fs::write(dir.join("a.txt"), "x".repeat(size)).unwrap();
        assert_eq!(
            src.load(Path::new("a.txt")).is_some(),
            expected_loaded,
            "{description}"
        );
    }

    /// A read after the deadline returns nothing and marks the report expired, so a
    /// build that has run out of time stops reading instead of finishing the corpus.
    #[test]
    fn adversarial_reads_stop_at_the_deadline() {
        let dir = tempdir();
        std::fs::write(dir.join("a.rs"), "x").unwrap();
        let src = FsDocumentSource::new(dir).with_limits(ScanLimits {
            deadline: Some(std::time::Instant::now()),
            ..ScanLimits::UNLIMITED
        });
        assert!(
            src.load(Path::new("a.rs")).is_none(),
            "no read past the deadline"
        );
        assert!(src.last_scan().expired, "the report says it expired");
    }

    #[test]
    fn capped_source_reports_the_last_scan() {
        let dir = tempdir();
        for name in ["a.rs", "b.rs", "c.rs"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let src = FsDocumentSource::new(dir).with_limits(ScanLimits {
            max_files: 2,
            max_file_bytes: u64::MAX,
            ..ScanLimits::UNLIMITED
        });
        assert_eq!(
            src.last_scan(),
            ScanReport::default(),
            "nothing scanned yet"
        );
        let m = src.scan();
        assert_eq!(m.entries.len(), 2);
        assert!(src.last_scan().truncated, "the file cap was hit");
        assert_eq!(
            src.compare(Some(&m)),
            IndexState::Fresh,
            "fresh under its own caps"
        );
    }
}
