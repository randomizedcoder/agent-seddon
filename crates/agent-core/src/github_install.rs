//! The per-tenant GitHub-App **install card** and its registry seam (forge-auth
//! F4; design `docs/design/forge-auth/README.md`).
//!
//! A [`GithubInstall`] records one GitHub App *installation* a tenant owns and the
//! repositories that install selected: `{ installation_id, selected_repos[],
//! installed_at }`. The owning tenant is **not** a card field — it is the store's
//! tenant dimension in the `(collection, tenant, id)` key (exactly as [`ForgeCard`]
//! omits its tenant), so a card is only ever read or written inside a tenant scope.
//!
//! This module owns only the **host-agnostic** shape: the card, its fail-closed
//! validation, and the [`GithubInstallRegistry`] CRUD seam (mirroring
//! [`ForgeRegistry`]). The persisted store is `agent-forge`'s `StoreInstalls`; the
//! gRPC control plane is `GithubInstallRegistryService` (`agent-grpc`).
//!
//! Everything on the card is attacker-reachable (it rides the wire and comes back
//! from a store a tenant can drive): the `installation_id` is the exact decimal
//! shape the minter turns into a URL segment, and each `selected_repos` slug becomes
//! a path/ref segment downstream — so both are validated with the same primitives
//! the rest of the forge path uses ([`valid_installation_id`] / [`safe_segment`]),
//! and a rejection never echoes the offending value.
//!
//! [`ForgeCard`]: crate::ForgeCard
//! [`ForgeRegistry`]: crate::ForgeRegistry

use crate::{safe_segment, valid_installation_id, Error, Result};
use async_trait::async_trait;

/// Most repositories one install card may select. A GitHub App install can span
/// many repos, but an unbounded list is a DoS vector (it becomes a stored blob and
/// a per-request allocation), so the count is capped — generous headroom, not an
/// invitation. Mirrors the `MAX_REGISTRY_*` caps.
pub const MAX_SELECTED_REPOS: usize = 512;

/// A per-tenant GitHub-App installation record. The `installation_id` is the card
/// id (unique within its tenant); `selected_repos` are the flat `owner__name` slugs
/// the install granted; `installed_at` is Unix seconds (no date crate — mirrors the
/// minter's Unix-seconds clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubInstall {
    /// Decimal GitHub installation id (the card id; `<= MAX_INSTALLATION_ID_LEN`).
    pub installation_id: String,
    /// The repositories this install selected, each a flat `owner__name` slug
    /// (a single `safe_segment`). Capped at [`MAX_SELECTED_REPOS`].
    pub selected_repos: Vec<String>,
    /// When the install was recorded, Unix seconds.
    pub installed_at: u64,
}

impl GithubInstall {
    /// Fail-soft normalization on ingest. There are no hostile *numbers* to clamp
    /// (`installed_at` is opaque data), so this is a documented no-op kept for parity
    /// with the card ingest discipline (`sanitize` then `validate` on every write).
    pub fn sanitize(&mut self) {}

    /// Fail-closed structural validation, run before any write and after any load:
    /// - `installation_id` must be the decimal installation-id shape
    ///   ([`valid_installation_id`]) — the *same* rule the `ghapp:` ref parser uses,
    ///   so the store and the minter can never disagree about what is a valid id;
    /// - `selected_repos` must not exceed [`MAX_SELECTED_REPOS`];
    /// - every `selected_repos` entry must be a single path-safe segment
    ///   ([`safe_segment`]) — it becomes a path/ref segment downstream.
    ///
    /// A rejection never echoes the offending value (it may be a log-injection
    /// payload); it names the field.
    pub fn validate(&self) -> Result<()> {
        if !valid_installation_id(&self.installation_id) {
            return Err(Error::Config(
                "github install: installation_id must be decimal digits (<= 20)".into(),
            ));
        }
        if self.selected_repos.len() > MAX_SELECTED_REPOS {
            return Err(Error::Config(format!(
                "github install `{}`: too many selected_repos (> {MAX_SELECTED_REPOS})",
                self.installation_id
            )));
        }
        if let Some(bad) = self.selected_repos.iter().position(|r| !safe_segment(r)) {
            return Err(Error::Config(format!(
                "github install `{}`: selected_repos[{bad}] is not a path-safe slug",
                self.installation_id
            )));
        }
        Ok(())
    }
}

