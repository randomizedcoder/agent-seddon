//! GitHub App install onboarding (forge-auth F5c2): the install URL `BeginInstall`
//! hands the portal, and how long its `state` stays redeemable.
//!
//! The single-use `state` table itself is [`agent_core::pending::PendingTable`], shared
//! with browser sign-in; `agent-runtime`'s `install_gate` holds one keyed to the caller
//! that started the onboarding.

/// How long a `state` from `BeginInstall` stays redeemable. An install (picking repos,
/// maybe approving an org request) takes longer than a sign-in, but not much.
pub const STATE_TTL_SECS: u64 = 900;
/// Onboardings started and not yet finished, across every tenant. A full table refuses
/// new ones rather than evicting someone mid-install.
pub const MAX_PENDING: usize = 1024;

/// The GitHub App install URL for `slug` carrying `state`:
/// `{web_base}/apps/{slug}/installations/new?state={state}`. `None` when `slug` is not a
/// GitHub slug (it becomes a path segment) or `web_base` is not an http(s) URL.
pub fn install_url(web_base: &str, slug: &str, state: &str) -> Option<String> {
    if !crate::ghapp::valid_github_login(slug) {
        return None;
    }
    let mut url = reqwest::Url::parse(web_base.trim_end_matches('/')).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.cannot_be_a_base() {
        return None;
    }
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(["apps", slug, "installations", "new"]);
    url.query_pairs_mut().clear().append_pair("state", state);
    Some(url.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::positive_github(
        "https://github.com",
        "my-app",
        Some("https://github.com/apps/my-app/installations/new?state=abc")
    )]
    #[case::positive_trailing_slash(
        "https://github.com/",
        "my-app",
        Some("https://github.com/apps/my-app/installations/new?state=abc")
    )]
    #[case::positive_ghes_host(
        "https://ghe.example.com",
        "app1",
        Some("https://ghe.example.com/apps/app1/installations/new?state=abc")
    )]
    #[case::negative_empty_slug("https://github.com", "", None)]
    #[case::negative_not_a_url("github.com", "my-app", None)]
    #[case::corner_non_http_scheme("ftp://github.com", "my-app", None)]
    #[case::adversarial_traversal_slug("https://github.com", "../../evil", None)]
    #[case::adversarial_query_slug("https://github.com", "app?x=1", None)]
    #[case::adversarial_separator_slug("https://github.com", "a/b", None)]
    fn install_url_cases(#[case] base: &str, #[case] slug: &str, #[case] want: Option<&str>) {
        assert_eq!(install_url(base, slug, "abc").as_deref(), want);
    }

    // adversarial: a hostile state value is query-encoded, never spliced into the URL.
    #[test]
    fn adversarial_state_is_query_encoded() {
        let url = install_url("https://github.com", "my-app", "a&redirect=https://evil").unwrap();
        assert!(
            url.ends_with("?state=a%26redirect%3Dhttps%3A%2F%2Fevil"),
            "{url}"
        );
    }
}
