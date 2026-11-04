//! Small deterministic helpers, deliberately duplicated from private upstream
//! code (`agent-search`'s `lang_of`, `agent-git`'s `safe_segment`) rather than
//! widening those crates' public surface for an ~18-line map and a validator.

use agent_core::ForgeHost;
use std::path::{Path, PathBuf};

/// Extension → coarse language label (a copy of `agent-search`'s private map).
pub(crate) fn lang_of(path: &Path) -> String {
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

/// Fail-closed single-segment validator — the shared `agent_core::safe_segment`
/// (promoted there for multi-session; see docs/design/multi-session/01-identity.md).
/// Rejects empty, `.`/`..`, a leading `-`, and anything outside `[A-Za-z0-9._-]`
/// — blocking path traversal and ref injection in a caller-supplied branch name.
pub(crate) fn safe_segment(s: &str) -> bool {
    agent_core::safe_segment(s)
}

/// Confine a git-reported repo-relative path to the repo (blocks symlink/`..`
/// escape from untrusted history via `agent_core::confine`) and return the
/// *original relative* string for display/matching. `None` ⇒ it escapes and is
/// dropped.
pub(crate) fn confined(root: &Path, rel: &Path) -> Option<String> {
    let s = rel.to_str()?;
    if agent_core::confine(root, s).is_err() {
        return None;
    }
    Some(s.to_string())
}

/// Like [`confined`], but returns the confined path as a [`PathBuf`]. `None` ⇒
/// it escapes and is dropped.
pub(crate) fn confined_relative(root: &Path, rel: &Path) -> Option<PathBuf> {
    confined(root, rel).map(PathBuf::from)
}

/// A "noisy" file whose *content diff* is low-signal for a reviewer — a lockfile
/// or generated code. Its hunks are collapsed to a one-liner (kept as a fact:
/// the file changed, by how much) rather than flooding the context. Filename +
/// suffix based, deliberately conservative (only well-known noise).
pub(crate) fn is_noisy(path: &Path) -> bool {
    is_lockfile(path) || is_generated_path(path)
}

/// A dependency lockfile (`Cargo.lock`, `go.sum`, `*.lock`, …).
fn is_lockfile(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    matches!(
        name,
        "Cargo.lock"
            | "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "go.sum"
            | "flake.lock"
            | "poetry.lock"
            | "Gemfile.lock"
            | "composer.lock"
    ) || name.ends_with(".lock")
}

/// A path that is generated code by naming convention (protobuf/gRPC stubs,
/// minified bundles, `_generated.go`, a `generated/` dir). Unlike [`is_noisy`],
/// lockfiles are not code: a finding on `Cargo.lock` (an advisory) is real.
pub(crate) fn is_generated_path(path: &Path) -> bool {
    let s = path.to_str().unwrap_or("");
    s.ends_with(".pb.go")
        || s.ends_with(".pb.rs")
        || s.ends_with("_pb2.py")
        || s.ends_with("_pb2.pyi")
        || s.ends_with(".min.js")
        || s.ends_with(".min.css")
        || s.ends_with("_generated.go")
        || s.contains("/generated/")
}

/// How much of a file's head [`is_generated`] reads.
pub(crate) const GENERATED_HEAD_BYTES: usize = 1024;

/// Whether a file is generated code: [`is_generated_path`], or a generator's
/// marker in its first [`GENERATED_HEAD_BYTES`] — Go's `// Code generated … DO NOT
/// EDIT.` line (golang.org/s/generatedcode) before the `package` clause, or a
/// comment line carrying `@generated` (prost, buf, many JS tools). `head` is
/// untrusted file content; only whole comment lines count.
pub(crate) fn is_generated(path: &Path, head: &[u8]) -> bool {
    // Cargo.lock says `@generated`, but a finding on a lockfile (an advisory) is
    // about the PR's dependencies, not generator output.
    if is_lockfile(path) {
        return false;
    }
    if is_generated_path(path) {
        return true;
    }
    let head = &head[..head.len().min(GENERATED_HEAD_BYTES)];
    let text = String::from_utf8_lossy(head);
    for line in text.lines() {
        let l = line.trim_end_matches('\r');
        if l.starts_with("package ") {
            break; // Go: the marker must come before the package clause
        }
        if is_marker_line(l) {
            return true;
        }
    }
    false
}

/// A generator's marker line: Go's `// Code generated … DO NOT EDIT.`, or a
/// comment line carrying `@generated`.
fn is_marker_line(l: &str) -> bool {
    if l.strip_prefix("// Code generated ")
        .is_some_and(|rest| rest.ends_with(" DO NOT EDIT."))
    {
        return true;
    }
    let t = l.trim_start();
    ["//", "#", "/*", "*"].iter().any(|c| t.starts_with(c)) && t.contains("@generated")
}

/// How many changed files [`unproven_generated`] reads at the base; past it, the
/// rest are unproven (listed as hand-written).
pub(crate) const MAX_BASE_PROBES: usize = 4096;

/// The changed files (`(path, base-side path)`) whose generated status the PR
/// could have made up: the base version wasn't already generated, or can't be
/// read. A new file, a rename into a generated path, or a marker the PR adds —
/// however it is spelled — is the change's own claim, so such a file is reviewed
/// as hand-written; otherwise stamping a file "generated" would hide its changes
/// from the call graph and rank its findings last.
pub(crate) async fn unproven_generated(
    repo: &dyn agent_core::RepoBackend,
    base: &agent_core::Revision,
    files: Vec<(String, Option<String>)>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for (i, (path, old)) in files.into_iter().enumerate() {
        let base_path = old.unwrap_or_else(|| path.clone());
        let proven = i < MAX_BASE_PROBES
            && match repo.read_file(base, Path::new(&base_path)).await {
                Ok(blob) if !blob.is_binary => {
                    let head = blob.text.as_bytes();
                    is_generated(
                        Path::new(&base_path),
                        &head[..head.len().min(GENERATED_HEAD_BYTES)],
                    )
                }
                _ => false,
            };
        if !proven {
            out.insert(path);
        }
    }
    out
}

/// Read up to [`GENERATED_HEAD_BYTES`] of a repo file for [`is_generated`]. The
/// path is untrusted: it is `confine`d, and only a regular file is opened (a FIFO
/// or device would block or never end). `None` when it can't be read.
pub(crate) fn read_head(root: &Path, rel: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let full = agent_core::confine(root, rel).ok()?;
    if !std::fs::metadata(&full).ok()?.is_file() {
        return None;
    }
    let mut buf = Vec::with_capacity(GENERATED_HEAD_BYTES);
    std::fs::File::open(&full)
        .ok()?
        .take(GENERATED_HEAD_BYTES as u64)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

/// Truncate untrusted text to a byte-ish char budget, appending an honest marker.
pub(crate) fn bound(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push_str(" …[truncated]");
        t
    }
}

/// Fail-closed validator for an explicit revision (a commit id or a ref like
/// `main`, `HEAD~1`, `origin/main`). More permissive than [`safe_segment`] (revs
/// carry `/`, `~`, `^`) but still rejects empty, a leading `-`, and any
/// space/shell metacharacter — so a swept, attacker-adjacent rev cannot inject an
/// option or a shell escape before git resolves it. Length-capped to a sane max.
pub(crate) fn safe_rev(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '~' | '^'))
}

