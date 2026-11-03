//! Index freshness manifest.
//!
//! On every start the agent must cheaply answer "is the search index up to date
//! with the working tree?". We record a [`Manifest`] alongside the index: a stamp
//! (mtime + size) per indexed file, plus the git HEAD it was built against.
//!
//! [`compare`] has a fast path — if the repo is a clean git checkout at the same
//! HEAD the manifest was built against, it returns [`IndexState::Fresh`] without
//! walking the tree at all (the common "already fresh" case). Otherwise it does a
//! stat-only, gitignore-aware walk and diffs the stamp sets. Stat + size (rather
//! than content hashing) keeps the walk cheap enough to run unconditionally.

use agent_core::{IndexState, Result};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A file's cheap change-detection stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    pub mtime_ms: u64,
    pub size: u64,
}

/// What the index was built from: a stamp per file (keyed by repo-relative path),
/// the git HEAD at build time, and when it was built.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub entries: BTreeMap<PathBuf, FileStamp>,
    #[serde(default)]
    pub git_head: Option<String>,
    #[serde(default)]
    pub built_ms: u64,
}

impl Manifest {
    /// Walk `root` (gitignore-aware) and stamp every file. Blocking — call from a
    /// blocking context.
    pub fn scan(root: &Path) -> Self {
        Self::scan_with(root, &ScanLimits::UNLIMITED).0
    }

    /// [`scan`](Self::scan) under `limits`: files over `max_file_bytes` are left out, and
    /// the walk stops after `max_files`. The [`ScanReport`] says what was left out, so a
    /// caller can mark its index partial instead of silently serving a truncated one.
    /// Blocking.
    pub fn scan_with(root: &Path, limits: &ScanLimits) -> (Self, ScanReport) {
        let (entries, report) = scan_entries(root, limits);
        let manifest = Self {
            entries,
            git_head: git_head(root),
            built_ms: now_ms(),
        };
        (manifest, report)
    }

    /// A stable digest of the stamp set — a cheap over-the-wire equality check
    /// (two agents agree the index matches iff the digests match).
    pub fn digest(&self) -> String {
        let mut h = DefaultHasher::new();
        for (path, stamp) in &self.entries {
            path.hash(&mut h);
            stamp.mtime_ms.hash(&mut h);
            stamp.size.hash(&mut h);
        }
        format!("{:016x}", h.finish())
    }

    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

/// Caps on a [`Manifest::scan_with`] walk. The default is [`ScanLimits::UNLIMITED`], the
/// walk every existing caller does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanLimits {
    /// Stop the walk after this many files.
    pub max_files: usize,
    /// Leave out a file larger than this (a generated or vendored blob).
    pub max_file_bytes: u64,
}

impl ScanLimits {
    pub const UNLIMITED: Self = Self {
        max_files: usize::MAX,
        max_file_bytes: u64::MAX,
    };
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self::UNLIMITED
    }
}

/// What a limited scan left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// The walk hit `max_files`, so files past it are missing.
    pub truncated: bool,
    /// Files left out for exceeding `max_file_bytes`.
    pub oversized: u64,
}

/// Determine the index state of `root` given the `stored` manifest (the one saved
/// next to the index, or `None` if no index exists yet). Blocking.
pub fn compare(root: &Path, stored: Option<&Manifest>) -> IndexState {
    compare_with(root, stored, &ScanLimits::UNLIMITED)
}

/// [`compare`] against a scan under the same `limits` the index was built with, so a
/// capped index does not read as stale forever. Blocking.
pub fn compare_with(root: &Path, stored: Option<&Manifest>, limits: &ScanLimits) -> IndexState {
    let Some(stored) = stored else {
        return IndexState::Missing;
    };
    // Fast path: a clean git checkout at the recorded HEAD cannot have diverged.
    if let (Some(head), Some(true)) = (git_head(root), git_clean(root)) {
        if stored.git_head.as_deref() == Some(head.as_str()) {
            return IndexState::Fresh;
        }
    }
    // Fallback: stat-walk and diff the stamp sets.
    if scan_entries(root, limits).0 == stored.entries {
        IndexState::Fresh
    } else {
        IndexState::Stale
    }
}

fn scan_entries(root: &Path, limits: &ScanLimits) -> (BTreeMap<PathBuf, FileStamp>, ScanReport) {
    let mut entries = BTreeMap::new();
    let mut report = ScanReport::default();
    // Sorted, so a capped walk keeps the same files every time.
    let walk = WalkBuilder::new(root)
        .hidden(false)
        .sort_by_file_name(std::cmp::Ord::cmp)
        .build();
    for result in walk {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        // Never index our own index/manifest — it changes as a side effect of
        // indexing and would make the tree perpetually "stale".
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        if rel.components().any(|c| c.as_os_str() == ".agent-seddon") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.len() > limits.max_file_bytes {
                report.oversized += 1;
                continue;
            }
            if entries.len() >= limits.max_files {
                report.truncated = true;
                break;
            }
            entries.insert(rel.to_path_buf(), stamp(&meta));
        }
    }
    (entries, report)
}

