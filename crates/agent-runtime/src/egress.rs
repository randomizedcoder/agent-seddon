//! Egress allow-list derivation (multi-tenancy C23-3c). Turns the operator's existing
//! config — LLM provider `base_url`s, the git-forge host, `[web] allow_hosts` — plus the
//! explicit `[sandbox.egress] allow_hosts` into the [`agent_egress::HostRule`] set the
//! loopback CONNECT proxy enforces. See `docs/design/multi-tenancy/01-process-isolation.md`.
//!
//! Kept in `agent-runtime` (which owns [`Config`]) so `agent-egress` stays config-agnostic.
//! Fail-closed: a malformed `base_url` is skipped (never panics), and an `enabled` egress
//! with an empty derived set blocks all egress.

use agent_egress::HostRule;

use crate::Config;

/// Companion hosts a git forge reaches beyond its API host (clone/asset/redirect targets),
/// keyed by `[forge] backend`. Used only when `[forge] base_url` is empty (the public
/// default); a custom/enterprise `base_url` contributes just its own host, and the operator
/// adds any companions via `[sandbox.egress] allow_hosts`.
fn forge_companion_hosts(backend: &str) -> &'static [&'static str] {
    match backend {
        "github" => &[
            "github.com",
            "api.github.com",
            "codeload.github.com",
            ".githubusercontent.com",
        ],
        "gitlab" => &["gitlab.com"],
        "gitea" => &["gitea.com"],
        "bitbucket" => &["bitbucket.org", "api.bitbucket.org"],
        _ => &[],
    }
}

/// Extract the bare host from a `scheme://host[:port]/path` URL. Returns `None` for
/// anything without a usable host (so a junk `base_url` is dropped, not fatal).
fn host_from_url(url: &str) -> Option<String> {
    let s = url.trim();
    let after = s.split_once("://").map(|(_, r)| r).unwrap_or(s);
    let authority = after.split(['/', '?', '#']).next().unwrap_or("");
    // Drop any userinfo (`user:pass@host`).
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    if authority.is_empty() {
        return None;
    }
    // Strip the port, handling an IPv6 literal `[::1]:443`.
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next()?.to_string()
    } else {
        authority.split(':').next().unwrap_or(authority).to_string()
    };
    (!host.is_empty()).then_some(host)
}

