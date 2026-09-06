//! Table-driven integration tests for the `git-cli` [`RepoBackend`] against a
//! real, freshly-built git repo in a temp dir (mirrors `agent-search`'s fixture
//! tests). Requires `git` on `PATH` — supplied by the nix dev shell.

use agent_core::{ChangeKind, RepoBackend, Revision, WorktreeSpec};
use agent_git::CliBackend;
use agent_testkit::tempdir;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `git -C dir <args>` for fixture setup, panicking on failure.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

/// A repo with `main` (a.txt="hello\n") and a `feature` branch that modifies
/// a.txt and adds b.txt (containing "world").
fn fixture() -> PathBuf {
    let dir = tempdir();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);

    git(&dir, &["switch", "-q", "-c", "feature"]);
    std::fs::write(dir.join("a.txt"), "hello\nmore\n").unwrap();
    std::fs::write(dir.join("b.txt"), "world\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "feature work"]);
    git(&dir, &["switch", "-q", "main"]);
    dir
}

fn backend(root: &Path) -> CliBackend {
    CliBackend::new(
        root,
        root.join(".agent-seddon/mirror"),
        root.join(".agent-seddon/worktrees"),
        "",
    )
}

#[tokio::test]
async fn resolve_yields_a_full_oid() {
    let dir = fixture();
    let oid = backend(&dir)
        .resolve(&Revision::from("main"))
        .await
        .unwrap();
    assert_eq!(oid.as_str().len(), 40, "sha-1 hex: {oid}");
}

#[tokio::test]
async fn read_file_at_two_revisions() {
    let dir = fixture();
    let b = backend(&dir);
    let main = b
        .read_file(&Revision::from("main"), Path::new("a.txt"))
        .await
        .unwrap();
    assert_eq!(main.text, "hello\n");
    assert!(!main.is_binary);
    assert_eq!(main.oid.as_str().len(), 40);

    let feat = b
        .read_file(&Revision::from("feature"), Path::new("a.txt"))
        .await
        .unwrap();
    assert_eq!(feat.text, "hello\nmore\n");
    // Same path, different revisions ⇒ different blob oids (immutable identity).
    assert_ne!(main.oid, feat.oid);
}

#[tokio::test]
async fn list_tree_sees_added_file_only_on_feature() {
    let dir = fixture();
    let b = backend(&dir);
    let main: Vec<_> = b
        .list_tree(&Revision::from("main"), Path::new(""), false)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert!(main.contains(&PathBuf::from("a.txt")));
    assert!(!main.contains(&PathBuf::from("b.txt")));

    let feat: Vec<_> = b
        .list_tree(&Revision::from("feature"), Path::new(""), false)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert!(feat.contains(&PathBuf::from("b.txt")));
}

/// A binary blob (non-UTF8 bytes, incl. NUL) read through the seam is byte-exact.
/// This is the property the git funnel needs from `stdout_bytes` (R3a): routing a
/// `cat-file blob` through `Sandbox::exec` must not go via the lossy `stdout`
/// string, which would change the length and corrupt the content. `read_file`
/// flags it binary and reports the exact byte length — both computed from the raw
/// bytes `git_bytes` returned.
#[tokio::test]
async fn positive_binary_blob_read_is_byte_exact() {
    let dir = fixture();
    // 0x00 forces the binary classifier; 0xFF/0xFE are invalid UTF-8 (a lossy
    // decode would replace them, changing the length).
    let payload: Vec<u8> = vec![0x00, 0xFF, 0xFE, 0x01, 0x80, 0x00, 0x42];
    std::fs::write(dir.join("bin.dat"), &payload).unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "add binary"]);

    let blob = backend(&dir)
        .read_file(&Revision::from("main"), Path::new("bin.dat"))
        .await
        .unwrap();
    assert!(
        blob.is_binary,
        "a NUL-containing blob must be flagged binary"
    );
    assert_eq!(
        blob.bytes_len,
        payload.len() as u64,
        "byte length must survive the seam exactly (proves stdout_bytes, not lossy stdout)"
    );
}

#[tokio::test]
async fn diff_main_to_feature_reports_add_and_modify() {
    let dir = fixture();
    let d = backend(&dir)
        .diff(&Revision::from("main"), &Revision::from("feature"), &[])
        .await
        .unwrap();
    let added = d
        .files
        .iter()
        .find(|f| f.new_path.as_deref() == Some(Path::new("b.txt")))
        .expect("b.txt in diff");
    assert_eq!(added.change, ChangeKind::Added);

    let modified = d
        .files
        .iter()
        .find(|f| f.new_path.as_deref() == Some(Path::new("a.txt")))
        .expect("a.txt in diff");
    assert_eq!(modified.change, ChangeKind::Modified);
    assert!(modified.additions >= 1, "a.txt gained a line");
    assert!(
        modified.patch.contains("+more"),
        "patch: {}",
        modified.patch
    );
}