/// The install-registry seam (forge-auth F4): CRUD over the persisted
/// [`GithubInstall`]s. Mirrors [`ForgeRegistry`] — one process holds the store while
/// any number of clients drive it; a `PerTenant<dyn GithubInstallRegistry>` routes
/// each caller to their own tenant's installs.
///
/// [`ForgeRegistry`]: crate::ForgeRegistry
#[async_trait]
pub trait GithubInstallRegistry: Send + Sync {
    async fn list(&self) -> Result<Vec<GithubInstall>>;
    async fn get(&self, installation_id: &str) -> Result<GithubInstall>;
    /// Upsert (create + update). Implementations `sanitize` + `validate` before any
    /// write.
    async fn put(&self, install: GithubInstall) -> Result<GithubInstall>;
    /// Remove an install; `Ok(false)` when the id was absent (not an error).
    async fn delete(&self, installation_id: &str) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiKeyRef, MAX_INSTALLATION_ID_LEN};
    use rstest::rstest;

    fn install(id: &str, repos: &[&str]) -> GithubInstall {
        GithubInstall {
            installation_id: id.to_string(),
            selected_repos: repos.iter().map(ToString::to_string).collect(),
            installed_at: 1_700_000_000,
        }
    }

    /// `validate` is the attacker-reachable gate on an install card, so it is
    /// table-tested across the four classes plus the mandatory `adversarial_` set.
    #[rstest]
    // positive: a well-formed install with repos validates.
    #[case::positive_valid(install("12345", &["octo__hello", "octo__world"]), true)]
    // positive: an install that selected no repos is still a valid record.
    #[case::positive_no_repos_selected(install("12345", &[]), true)]
    // boundary: the longest decimal id (20 digits) is accepted.
    #[case::boundary_id_20_digits(install("99999999999999999999", &["a__b"]), true)]
    // negative: a non-decimal id is rejected.
    #[case::negative_id_non_digit(install("12a45", &["a__b"]), false)]
    // negative: an empty id is rejected.
    #[case::negative_empty_id(install("", &["a__b"]), false)]
    // negative: an empty repo slug is not a safe segment.
    #[case::negative_empty_repo_entry(install("12345", &[""]), false)]
    // adversarial: a 21-digit id is over the cap (hostile-large) → rejected.
    #[case::adversarial_id_over_len(install("123456789012345678901", &["a__b"]), false)]
    // adversarial: a traversal id is not a decimal id → rejected.
    #[case::adversarial_id_traversal(install("../x", &["a__b"]), false)]
    // adversarial: a traversal repo slug is rejected before it becomes a path.
    #[case::adversarial_repo_traversal(install("12345", &["../etc"]), false)]
    // adversarial: a separator in a repo slug is rejected (not a single segment).
    #[case::adversarial_repo_separator(install("12345", &["a/b"]), false)]
    // adversarial: a leading-dash repo slug (option-injection shape) is rejected.
    #[case::adversarial_repo_leading_dash(install("12345", &["-rf"]), false)]
    fn validate_matrix(#[case] card: GithubInstall, #[case] ok: bool) {
        assert_eq!(card.validate().is_ok(), ok);
    }

    // boundary: selected_repos exactly at the cap validates; cap+1 is rejected.
    #[test]
    fn boundary_repos_at_cap_then_over() {
        let at_cap: Vec<String> = (0..MAX_SELECTED_REPOS).map(|i| format!("r{i}")).collect();
        let mut c = GithubInstall {
            installation_id: "12345".into(),
            selected_repos: at_cap,
            installed_at: 0,
        };
        assert!(c.validate().is_ok(), "exactly at cap must validate");
        c.selected_repos.push("one__more".into());
        assert!(c.validate().is_err(), "cap+1 must be rejected");
    }

    // adversarial: a rejection never echoes a hostile repo slug (log-injection).
    #[test]
    fn adversarial_error_never_echoes_repo_value() {
        let err = install("12345", &["../secretpayload"])
            .validate()
            .expect_err("traversal repo must be rejected");
        assert!(!err.to_string().contains("secretpayload"), "leaked: {err}");
    }

    // The store's id rule and the `ghapp:` ref parser's id rule MUST agree, or an id
    // could be storable yet unmintable (or the reverse). Assert they decide every
    // sample identically — this is the whole point of the shared `valid_installation_id`.
    #[rstest]
    #[case::positive_plain("12345")]
    #[case::boundary_max_len("99999999999999999999")]
    #[case::negative_non_digit("12a45")]
    #[case::adversarial_over_len("123456789012345678901")]
    #[case::adversarial_traversal("../x")]
    #[case::adversarial_empty("")]
    fn installation_id_rule_matches_ref_parser(#[case] id: &str) {
        let via_predicate = valid_installation_id(id);
        let via_parser = matches!(
            ApiKeyRef::parse(&format!("ghapp:{id}")),
            Ok(ApiKeyRef::GitHubApp(_))
        );
        assert_eq!(
            via_predicate, via_parser,
            "id {id:?}: predicate={via_predicate} parser={via_parser} (len cap {MAX_INSTALLATION_ID_LEN})"
        );
    }
}
