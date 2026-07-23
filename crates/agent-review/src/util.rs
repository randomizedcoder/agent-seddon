//! Small deterministic helpers, deliberately duplicated from private upstream
//! code (`agent-search`'s `lang_of`, `agent-git`'s `safe_segment`) rather than
//! widening those crates' public surface for an ~18-line map and a validator.

use agent_core::ForgeHost;
use std::path::Path;

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

/// Fail-closed single-segment validator (a copy of `agent-git`'s `safe_segment`).
/// Rejects empty, `.`/`..`, a leading `-`, and anything outside `[A-Za-z0-9._-]`
/// — blocking path traversal and ref injection in a caller-supplied branch name.
pub(crate) fn safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
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
