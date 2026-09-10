//! Per-kind forge construction (config design C36, increment D1).
//!
//! This module is the ONE place a [`ForgeCard`] (or a fleet row's inline
//! backend/base_url/repo) turns into a live `Forge`. It owns the host-specific
//! knowledge the design lifts out of core + the runtime wiring:
//!
//! - **known kinds** = whatever host impls are built into this binary (a
//!   feature-gated list); an unknown kind fails closed, listing the known kinds —
//!   there is no hardcoded allow-list in `agent-core` any more.
//! - **default `base_url`** per kind (empty on the card ⇒ this default).
//! - **repo decoding** — how a flat `owner__name` slug becomes the host's API
//!   path — driven by the card's declared [`RepoEncoding`].
//! - the **SSRF screen** on an overriding `base_url` (loopback/private hosts
//!   refused), best-effort defense-in-depth on the operational forge.

use std::net::IpAddr;
use std::sync::Arc;

use agent_core::{safe_segment, Error, Forge, ForgeCard, RepoEncoding, Result, Secret};

/// The forge kinds built into this binary. The card's `kind` is validated against
/// this at build time — "known kinds = registered kinds", not a core allow-list.
pub fn known_kinds() -> Vec<&'static str> {
    [
        #[cfg(feature = "forge-github")]
        "github",
        #[cfg(feature = "forge-gitlab")]
        "gitlab",
    ]
    .to_vec()
}

/// The kind's registered default `base_url` (used when a card leaves `base_url`
/// empty). `None` for an unknown/unbuilt kind.
pub fn default_base_url(kind: &str) -> Option<&'static str> {
    match kind {
        #[cfg(feature = "forge-github")]
        "github" => Some("https://api.github.com"),
        #[cfg(feature = "forge-gitlab")]
        "gitlab" => Some("https://gitlab.com/api/v4"),
        _ => None,
    }
}

/// The repo-encoding a kind requires (also the default for a fleet row that carries
/// no explicit encoding). `None` for an unknown/unbuilt kind.
pub fn expected_encoding(kind: &str) -> Option<RepoEncoding> {
    match kind {
        #[cfg(feature = "forge-github")]
        "github" => Some(RepoEncoding::OwnerName),
        #[cfg(feature = "forge-gitlab")]
        "gitlab" => Some(RepoEncoding::Path),
        _ => None,
    }
}

fn unknown_kind(kind: &str) -> Error {
    Error::Config(format!("unknown forge kind `{kind}` (known: {})", {
        let k = known_kinds();
        if k.is_empty() {
            "<none — check enabled cargo features>".to_string()
        } else {
            k.join(", ")
        }
    }))
}

/// Decode a GitHub `owner__name` slug into `(owner, name)`. Both halves must be
/// non-empty path-safe segments — a traversal / separator slug is rejected.
pub fn decode_owner_name(repo: &str) -> Result<(String, String)> {
    let (owner, name) = repo
        .split_once("__")
        .filter(|(o, n)| !o.is_empty() && !n.is_empty())
        .ok_or_else(|| Error::Config("github repo must be `owner__name`".to_string()))?;
    if !safe_segment(owner) || !safe_segment(name) {
        return Err(Error::Config(format!("hostile repo slug `{repo}`")));
    }
    Ok((owner.to_string(), name.to_string()))
}

/// Decode a GitLab slug (`group__subgroup__name`) into a `group/subgroup/name`
/// path. Each segment must be a non-empty path-safe segment; the impl
/// percent-encodes the `/`.
pub fn decode_path(repo: &str) -> Result<String> {
    let path = repo.replace("__", "/");
    if path.is_empty() {
        return Err(Error::Config("gitlab repo must not be empty".to_string()));
    }
    for seg in path.split('/') {
        if seg.is_empty() || !safe_segment(seg) {
            return Err(Error::Config(format!("hostile repo slug `{repo}`")));
        }
    }
    Ok(path)
}