/// Parse a git remote URL into `(host, owner, repo)`. **Fails closed**: returns
/// `None` on anything it does not fully recognize, and rejects an `owner`/`repo`
/// that is not a safe path segment (the URL is attacker-controlled repo config).
pub(crate) fn parse_remote(url: &str) -> Option<(String, String, String)> {
    let url = url.trim();
    // scp-like: git@host:owner/repo(.git)
    if let Some(after) = url.strip_prefix("git@") {
        let (host, path) = after.split_once(':')?;
        return finish(host, path);
    }
    // https://host/owner/repo(.git) — strip an optional `user@` and a `:port`.
    let after = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))?;
    let (host_part, path) = after.split_once('/')?;
    let host = host_part
        .rsplit('@')
        .next()
        .unwrap_or(host_part)
        .split(':')
        .next()
        .unwrap_or(host_part);
    finish(host, path)
}

fn finish(host: &str, path: &str) -> Option<(String, String, String)> {
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    // A repo path may contain sub-groups (GitLab); `owner` is the first segment,
    // `repo` the LAST, and every segment is validated fail-closed.
    let (owner, repo) = path.split_once('/')?;
    let repo_name = repo.rsplit('/').next().unwrap_or(repo);
    if host.is_empty() || owner.is_empty() || repo_name.is_empty() {
        return None;
    }
    if !safe_segment(owner) || !safe_segment(repo_name) {
        return None;
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
    {
        return None;
    }
    Some((host.to_string(), owner.to_string(), repo_name.to_string()))
}

/// Map a parsed host to the closed `ForgeHost` set (fails closed to `Other`).
pub(crate) fn forge_host(host: &str) -> ForgeHost {
    let h = host.to_ascii_lowercase();
    if h == "github.com" || h.ends_with(".github.com") {
        ForgeHost::GitHub
    } else if h == "gitlab.com" || h.contains("gitlab") {
        ForgeHost::GitLab
    } else {
        ForgeHost::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// Each row: a path, its first bytes, and whether it is generated code.
    #[rstest]
    #[case::positive_go_header(
        "Go's generator line",
        "x/a.go",
        "// Code generated by protoc-gen-go. DO NOT EDIT.\n\npackage x\n",
        true
    )]
    #[case::positive_after_license(
        "the marker under a license block",
        "x/a.go",
        "// Copyright 2026\n\n// Code generated by stringer; DO NOT EDIT.\npackage x\n",
        true
    )]
    #[case::positive_at_generated(
        "an @generated comment (prost)",
        "src/a.rs",
        "// This file is @generated by prost-build.\n",
        true
    )]
    #[case::positive_path_rule(
        "a .pb.go path, whatever its content",
        "x/a.pb.go",
        "package x\n",
        true
    )]
    #[case::positive_generated_dir("a nested generated/ dir", "api/generated/a.go", "", true)]
    #[case::negative_hand_written(
        "an ordinary file",
        "x/a.go",
        "package x\n\nfunc F() {}\n",
        false
    )]
    #[case::negative_lockfile(
        "a lockfile is not generated code",
        "Cargo.lock",
        "# This file is automatically @generated by Cargo.\n",
        false
    )]
    #[case::negative_after_package(
        "the Go marker after package does not count",
        "x/a.go",
        "package x\n\n// Code generated by x. DO NOT EDIT.\n",
        false
    )]
    #[case::corner_crlf(
        "a CRLF header still counts",
        "x/a.go",
        "// Code generated by x. DO NOT EDIT.\r\npackage x\r\n",
        true
    )]
    #[case::boundary_missing_period(
        "the Go line must end with `DO NOT EDIT.`",
        "x/a.go",
        "// Code generated by x. DO NOT EDIT\npackage x\n",
        false
    )]
    #[case::adversarial_marker_in_code(
        "@generated inside code, not a comment",
        "x/a.go",
        "var s = \"@generated\"\n",
        false
    )]
    #[case::adversarial_marker_past_head("a marker past the head is not read", "x/a.js", &format!("{}\n// @generated\n", "x".repeat(2000)), false)]
    fn is_generated_cases(
        #[case] description: &str,
        #[case] path: &str,
        #[case] head: &str,
        #[case] want: bool,
    ) {
        assert_eq!(
            is_generated(Path::new(path), head.as_bytes()),
            want,
            "{description}"
        );
    }

    /// Each row: a patch, and whether it adds a generator marker.
    /// A base repo for [`unproven_generated`]: `x/gen.go` was generated at the base,
    /// `x/hand.go` was not, `gen/old.go` was generated (a rename's old side).
    fn base_repo() -> agent_testkit::FixtureRepo {
        let gen = "// Code generated by x. DO NOT EDIT.\n\npackage x\n";
        agent_testkit::FixtureRepo::new()
            .with_blob("base", "x/gen.go", gen)
            .with_blob("base", "x/hand.go", "package x\n")
            .with_blob("base", "gen/old.go", gen)
            .with_blob(
                "base",
                "x/lit.go",
                "package x\n\nvar s = \"// @generated\"\n",
            )
    }

    #[rstest]
    #[case::positive_generated_at_base("already generated before the PR", "x/gen.go", None, false)]
    #[case::positive_rename_of_generated(
        "a rename keeps its base side's status",
        "y/new.go",
        Some("gen/old.go"),
        false
    )]
    #[case::negative_hand_written_at_base(
        "the PR stamps a hand-written file",
        "x/hand.go",
        None,
        true
    )]
    #[case::negative_new_file(
        "a file the PR adds has no base to prove it",
        "x/added.pb.go",
        None,
        true
    )]
    #[case::corner_rename_into_generated_path(
        "a rename into a generated path is the PR's claim",
        "z/evil.pb.go",
        Some("x/hand.go"),
        true
    )]
    #[case::adversarial_marker_in_string(
        "a marker inside a string at the base is not a marker",
        "x/lit.go",
        None,
        true
    )]
    #[case::adversarial_traversal(
        "a base path that climbs out reads nothing",
        "x/a.go",
        Some("../../etc/passwd"),
        true
    )]
    #[tokio::test]
    async fn unproven_generated_cases(
        #[case] description: &str,
        #[case] path: &str,
        #[case] old: Option<&str>,
        #[case] unproven: bool,
    ) {
        let repo = base_repo();
        let got = unproven_generated(
            &repo,
            &agent_core::Revision::from("base".to_string()),
            vec![(path.to_string(), old.map(str::to_string))],
        )
        .await;
        assert_eq!(got.contains(path), unproven, "{description}");
    }

    #[tokio::test]
    async fn boundary_probes_past_the_cap_are_unproven() {
        // desc: only MAX_BASE_PROBES files are read; the rest stay hand-written.
        let repo = base_repo();
        let files = vec![("x/gen.go".to_string(), None); MAX_BASE_PROBES + 1];
        let first = unproven_generated(
            &repo,
            &agent_core::Revision::from("base".to_string()),
            files,
        )
        .await;
        assert!(
            first.contains("x/gen.go"),
            "the file past the cap is unproven"
        );
    }

    /// Each row: what the path names, and whether a head is read from it.
    #[rstest]
    #[case::positive_file("a regular file", "a.go", true)]
    #[case::negative_missing("a missing file", "nope.go", false)]
    #[case::negative_dir("a directory", "d", false)]
    #[case::adversarial_traversal("a path out of the repo", "../outside.go", false)]
    #[case::adversarial_absolute("an absolute path", "/etc/passwd", false)]
    fn read_head_cases(#[case] description: &str, #[case] rel: &str, #[case] want: bool) {
        let dir = agent_testkit::tempdir();
        let root = dir.join("repo");
        std::fs::create_dir_all(root.join("d")).unwrap();
        std::fs::write(root.join("a.go"), "x".repeat(5000)).unwrap();
        std::fs::write(dir.join("outside.go"), "secret").unwrap();
        let got = read_head(&root, rel);
        assert_eq!(got.is_some(), want, "{description}");
        if let Some(b) = got {
            assert_eq!(b.len(), GENERATED_HEAD_BYTES, "{description}: capped");
        }
    }

    /// A symlink in the repo that points out of it is not read.
    #[cfg(unix)]
    #[test]
    fn adversarial_read_head_symlink_escape() {
        let dir = agent_testkit::tempdir();
        let root = dir.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(dir.join("secret.go"), "// @generated").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.go"), root.join("link.go")).unwrap();
        assert!(read_head(&root, "link.go").is_none());
    }

    #[rstest]
    #[case::positive_https(
        "https://github.com/randomizedcoder/agent-seddon.git",
        "github.com",
        "randomizedcoder",
        "agent-seddon"
    )]
    #[case::positive_https_no_git("https://github.com/o/r", "github.com", "o", "r")]
    #[case::positive_scp("git@github.com:o/r.git", "github.com", "o", "r")]
    #[case::positive_ssh("ssh://git@gitlab.com/group/proj.git", "gitlab.com", "group", "proj")]
    #[case::corner_gitlab_subgroup("https://gitlab.com/a/b/c.git", "gitlab.com", "a", "c")]
    fn parses_known_remotes(
        #[case] url: &str,
        #[case] host: &str,
        #[case] owner: &str,
        #[case] repo: &str,
    ) {
        let (h, o, r) = parse_remote(url).expect("parses");
        assert_eq!((h.as_str(), o.as_str(), r.as_str()), (host, owner, repo));
    }

    #[rstest]
    #[case::adversarial_traversal("https://evil/../../x/y")]
    #[case::adversarial_scheme_missing("github.com/o/r")]
    #[case::adversarial_empty("")]
    #[case::adversarial_no_repo("https://github.com/onlyowner")]
    #[case::adversarial_dotdot_owner("git@h:../r.git")]
    fn fails_closed_on_hostile_remotes(#[case] url: &str) {
        assert!(parse_remote(url).is_none(), "should fail closed: {url}");
    }

    #[rstest]
    #[case::positive("main", true)]
    #[case::adversarial_traversal("..", false)]
    #[case::adversarial_slash("../heads/main", false)]
    #[case::adversarial_leading_dash("-x", false)]
    fn safe_segment_validates(#[case] s: &str, #[case] ok: bool) {
        assert_eq!(safe_segment(s), ok);
    }
}