#[tokio::test]
async fn diff_is_cached_by_immutable_oids() {
    let dir = fixture();
    let b = backend(&dir);
    let first = b
        .diff(&Revision::from("main"), &Revision::from("feature"), &[])
        .await
        .unwrap();
    let (hits, misses) = b.cache_stats();
    assert_eq!((hits, misses), (0, 1), "first diff is a miss");

    // Same immutable endpoints ⇒ served from cache, identical result.
    let second = b
        .diff(&Revision::from("main"), &Revision::from("feature"), &[])
        .await
        .unwrap();
    let (hits, _) = b.cache_stats();
    assert_eq!(hits, 1, "second diff is a hit");
    assert_eq!(first.files.len(), second.files.len());
    assert_eq!(first.target, second.target);
}

#[tokio::test]
async fn diff_path_glob_narrows_to_one_file() {
    let dir = fixture();
    let d = backend(&dir)
        .diff(
            &Revision::from("main"),
            &Revision::from("feature"),
            &["b.txt".to_string()],
        )
        .await
        .unwrap();
    assert_eq!(d.files.len(), 1);
    assert_eq!(d.files[0].new_path.as_deref(), Some(Path::new("b.txt")));
}

#[tokio::test]
async fn grep_finds_content_at_revision() {
    let dir = fixture();
    let b = backend(&dir);
    // "world" exists only on feature (in b.txt).
    let hits = b
        .grep(&Revision::from("feature"), "world", &[], 20)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, PathBuf::from("b.txt"));
    assert_eq!(hits[0].line, 1);

    let none = b
        .grep(&Revision::from("main"), "world", &[], 20)
        .await
        .unwrap();
    assert!(none.is_empty());
}

#[tokio::test]
async fn log_walks_history() {
    let dir = fixture();
    let commits = backend(&dir)
        .log(&Revision::from("feature"), None, 10)
        .await
        .unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].summary, "feature work");
    assert_eq!(commits[1].summary, "init");
    assert_eq!(commits[0].author, "t");
    assert_eq!(commits[0].parents.len(), 1);
}

#[tokio::test]
async fn branches_lists_both() {
    let dir = fixture();
    let names: Vec<_> = backend(&dir)
        .branches()
        .await
        .unwrap()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert!(names.contains(&"main".to_string()));
    assert!(names.contains(&"feature".to_string()));
}

#[tokio::test]
async fn log_range_is_directional() {
    let dir = fixture();
    let b = backend(&dir);
    // `main..feature` has the one feature commit; the reverse is empty (main is an
    // ancestor of feature).
    let fwd = b
        .log_range(&Revision::from("main"), &Revision::from("feature"), 10)
        .await
        .unwrap();
    assert_eq!(fwd.len(), 1, "main..feature has the feature commit");
    assert!(fwd[0].summary.contains("feature"));
    assert!(!fwd[0].oid.as_str().is_empty());
    let rev = b
        .log_range(&Revision::from("feature"), &Revision::from("main"), 10)
        .await
        .unwrap();
    assert!(rev.is_empty(), "feature..main is empty");
}

#[tokio::test]
async fn mirror_bootstrap_and_worktree_from_mirror() {
    let source = fixture(); // stands in for the upstream remote
    let work = tempdir();
    let b = CliBackend::new(
        &source, // reads fall back to the checkout
        work.join("mirror"),
        work.join("wt"),
        source.to_string_lossy().to_string(), // remote = the source repo
    );

    // First call clones the bare mirror; second is a no-op.
    assert!(b.ensure_mirror().await.unwrap(), "mirror cloned");
    assert!(!b.ensure_mirror().await.unwrap(), "already present");
    assert!(work.join("mirror").join("objects").exists());

    // status() now reports the mirror as the base object DB.
    let st = b.status().await.unwrap();
    assert_eq!(st.mirror_path, work.join("mirror"));

    // fetch() stamps FETCH_HEAD, so last_fetch_ms becomes non-zero.
    let after = b.fetch().await.unwrap();
    assert!(after.last_fetch_ms > 0, "fetch stamped FETCH_HEAD");

    // A worktree materializes from the shared mirror, not the source checkout.
    let h = b
        .worktree_add(&WorktreeSpec {
            revision: Revision::from("feature"),
            writable: false,
            id: Some("cmp".into()),
        })
        .await
        .unwrap();
    assert!(h.path.join("b.txt").exists(), "checked out from mirror");
    b.worktree_remove("cmp").await.unwrap();
}

