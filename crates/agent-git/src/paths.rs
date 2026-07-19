//! Path resolution for the git seam — mirrors `agent_search`'s `repo_root` /
//! `default_index_dir` pattern so the mirror, worktrees and caches live under the
//! same per-repo `.agent-seddon/` tree.

use std::path::{Path, PathBuf};

/// Walk up from `start` looking for a `.git` entry; return that repo root, or
/// `start` itself if none is found.
pub fn repo_root(start: &Path) -> PathBuf {
    let mut cur = start;
    loop {
        if cur.join(".git").exists() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => return start.to_path_buf(),
        }
    }
}

/// Default shared bare/mirror object DB: `<root>/.agent-seddon/mirror`.
pub fn default_mirror_dir(root: &Path) -> PathBuf {
    root.join(".agent-seddon").join("mirror")
}

/// Default parent dir for disposable worktrees: `<root>/.agent-seddon/worktrees`.
pub fn default_worktrees_dir(root: &Path) -> PathBuf {
    root.join(".agent-seddon").join("worktrees")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;

    #[test]
    fn repo_root_finds_git_dir() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("src/inner")).unwrap();
        assert_eq!(repo_root(&dir.join("src/inner")), dir);
    }

    #[test]
    fn repo_root_falls_back_to_start() {
        let dir = tempdir();
        assert_eq!(repo_root(&dir), dir);
    }

    #[test]
    fn default_dirs_layout() {
        let root = Path::new("/repo");
        assert_eq!(
            default_mirror_dir(root),
            Path::new("/repo/.agent-seddon/mirror")
        );
        assert_eq!(
            default_worktrees_dir(root),
            Path::new("/repo/.agent-seddon/worktrees")
        );
    }
}