/// Screen an overriding `base_url`: it must be an `http(s)://host` URL whose host is
/// not loopback/private/link-local. Best-effort defense-in-depth against SSRF on the
/// operational forge — it screens literal IP hosts and obvious local names; it does
/// NOT resolve DNS (that is a network sandbox's job, not this guard's).
pub fn screen_base_url(url: &str) -> Result<()> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| Error::Config(format!("invalid base_url: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        s => return Err(Error::Config(format!("base_url scheme `{s}` not allowed"))),
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Config("base_url has no host".to_string()))?;
    // IPv6 literals arrive bracketed in the URL host.
    let h = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".local") || h.ends_with(".internal") {
        return Err(Error::Config(format!(
            "base_url host `{host}` is local (SSRF screen)"
        )));
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        let blocked = match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            IpAddr::V6(v6) => {
                // loopback / unspecified / IPv4-mapped-private handled coarsely, plus
                // ULA fc00::/7.
                v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00
            }
        };
        if blocked {
            return Err(Error::Config(format!(
                "base_url host `{host}` is private/loopback (SSRF screen)"
            )));
        }
    }
    Ok(())
}

/// Build a live `Forge` from a card and a per-use repo slug + resolved token.
///
/// Resolves the base URL (card override, SSRF-screened; else the kind default),
/// validates the card's `repo_encoding` matches the kind, decodes the slug, and
/// dispatches to the impl. An unknown kind fails closed with the known kinds listed.
pub fn build_forge_from_card(
    card: &ForgeCard,
    repo: &str,
    token: Secret,
) -> Result<Arc<dyn Forge>> {
    let expected = expected_encoding(&card.kind).ok_or_else(|| unknown_kind(&card.kind))?;
    if card.repo_encoding != expected {
        return Err(Error::Config(format!(
            "forge kind `{}` requires repo_encoding `{}` (got `{}`)",
            card.kind,
            expected.as_str(),
            card.repo_encoding.as_str()
        )));
    }
    let base = if card.base_url.is_empty() {
        default_base_url(&card.kind)
            .ok_or_else(|| unknown_kind(&card.kind))?
            .to_string()
    } else {
        screen_base_url(&card.base_url)?;
        card.base_url.clone()
    };
    let timeout = card.timeout_secs.clamp(1, 300) as u64;
    let retries = card.max_retries.min(10);
    match card.kind.as_str() {
        #[cfg(feature = "forge-github")]
        "github" => {
            let (owner, name) = decode_owner_name(repo)?;
            Ok(Arc::new(crate::GitHubForge::new(
                base, owner, name, token, timeout, retries,
            )?))
        }
        #[cfg(feature = "forge-gitlab")]
        "gitlab" => {
            let project = decode_path(repo)?;
            Ok(Arc::new(crate::GitLabForge::new(
                base, project, token, timeout, retries,
            )?))
        }
        other => Err(unknown_kind(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn card(kind: &str, base_url: &str, enc: RepoEncoding) -> ForgeCard {
        ForgeCard {
            id: "f".into(),
            kind: kind.into(),
            enabled: true,
            base_url: base_url.into(),
            token_ref: "env:X".into(),
            repo_encoding: enc,
            timeout_secs: 30,
            max_retries: 3,
        }
    }

    // desc (positive): a github card with an empty base_url builds a github forge
    // against the kind's registered default → expect Ok and name == "github".
    // `Arc<dyn Forge>` is not `Debug`, so use match instead of `.expect`/`.expect_err`.
    fn built(r: Result<Arc<dyn Forge>>) -> Arc<dyn Forge> {
        match r {
            Ok(f) => f,
            Err(e) => panic!("build failed: {e}"),
        }
    }
    fn build_err(r: Result<Arc<dyn Forge>>) -> Error {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        }
    }

    #[test]
    fn positive_github_card_builds_forge() {
        let f = built(build_forge_from_card(
            &card("github", "", RepoEncoding::OwnerName),
            "octocat__hello",
            Secret::from("t".to_string()),
        ));
        assert_eq!(f.name(), "github");
    }

    // desc (positive): a self-hosted gitlab base_url overrides the kind default and
    // still builds → expect Ok and name == "gitlab".
    #[test]
    fn positive_self_hosted_gitlab_base_url() {
        let f = built(build_forge_from_card(
            &card(
                "gitlab",
                "https://gitlab.example.com/api/v4",
                RepoEncoding::Path,
            ),
            "group__project",
            Secret::from("t".to_string()),
        ));
        assert_eq!(f.name(), "gitlab");
    }

    // desc (boundary): an empty base_url resolves to the kind's registered default
    // (github/gitlab both have one) → both build.
    #[test]
    fn boundary_empty_base_url_uses_kind_default() {
        assert!(default_base_url("github").is_some());
        assert!(default_base_url("gitlab").is_some());
        assert!(build_forge_from_card(
            &card("github", "", RepoEncoding::OwnerName),
            "o__n",
            Secret::from("t".to_string())
        )
        .is_ok());
    }

    // desc (negative): an unknown kind is rejected, and the error lists the known
    // kinds (fail-closed, not a silent no-op).
    #[test]
    fn negative_unknown_backend_rejected() {
        let err = build_err(build_forge_from_card(
            &card("bitbucket", "", RepoEncoding::OwnerName),
            "o__n",
            Secret::from("t".to_string()),
        ));
        let msg = err.to_string();
        assert!(msg.contains("unknown forge kind"), "got: {msg}");
        assert!(msg.contains("github"), "must list known kinds: {msg}");
    }

    // desc (negative): a repo_encoding that does not match the kind is rejected.
    #[test]
    fn negative_mismatched_encoding_rejected() {
        let err = build_err(build_forge_from_card(
            &card("github", "", RepoEncoding::Path),
            "o__n",
            Secret::from("t".to_string()),
        ));
        assert!(err.to_string().contains("requires repo_encoding"));
    }

    // desc (positive): the gitlab path encoding maps `group__subgroup__name` to a
    // `group/subgroup/name` path.
    #[test]
    fn positive_gitlab_subgroup_encoding() {
        assert_eq!(
            decode_path("group__subgroup__name").expect("decode"),
            "group/subgroup/name"
        );
    }

    #[rstest]
    // desc (corner): dots and dashes in a slug are preserved, not mangled.
    #[case::owner_name("my.org__a-b")]
    fn corner_repo_with_dots_and_dashes_preserved(#[case] repo: &str) {
        let (o, n) = decode_owner_name(repo).expect("decode");
        assert_eq!((o.as_str(), n.as_str()), ("my.org", "a-b"));
    }

    #[rstest]
    // adversarial: a traversal / separator slug is rejected on both encodings.
    #[case::traversal("..__x")]
    #[case::separator("a/b__c")]
    #[case::single("no-separator")]
    fn adversarial_hostile_repo_slug_rejected(#[case] repo: &str) {
        assert!(decode_owner_name(repo).is_err(), "owner_name {repo}");
    }

    #[rstest]
    #[case::traversal("a__..__c")]
    #[case::abs("a__/etc/passwd")]
    fn adversarial_hostile_path_slug_rejected(#[case] repo: &str) {
        assert!(decode_path(repo).is_err(), "path {repo}");
    }

    #[rstest]
    // adversarial: a private/loopback base_url on the operational forge is screened.
    #[case::loopback_v4("http://127.0.0.1/api")]
    #[case::private_v4("https://10.0.0.5/api/v4")]
    #[case::private_192("https://192.168.1.10/api")]
    #[case::link_local("https://169.254.169.254/latest/meta-data")]
    #[case::localhost("http://localhost:8080/api")]
    #[case::loopback_v6("http://[::1]/api")]
    #[case::dot_local("https://gitlab.local/api/v4")]
    #[case::bad_scheme("ftp://example.com/api")]
    fn adversarial_base_url_ssrf_screened(#[case] url: &str) {
        assert!(screen_base_url(url).is_err(), "must screen {url}");
    }

    #[rstest]
    // desc (positive): public hosts pass the screen.
    #[case::github("https://api.github.com")]
    #[case::self_hosted("https://gitlab.example.com/api/v4")]
    #[case::public_ip("https://8.8.8.8/api")]
    fn positive_public_base_url_passes_screen(#[case] url: &str) {
        assert!(screen_base_url(url).is_ok(), "should pass {url}");
    }
}