#[tokio::test]
async fn worktree_add_list_remove_roundtrip() {
    let dir = fixture();
    let b = backend(&dir);
    let handle = b
        .worktree_add(&WorktreeSpec {
            revision: Revision::from("feature"),
            writable: false,
            id: Some("cmp".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(handle.id, "cmp");
    assert!(handle.path.join("b.txt").exists(), "worktree checked out");

    let listed = b.worktree_list().await.unwrap();
    assert!(listed.iter().any(|w| w.id == "cmp"));

    b.worktree_remove("cmp").await.unwrap();
    assert!(!handle.path.exists(), "worktree dir removed");
}

// --- inc 2 (C9): fetch_pr against a real repo -------------------------------

/// Capture `git -C dir <args>` stdout (trimmed), panicking on failure.
fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("spawn git");
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The fixture repo with `refs/pull/1/head` pointing at the `feature` commit —
/// standing in for a forge PR head ref, which is *not* reachable via a default
/// `git fetch` (the exact gap `fetch_pr` closes). Returns (source, head oid).
fn fixture_with_pr() -> (PathBuf, String) {
    let source = fixture();
    let head = git_out(&source, &["rev-parse", "feature"]);
    git(&source, &["update-ref", "refs/pull/1/head", &head]);
    (source, head)
}

/// A backend mirroring `source`, configured with the GitHub PR-ref template.
fn pr_backend(source: &Path, work: &Path) -> CliBackend {
    CliBackend::new(
        source, // reads fall back to the checkout
        work.join("mirror"),
        work.join("wt"),
        source.to_string_lossy().to_string(),
    )
    .with_pr_ref_template("refs/pull/{n}/head")
}

#[tokio::test]
async fn positive_fetch_pr_resolves_head_oid() {
    let (source, head) = fixture_with_pr();
    let work = tempdir();
    let b = pr_backend(&source, &work);

    let got = b.fetch_pr(1).await.unwrap();
    assert_eq!(got.as_str().len(), 40, "full sha-1: {}", got.as_str());
    assert_eq!(got.as_str(), head, "fetched head == the PR head commit");
}

#[tokio::test]
async fn positive_worktree_of_fetched_pr_has_expected_files() {
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let b = pr_backend(&source, &work);

    let head = b.fetch_pr(1).await.unwrap();
    let h = b
        .worktree_add(&WorktreeSpec {
            revision: head,
            writable: false,
            id: Some("pr-1".into()),
        })
        .await
        .unwrap();
    assert!(
        h.path.join("b.txt").exists(),
        "the PR head's files are checked out"
    );
    b.worktree_remove("pr-1").await.unwrap();
}

#[tokio::test]
async fn corner_refetch_same_pr_is_idempotent() {
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let b = pr_backend(&source, &work);

    let first = b.fetch_pr(1).await.unwrap();
    let second = b.fetch_pr(1).await.unwrap();
    assert_eq!(first, second, "refetching the same PR yields the same oid");
}

#[tokio::test]
async fn negative_unknown_pr_number_is_soft_error() {
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let b = pr_backend(&source, &work);

    let err = b.fetch_pr(999).await.unwrap_err();
    assert!(!err.to_string().is_empty(), "a missing PR ref is an error");
    // The mirror survives a failed fetch (no partial teardown).
    assert!(
        work.join("mirror").join("objects").exists(),
        "mirror intact after a failed fetch"
    );
}

#[tokio::test]
async fn boundary_pr_number_max_u64() {
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let b = pr_backend(&source, &work);

    // `u64::MAX` builds a well-formed refspec; it only errs because no such PR
    // ref exists — never on a formatting/overflow panic.
    let err = b.fetch_pr(u64::MAX).await.unwrap_err();
    assert!(!err.to_string().is_empty(), "missing ref, not a panic");
}

#[tokio::test]
async fn corner_worktree_of_fetched_pr_is_advisory_readonly() {
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let b = pr_backend(&source, &work);

    let head = b.fetch_pr(1).await.unwrap();
    let h = b
        .worktree_add(&WorktreeSpec {
            revision: head,
            writable: false,
            id: Some("pr-ro".into()),
        })
        .await
        .unwrap();
    // `writable:false` is an advisory handle flag in inc 2 — true FS read-only /
    // no-exec enforcement is a C23 sandbox-backend concern, not asserted here.
    assert!(!h.writable, "handle marked read-only (advisory)");
    b.worktree_remove("pr-ro").await.unwrap();
}