fn stamp(meta: &std::fs::Metadata) -> FileStamp {
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    FileStamp {
        mtime_ms,
        size: meta.len(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `HEAD` object id, or `None` if `root` is not a git checkout / git is absent.
fn git_head(root: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!head.is_empty()).then_some(head)
}

/// `true` if the working tree is clean (no tracked/untracked changes). `None` if
/// not a git checkout.
fn git_clean(root: &Path) -> Option<bool> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(out.stdout.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    /// A non-git fixture tree with two files.
    fn fixture() -> PathBuf {
        let dir = tempdir();
        std::fs::write(dir.join("a.rs"), "fn a() {}").unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/b.rs"), "fn b() {}").unwrap();
        dir
    }

    #[test]
    fn missing_when_no_manifest() {
        let dir = fixture();
        assert_eq!(compare(&dir, None), IndexState::Missing);
    }

    #[test]
    fn fresh_when_unchanged() {
        let dir = fixture();
        let m = Manifest::scan(&dir);
        assert_eq!(compare(&dir, Some(&m)), IndexState::Fresh);
    }

    // Each mutation makes a previously-fresh manifest stale.
    #[rstest]
    #[case::added(|d: &Path| std::fs::write(d.join("c.rs"), "x").unwrap())]
    #[case::removed(|d: &Path| std::fs::remove_file(d.join("a.rs")).unwrap())]
    #[case::changed(|d: &Path| std::fs::write(d.join("a.rs"), "fn a() { changed }").unwrap())]
    fn stale_after_mutation(#[case] mutate: fn(&Path)) {
        let dir = fixture();
        let m = Manifest::scan(&dir);
        mutate(&dir);
        assert_eq!(compare(&dir, Some(&m)), IndexState::Stale);
    }

    #[test]
    fn digest_is_stable_and_sensitive() {
        let dir = fixture();
        let m = Manifest::scan(&dir);
        assert_eq!(m.digest(), m.digest(), "digest is deterministic");
        std::fs::write(dir.join("a.rs"), "fn a() { more }").unwrap();
        let m2 = Manifest::scan(&dir);
        assert_ne!(m.digest(), m2.digest(), "digest changes with content");
    }

    #[test]
    fn manifest_roundtrips_through_disk() {
        let dir = fixture();
        let m = Manifest::scan(&dir);
        let path = dir.join("manifest.json");
        m.save(&path).unwrap();
        let loaded = Manifest::load(&path).unwrap();
        assert_eq!(loaded.entries, m.entries);
        assert_eq!(loaded.digest(), m.digest());
    }

    #[test]
    fn index_dir_is_excluded_from_scan() {
        let dir = fixture();
        std::fs::create_dir_all(dir.join(".agent-seddon/index")).unwrap();
        std::fs::write(dir.join(".agent-seddon/index/seg.dat"), "binary").unwrap();
        let (entries, _) = scan_entries(&dir, &ScanLimits::UNLIMITED);
        assert!(
            !entries.keys().any(|p| p.starts_with(".agent-seddon")),
            "the index directory must not be part of the freshness manifest"
        );
    }

    // A limited scan: files over the size cap are left out and counted; the walk stops at
    // the file cap and says so; a capped manifest compares fresh under the same limits.
    #[rstest]
    #[case::positive_unlimited_keeps_all(
        "no caps keep every file",
        ScanLimits::UNLIMITED,
        3,
        ScanReport { truncated: false, oversized: 0 }
    )]
    #[case::negative_oversized_left_out(
        "a file over max_file_bytes is left out and counted",
        ScanLimits { max_files: usize::MAX, max_file_bytes: 16 },
        2,
        ScanReport { truncated: false, oversized: 1 }
    )]
    #[case::boundary_exactly_max_files_not_truncated(
        "exactly max_files files is complete, not truncated",
        ScanLimits { max_files: 3, max_file_bytes: u64::MAX },
        3,
        ScanReport { truncated: false, oversized: 0 }
    )]
    #[case::boundary_one_over_max_files_truncated(
        "one file over max_files stops the walk and says so",
        ScanLimits { max_files: 2, max_file_bytes: u64::MAX },
        2,
        ScanReport { truncated: true, oversized: 0 }
    )]
    #[case::corner_zero_files_cap(
        "a zero file cap keeps nothing, truncated",
        ScanLimits { max_files: 0, max_file_bytes: u64::MAX },
        0,
        ScanReport { truncated: true, oversized: 0 }
    )]
    #[case::adversarial_huge_blob_never_indexed(
        "a huge blob is left out even with room for it in the file cap",
        ScanLimits { max_files: 10, max_file_bytes: 1024 },
        2,
        ScanReport { truncated: false, oversized: 1 }
    )]
    fn scan_with_limits(
        #[case] description: &str,
        #[case] limits: ScanLimits,
        #[case] expected_files: usize,
        #[case] expected_report: ScanReport,
    ) {
        let dir = fixture();
        // A third file, larger than the small caps above.
        std::fs::write(dir.join("big.rs"), "x".repeat(4096)).unwrap();
        let (m, report) = Manifest::scan_with(&dir, &limits);
        assert_eq!(m.entries.len(), expected_files, "{description}: files");
        assert_eq!(report, expected_report, "{description}: report");
        assert_eq!(
            compare_with(&dir, Some(&m), &limits),
            IndexState::Fresh,
            "{description}: a capped manifest is fresh under its own limits"
        );
    }

    #[test]
    fn capped_scan_is_deterministic() {
        let dir = fixture();
        for name in ["c.rs", "d.rs", "e.rs"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let limits = ScanLimits {
            max_files: 3,
            max_file_bytes: u64::MAX,
        };
        let a = Manifest::scan_with(&dir, &limits).0;
        let b = Manifest::scan_with(&dir, &limits).0;
        assert_eq!(
            a.entries, b.entries,
            "the same files survive the cap each time"
        );
    }
}