/// Build the egress allow-list from `cfg`. Returns an empty vec when egress is disabled.
/// The result feeds `agent_egress::HostMatcher::new`. Deterministic and side-effect-free.
pub fn derive_egress_allowlist(cfg: &Config) -> Vec<HostRule> {
    if !cfg.sandbox.egress.enabled {
        return Vec::new();
    }

    // Collect raw host tokens (hosts + `.suffix`/`*.suffix` globs), then dedup + parse.
    let mut tokens: Vec<String> = Vec::new();

    // 1. LLM provider. A configured `base_url` wins; the anthropic provider has a
    //    well-known default when it is empty (mirrors `anthropic_provider` in builder.rs).
    if let Some(h) = host_from_url(&cfg.provider.base_url) {
        tokens.push(h);
    } else if cfg.agent.provider == "anthropic" {
        tokens.push("api.anthropic.com".to_string());
    }

    // 2. Git forge (REST API host + companions). Only when a backend is configured.
    if !cfg.forge.backend.is_empty() {
        if let Some(h) = host_from_url(&cfg.forge.base_url) {
            tokens.push(h);
        } else {
            for c in forge_companion_hosts(&cfg.forge.backend) {
                tokens.push((*c).to_string());
            }
        }
    }

    // 3. `[web] allow_hosts` — so `web_fetch` keeps working under egress (its SSRF screen
    //    stays the inner check). These are host globs; `HostRule::parse` handles `*.`/`.`.
    tokens.extend(cfg.web.allow_hosts.iter().cloned());

    // 4. Operator extras.
    tokens.extend(cfg.sandbox.egress.allow_hosts.iter().cloned());

    // Dedup on the normalised token, preserving first-seen order, then parse (dropping
    // anything that is not a plausible host rule — fail-closed, never widens the list).
    let mut seen = std::collections::HashSet::new();
    tokens
        .into_iter()
        .filter(|t| seen.insert(t.trim().to_ascii_lowercase()))
        .filter_map(|t| HostRule::parse(&t))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal enabled config with the given provider/forge/web wiring.
    fn cfg_with(
        provider_kind: &str,
        provider_base: &str,
        forge_backend: &str,
        forge_base: &str,
        web_hosts: &[&str],
        extras: &[&str],
    ) -> Config {
        let mut c = Config::minimal_for_test();
        c.sandbox.egress.enabled = true;
        c.sandbox.egress.allow_hosts = extras.iter().map(ToString::to_string).collect();
        c.agent.provider = provider_kind.to_string();
        c.provider.base_url = provider_base.to_string();
        c.forge.backend = forge_backend.to_string();
        c.forge.base_url = forge_base.to_string();
        c.web.allow_hosts = web_hosts.iter().map(ToString::to_string).collect();
        c
    }

    fn has_exact(rules: &[HostRule], host: &str) -> bool {
        rules.contains(&HostRule::Exact(host.to_string()))
    }
    fn has_suffix(rules: &[HostRule], host: &str) -> bool {
        rules.contains(&HostRule::Suffix(host.to_string()))
    }

    #[test]
    fn positive_derives_provider_forge_web_and_extras() {
        let rules = derive_egress_allowlist(&cfg_with(
            "openai-compat",
            "https://kimi.example.com/v1",
            "github",
            "",
            &["docs.rs"],
            &["mirror.internal"],
        ));
        assert!(has_exact(&rules, "kimi.example.com"), "provider host");
        assert!(has_exact(&rules, "api.github.com"), "forge companion");
        assert!(
            has_suffix(&rules, "githubusercontent.com"),
            "forge suffix companion"
        );
        assert!(has_exact(&rules, "docs.rs"), "web allow_hosts");
        assert!(has_exact(&rules, "mirror.internal"), "operator extra");
    }

    #[test]
    fn positive_anthropic_default_host_when_base_url_empty() {
        let rules = derive_egress_allowlist(&cfg_with("anthropic", "", "", "", &[], &[]));
        assert!(has_exact(&rules, "api.anthropic.com"));
    }

    #[test]
    fn positive_custom_forge_base_url_used_without_companions() {
        let rules = derive_egress_allowlist(&cfg_with(
            "anthropic",
            "",
            "github",
            "https://ghe.corp.example/api/v3",
            &[],
            &[],
        ));
        assert!(has_exact(&rules, "ghe.corp.example"));
        assert!(
            !has_exact(&rules, "github.com"),
            "companions skipped for a custom base_url"
        );
    }

    #[test]
    fn negative_disabled_returns_empty() {
        let mut c = cfg_with("anthropic", "", "github", "", &["docs.rs"], &["x.example"]);
        c.sandbox.egress.enabled = false;
        assert!(derive_egress_allowlist(&c).is_empty());
    }

    #[test]
    fn corner_dedups_repeated_hosts() {
        // web + extras both name the same host ⇒ one rule.
        let rules = derive_egress_allowlist(&cfg_with(
            "anthropic",
            "",
            "",
            "",
            &["dup.example"],
            &["dup.example", "DUP.example"],
        ));
        let n = rules
            .iter()
            .filter(|r| **r == HostRule::Exact("dup.example".into()))
            .count();
        assert_eq!(n, 1, "case-insensitive dedup");
    }

    #[test]
    fn adversarial_hostile_base_url_skipped_not_panic() {
        // A garbage provider base_url must be dropped, not derived, not panic.
        let rules = derive_egress_allowlist(&cfg_with(
            "openai-compat",
            "not a url ::: @@@",
            "",
            "",
            &[],
            &[],
        ));
        assert!(rules.is_empty(), "junk base_url derives nothing");
    }
}
