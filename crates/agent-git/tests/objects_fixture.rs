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

/// Adversarial: a model-controlled revision like `--output=<file>` must never
/// reach `git log`/`ls-tree` as an option — `git log --output=<file>` writes and
/// truncates that file, an escape past path confinement. The backend resolves the
/// revision to an oid first (`rev-parse --verify` rejects a leading-dash value),
/// so every walk fails closed and the victim file is left untouched.
#[tokio::test]
async fn adversarial_revision_arg_injection_cannot_write_a_file() {
    let dir = fixture();
    let victim = dir.join("victim.txt");
    std::fs::write(&victim, "ORIGINAL").unwrap();
    let b = backend(&dir);

    let hostile = format!("--output={}", victim.display());
    let rev = Revision::from(hostile.as_str());

    assert!(
        b.log(&rev, None, 10).await.is_err(),
        "log must reject a leading-dash revision"
    );
    assert!(
        b.log_touched(&rev, 10).await.is_err(),
        "log_touched must reject a leading-dash revision"
    );
    assert!(
        b.list_tree(&rev, Path::new(""), false).await.is_err(),
        "list_tree must reject a leading-dash revision"
    );

    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "ORIGINAL",
        "the victim file must be untouched by any of the three walks"
    );
}

/// A benign revision still resolves and walks correctly after the resolve-first
/// change (no behavioural regression for real refs/oids).
#[tokio::test]
async fn positive_log_still_walks_a_real_branch() {
    let dir = fixture();
    let b = backend(&dir);
    let commits = b.log(&Revision::from("main"), None, 10).await.unwrap();
    assert!(!commits.is_empty(), "main has at least the init commit");
}

// --- review-graph inc 1: heads+tags mirror and the PR base fetch ------------

/// The fleet layout: the mirror is also the root (reads and diffs run on it).
fn fleet_backend(source: &Path, work: &Path) -> CliBackend {
    let mirror = work.join("mirror");
    CliBackend::new(
        &mirror,
        &mirror,
        work.join("wt"),
        source.to_string_lossy().to_string(),
    )
    .with_pr_ref_template("refs/pull/{n}/head")
}

/// Commit `file` on a new or existing `branch` of `repo`, returning the new oid.
fn commit_on(repo: &Path, branch: &str, file: &str) -> String {
    git(repo, &["switch", "-q", "-C", branch]);
    std::fs::write(repo.join(file), format!("{branch}\n")).unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", file]);
    let oid = git_out(repo, &["rev-parse", "HEAD"]);
    git(repo, &["switch", "-q", "main"]);
    oid
}

#[tokio::test]
async fn positive_mirror_is_heads_and_tags_only() {
    // desc: a new mirror of a repo with a branch, a tag and a forge PR ref. expect:
    // branches + tags arrive, `refs/pull/*` does not, and the fetch config is exactly
    // the two refspecs (no `mirror = true`, which would imply `+refs/*:refs/*`).
    let (source, _head) = fixture_with_pr();
    git(&source, &["tag", "v1"]);
    let work = tempdir();
    let b = fleet_backend(&source, &work);
    assert!(b.ensure_mirror().await.unwrap());

    let mirror = work.join("mirror");
    let refs = git_out(&mirror, &["for-each-ref", "--format=%(refname)"]);
    let refs: Vec<&str> = refs.lines().collect();
    assert!(refs.contains(&"refs/heads/main") && refs.contains(&"refs/heads/feature"));
    assert!(refs.contains(&"refs/tags/v1"), "{refs:?}");
    assert!(
        !refs.iter().any(|r| r.starts_with("refs/pull/")),
        "{refs:?}"
    );
    let fetch = git_out(&mirror, &["config", "--get-all", "remote.origin.fetch"]);
    assert_eq!(
        fetch.lines().collect::<Vec<_>>(),
        agent_git::MIRROR_REFSPECS
    );
    let mirror_key = Command::new("git")
        .args(["-C"])
        .arg(&mirror)
        .args(["config", "--get", "remote.origin.mirror"])
        .status()
        .unwrap();
    assert!(!mirror_key.success(), "remote.origin.mirror is unset");
    assert!(
        !work.join("mirror.partial").exists(),
        "staging dir renamed away"
    );
}

#[tokio::test]
async fn positive_staging_target_base_written() {
    // desc: a PR targets `staging`, a branch created upstream after the mirror was
    // made (hypothesis (a): the base was never fetched, so the diff failed and the
    // draft read "Files changed: 0"). expect: before `fetch_branch` the diff errs;
    // after it, `refs/heads/staging` is upstream's oid and the diff has the PR's files.
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let b = fleet_backend(&source, &work);
    b.ensure_mirror().await.unwrap();

    let staging = commit_on(&source, "staging", "s.txt");
    let pr = b.fetch_pr(1).await.unwrap();
    let staging_rev = Revision::from("staging");
    assert!(b.diff(&staging_rev, &pr, &[]).await.is_err(), "no base yet");

    b.fetch_branch("staging").await.unwrap();
    let mirror = work.join("mirror");
    assert_eq!(
        git_out(&mirror, &["rev-parse", "refs/heads/staging"]),
        staging
    );
    let diff = b.diff(&staging_rev, &pr, &[]).await.unwrap();
    assert!(!diff.files.is_empty(), "the PR's files are back");
}

