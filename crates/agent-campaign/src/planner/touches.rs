//! Resolution of an `execute` decision's `touches` (`03-decomposition.md` step 4):
//! every entry must name something real, or the decision is an attempt `error`.
//!
//! Until RK-08 lands (CP-07), the only resolvable form is an **exact relative path
//! inside the worktree**: [`WorktreeTouches`] requires every `/`-segment to pass
//! `safe_segment`, the whole path to pass `confine` (no symlink escape) and the
//! target to exist and not be a symlink. Node-key syntax (`rust:fn:…`), wildcards
//! and absolute paths are rejected, not guessed at.

use agent_core::campaign::MAX_TOUCH;
use agent_core::{confine, safe_segment};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

/// Which entry failed and why (`touches[i]: reason`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchError {
    pub index: usize,
    pub reason: String,
}

impl std::fmt::Display for TouchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "touches[{}]: {}", self.index, self.reason)
    }
}

/// Resolves a decision's `touches` before the store is written.
#[async_trait]
pub trait TouchResolver: Send + Sync {
    /// `Ok` when every entry resolves; the first failure otherwise.
    async fn resolve(&self, touches: &[String]) -> Result<(), TouchError>;
}

/// Exact paths that exist in a checkout at `root`.
#[derive(Debug, Clone)]
pub struct WorktreeTouches {
    root: PathBuf,
}

/// Characters that mark a node key or a pattern rather than an exact path.
const NOT_A_PATH: [char; 7] = [':', '%', '*', '?', '[', '{', '\\'];

impl WorktreeTouches {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        WorktreeTouches { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// One entry, fail closed.
    pub fn check(&self, entry: &str) -> Result<(), String> {
        if entry.is_empty() {
            return Err("empty".to_string());
        }
        if entry.chars().count() > MAX_TOUCH {
            return Err(format!("over {MAX_TOUCH} chars"));
        }
        if entry.contains(NOT_A_PATH) {
            return Err(
                "exact relative paths only until RK-08 (no node keys, wildcards or backslashes)"
                    .to_string(),
            );
        }
        if entry.starts_with('/') {
            return Err("absolute paths are not allowed".to_string());
        }
        for (i, seg) in entry.split('/').enumerate() {
            if !safe_segment(seg) {
                return Err(format!("segment {i} is not a safe path segment"));
            }
        }
        let resolved = confine(&self.root, entry)?;
        match std::fs::symlink_metadata(&resolved) {
            Err(_) => Err("does not exist in the worktree".to_string()),
            Ok(m) if m.file_type().is_symlink() => Err("is a symlink".to_string()),
            Ok(_) => Ok(()),
        }
    }
}

#[async_trait]
impl TouchResolver for WorktreeTouches {
    async fn resolve(&self, touches: &[String]) -> Result<(), TouchError> {
        for (index, t) in touches.iter().enumerate() {
            self.check(t)
                .map_err(|reason| TouchError { index, reason })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    fn worktree() -> PathBuf {
        let root = tempdir();
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "").unwrap();
        std::fs::write(root.join("src/deep/mod.rs"), "").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        root
    }

    #[rstest]
    #[case::positive_file("src/lib.rs", Ok(()))]
    #[case::positive_nested("src/deep/mod.rs", Ok(()))]
    #[case::positive_dir("src", Ok(()))]
    #[case::positive_top_level("README.md", Ok(()))]
    #[case::negative_missing("crates/nope.rs", Err("does not exist"))]
    #[case::negative_empty("", Err("empty"))]
    #[case::negative_trailing_slash("src/", Err("segment 1"))]
    #[case::corner_dot_segment("./src/lib.rs", Err("segment 0"))]
    #[case::adversarial_traversal("../../etc/passwd", Err("segment 0"))]
    #[case::adversarial_inner_traversal("src/../../etc/passwd", Err("segment 1"))]
    #[case::adversarial_absolute("/etc/passwd", Err("absolute"))]
    #[case::adversarial_wildcard_key("fn:%", Err("exact relative paths only"))]
    #[case::adversarial_node_key(
        "rust:fn:agent_core::security::confine",
        Err("exact relative paths only")
    )]
    #[case::adversarial_glob("src/*.rs", Err("exact relative paths only"))]
    #[case::adversarial_backslash("src\\lib.rs", Err("exact relative paths only"))]
    #[case::adversarial_leading_dash("-rf", Err("segment 0"))]
    #[case::adversarial_space("src/my file.rs", Err("segment 1"))]
    #[case::adversarial_unicode("src/líb.rs", Err("segment 1"))]
    #[case::adversarial_nul("src/lib.rs\0", Err("segment 1"))]
    #[case::boundary_200("a/".repeat(99) + "ab", Err("does not exist"))]
    #[case::boundary_201("a/".repeat(100) + "b", Err("over 200"))]
    #[case::adversarial_huge("a/".repeat(4096), Err("over 200"))]
    fn check_rows(#[case] entry: impl AsRef<str>, #[case] want: Result<(), &str>) {
        let got = WorktreeTouches::new(worktree()).check(entry.as_ref());
        match want {
            Ok(()) => got.unwrap(),
            Err(sub) => {
                let e = got.unwrap_err();
                assert!(e.contains(sub), "{e:?} lacks {sub:?}");
            }
        }
    }

    #[test]
    fn adversarial_long_single_segment() {
        // 129 chars is over `MAX_SEGMENT_LEN` but under the touch cap.
        let entry = "a".repeat(129);
        let e = WorktreeTouches::new(worktree()).check(&entry).unwrap_err();
        assert!(e.contains("segment 0"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn adversarial_symlink_escape_and_symlink_inside() {
        let root = worktree();
        let outside = tempdir();
        std::fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), root.join("src/escape")).unwrap();
        std::os::unix::fs::symlink(root.join("src/lib.rs"), root.join("src/alias.rs")).unwrap();
        let w = WorktreeTouches::new(&root);
        let e = w.check("src/escape").unwrap_err();
        assert!(e.contains("symlink"), "{e}");
        let e = w.check("src/alias.rs").unwrap_err();
        assert_eq!(e, "is a symlink");
        // A symlinked directory on the way out is caught by `confine` too.
        std::os::unix::fs::symlink(&outside, root.join("src/out")).unwrap();
        let e = w.check("src/out/secret").unwrap_err();
        assert!(e.contains("symlink"), "{e}");
    }

    #[tokio::test]
    async fn positive_resolve_all_and_first_failure_indexed() {
        let w = WorktreeTouches::new(worktree());
        w.resolve(&["src/lib.rs".into(), "README.md".into()])
            .await
            .unwrap();
        w.resolve(&[]).await.unwrap();
        let e = w
            .resolve(&["src/lib.rs".into(), "nope".into(), "/etc".into()])
            .await
            .unwrap_err();
        assert_eq!(e.index, 1);
        assert_eq!(e.to_string(), "touches[1]: does not exist in the worktree");
    }
}