#[tokio::test]
async fn positive_fetch_branch_moves_a_stale_base() {
    // desc: upstream `main` advanced since the mirror was made. expect: `fetch_branch`
    // moves the mirror's `main` to it (a forced, full refspec); other branches are
    // left as they were.
    let source = fixture();
    let work = tempdir();
    let b = fleet_backend(&source, &work);
    b.ensure_mirror().await.unwrap();
    let mirror = work.join("mirror");
    let feature_before = git_out(&mirror, &["rev-parse", "refs/heads/feature"]);

    let new_main = commit_on(&source, "main", "later.txt");
    commit_on(&source, "feature", "f2.txt");
    b.fetch_branch("main").await.unwrap();
    assert_eq!(
        git_out(&mirror, &["rev-parse", "refs/heads/main"]),
        new_main
    );
    assert_eq!(
        git_out(&mirror, &["rev-parse", "refs/heads/feature"]),
        feature_before,
        "only the named branch moved"
    );
}

#[tokio::test]
async fn adversarial_killed_clone_is_replaced() {
    // desc: the mirror path holds what a clone killed partway leaves (l2's nixpkgs
    // mirror): a bare repo whose HEAD is git's `.invalid` placeholder, with no refs.
    // expect: it is not treated as ready; `ensure_mirror` replaces it with a working
    // mirror, and nothing is left at `.partial` / `.stale`.
    let source = fixture();
    let work = tempdir();
    let mirror = work.join("mirror");
    std::fs::create_dir_all(&mirror).unwrap();
    git(&mirror, &["init", "-q", "--bare"]);
    std::fs::write(mirror.join("HEAD"), "ref: refs/heads/.invalid\n").unwrap();
    std::fs::write(mirror.join("objects/pack/tmp_pack_junk"), vec![0u8; 1024]).unwrap();

    let b = fleet_backend(&source, &work);
    assert!(
        b.ensure_mirror().await.unwrap(),
        "the killed clone was replaced"
    );
    assert_eq!(
        git_out(&mirror, &["rev-parse", "refs/heads/main"]),
        git_out(&source, &["rev-parse", "main"])
    );
    assert!(!mirror.join("objects/pack/tmp_pack_junk").exists());
    assert!(!work.join("mirror.partial").exists() && !work.join("mirror.stale").exists());
    assert!(
        !b.ensure_mirror().await.unwrap(),
        "ready now: no second clone"
    );
}

#[tokio::test]
async fn adversarial_old_shape_mirror_fetch_skips_pull_refs() {
    // desc: a mirror made by the old code (`clone --mirror`, config `+refs/*:refs/*`,
    // `mirror = true`); upstream then gains a commit on main and a new PR ref.
    // expect: `fetch` moves main but does not bring the new `refs/pull/*`.
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let mirror = work.join("mirror");
    git(
        &work,
        &[
            "clone",
            "-q",
            "--mirror",
            &source.to_string_lossy(),
            "mirror",
        ],
    );
    let new_main = commit_on(&source, "main", "c.txt");
    git(&source, &["update-ref", "refs/pull/2/head", &new_main]);

    let b = fleet_backend(&source, &work);
    b.fetch().await.unwrap();
    assert_eq!(
        git_out(&mirror, &["rev-parse", "refs/heads/main"]),
        new_main
    );
    let refs = git_out(&mirror, &["for-each-ref", "--format=%(refname)"]);
    assert!(!refs.contains("refs/pull/2/head"), "{refs}");
}

#[tokio::test]
async fn boundary_empty_repo_is_cloned_once() {
    // desc: the remote is an empty repo, so the finished mirror has no branch.
    // expect: one clone, then the mirror counts as ready (no clone per call).
    let source = tempdir();
    git(&source, &["init", "-q", "-b", "main"]);
    let work = tempdir();
    let b = fleet_backend(&source, &work);
    assert!(b.ensure_mirror().await.unwrap(), "first call clones");
    assert!(!b.ensure_mirror().await.unwrap(), "no second clone");
}

#[tokio::test]
async fn negative_failed_clone_leaves_no_partial() {
    // desc: the remote does not exist, so the clone fails.
    // expect: an error, and no `.partial` staging dir left behind.
    let work = tempdir();
    let b = fleet_backend(&work.join("no-such-repo"), &work);
    assert!(b.ensure_mirror().await.is_err());
    assert!(!work.join("mirror.partial").exists());
    assert!(!work.join("mirror").exists());
}

#[tokio::test]
async fn corner_leftover_partial_is_cleared_by_the_next_clone() {
    // desc: a killed new-flow clone left `<mirror>.partial` behind.
    // expect: the next clone clears it, finishes, and leaves no staging dir.
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let partial = work.join("mirror.partial");
    std::fs::create_dir_all(partial.join("objects")).unwrap();
    std::fs::write(partial.join("HEAD"), "ref: refs/heads/.invalid\n").unwrap();
    let b = fleet_backend(&source, &work);
    assert!(b.ensure_mirror().await.unwrap(), "cloned");
    assert!(!partial.exists(), "the leftover staging dir is gone");
    let main = git_out(&work.join("mirror"), &["rev-parse", "refs/heads/main"]);
    assert_eq!(main.len(), 40, "the mirror has main: {main}");
}

#[tokio::test]
async fn positive_inline_fetch_clones_the_mirror() {
    // desc: a backend without the background clone (the CLI path) fetches with no
    // mirror yet. expect: the clone runs inline, under the ordinary git guard, and the
    // fetch succeeds against the new mirror.
    let (source, _head) = fixture_with_pr();
    let work = tempdir();
    let b = fleet_backend(&source, &work);
    b.fetch().await.unwrap();
    let main = git_out(&work.join("mirror"), &["rev-parse", "refs/heads/main"]);
    assert_eq!(main.len(), 40, "the mirror has main: {main}");
}
