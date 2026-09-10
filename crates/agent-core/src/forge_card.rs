//! The forge (git-host) config card and its registry seam (config design C36,
//! increment D1).
//!
//! A [`ForgeCard`] lifts the three hardcoded forge blockers into config: the
//! backend allow-list (`kind` now selects a *registered* impl factory, so "known
//! kinds = whatever is built into the binary"), the per-host default `base_url`
//! (empty ⇒ the kind's registered default, owned by the impl), and the per-host
//! repo-encoding (declared on the card, travels with it).
//!
//! This module owns only the **host-agnostic** shape: the card, its validation, and
//! the [`ForgeRegistry`] CRUD seam. Host-specific knowledge — the per-kind default
//! `base_url`, the repo-slug decoding, and the SSRF screen on `base_url` — lives with
//! the impls in `agent-forge`, so adding a host is a new impl + a factory line and
//! **no edit to this core allow-list** (there no longer is one).

use crate::{safe_segment, ApiKeyRef, Error, Result};
use async_trait::async_trait;

/// How a flat `owner__name` repo slug maps to a host's API path. Declared on the
/// card (per C36) rather than baked per-backend in the build path, so the encoding
/// travels with the forge. Rides the wire as a validated string (see
/// [`RepoEncoding::parse`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoEncoding {
    /// GitHub-style: split `owner__name` into `(owner, name)`.
    OwnerName,
    /// GitLab-style: `__` → `/` (a `group/subgroup/name` path, percent-encoded by
    /// the impl).
    Path,
}

impl RepoEncoding {
    pub fn as_str(&self) -> &'static str {
        match self {
            RepoEncoding::OwnerName => "owner_name",
            RepoEncoding::Path => "path",
        }
    }
    /// Parse the wire string. Empty or unknown → `None` (the caller rejects it as
    /// fail-closed: an absent/garbage encoding must never silently pick a default).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "owner_name" => RepoEncoding::OwnerName,
            "path" => RepoEncoding::Path,
            _ => return None,
        })
    }
}

/// Clamp bounds for a hostile `timeout_secs` / `max_retries` on ingest.
const TIMEOUT_MIN: u32 = 1;
const TIMEOUT_MAX: u32 = 300;
const RETRIES_MAX: u32 = 10;

/// A git-host card. `kind` selects the `Forge` impl factory (validated at build
/// time against the registered kinds, not a hardcoded list here); `base_url` empty
/// ⇒ the kind's registered default; `token_ref` is a reference, never a raw token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeCard {
    /// Path-safe forge id.
    pub id: String,
    /// The impl factory selector: `github` | `gitlab` | (future hosts).
    pub kind: String,
    /// A disabled card is stored but not built into a live forge.
    pub enabled: bool,
    /// Empty ⇒ the kind's registered default; else an override (SSRF-screened when
    /// the operational forge is built).
    pub base_url: String,
    /// `env:NAME` | `file:/path` — never a raw token.
    pub token_ref: String,
    /// How the repo slug maps to the host API path.
    pub repo_encoding: RepoEncoding,
    /// HTTP timeout (seconds), clamped by [`ForgeCard::sanitize`].
    pub timeout_secs: u32,
    /// HTTP retry budget, clamped by [`ForgeCard::sanitize`].
    pub max_retries: u32,
}

impl ForgeCard {
    /// Clamp hostile numbers on ingest (a model/operator value is attacker-reachable):
    /// `timeout_secs` into `[1, 300]`, `max_retries` to `<= 10`.
    pub fn sanitize(&mut self) {
        self.timeout_secs = self.timeout_secs.clamp(TIMEOUT_MIN, TIMEOUT_MAX);
        self.max_retries = self.max_retries.min(RETRIES_MAX);
    }

    /// Fail-closed structural validation, run before any write:
    /// - `id` must be a path-safe segment (it may become a storage key);
    /// - `kind` must be non-empty (the *known-kind* check is at build time, where
    ///   the registered factories live — an unknown kind fails closed there,
    ///   listing the known kinds);
    /// - `token_ref` must be empty or `env:`/`file:` — a raw secret is rejected and
    ///   never echoed;
    /// - `base_url`, when set, must be a syntactic `http(s)://host` URL (the SSRF
    ///   screen on private/loopback hosts is applied when the operational forge is
    ///   built, in `agent-forge`).
    ///
    /// `repo_encoding` needs no check here: it is already a parsed enum (an absent
    /// or unknown wire value was rejected at the wire→core boundary).
    pub fn validate(&self) -> Result<()> {
        if !safe_segment(&self.id) {
            return Err(Error::Config(format!("invalid forge id `{}`", self.id)));
        }
        if self.kind.trim().is_empty() {
            return Err(Error::Config(format!(
                "forge card `{}`: kind must not be empty",
                self.id
            )));
        }
        // A raw token here is exactly the secret-in-config mistake the reference
        // type prevents; the error never echoes the value.
        ApiKeyRef::parse(&self.token_ref)
            .map_err(|e| Error::Config(format!("forge card `{}`: token_ref {e}", self.id)))?;
        if !self.base_url.is_empty() {
            let ok = self.base_url.starts_with("http://") || self.base_url.starts_with("https://");
            if !ok {
                return Err(Error::Config(format!(
                    "forge card `{}`: base_url must be an http(s) URL",
                    self.id
                )));
            }
        }
        Ok(())
    }
}

/// The forge-registry seam (config design C36): CRUD over the persisted
/// [`ForgeCard`]s. Mirrors [`crate::RoleRegistry`] / the provider registry — one
/// process holds the store while any number of clients drive it.
#[async_trait]
pub trait ForgeRegistry: Send + Sync {
    async fn list(&self) -> Result<Vec<ForgeCard>>;
    async fn get(&self, id: &str) -> Result<ForgeCard>;
    /// Upsert (create + update). Implementations `sanitize` + `validate` before any
    /// write.
    async fn put(&self, card: ForgeCard) -> Result<ForgeCard>;
    /// Remove a card; `Ok(false)` when the id was absent (not an error).
    async fn delete(&self, id: &str) -> Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn card(id: &str, kind: &str, base_url: &str, token_ref: &str) -> ForgeCard {
        ForgeCard {
            id: id.to_string(),
            kind: kind.to_string(),
            enabled: true,
            base_url: base_url.to_string(),
            token_ref: token_ref.to_string(),
            repo_encoding: RepoEncoding::OwnerName,
            timeout_secs: 30,
            max_retries: 3,
        }
    }

    #[rstest]
    // desc: a well-formed github card validates → expect Ok.
    #[case::positive_ok("gh", "github", "", "env:GH_TOKEN", true)]
    // desc: an explicit self-hosted https base_url validates → expect Ok.
    #[case::positive_self_hosted(
        "gl",
        "gitlab",
        "https://gitlab.example.com/api/v4",
        "env:GL",
        true
    )]
    // desc: a file: token reference validates → expect Ok.
    #[case::positive_file_token("gh", "github", "", "file:/run/secrets/gh", true)]
    // desc: an empty token_ref means "no token" and is allowed → expect Ok.
    #[case::boundary_empty_token("gh", "github", "", "", true)]
    // desc: an empty id is not a path-safe segment → expect Err.
    #[case::negative_empty_id("", "github", "", "env:X", false)]
    // desc: an empty kind is rejected → expect Err.
    #[case::negative_empty_kind("gh", "", "", "env:X", false)]
    // desc: a non-http base_url is rejected → expect Err.
    #[case::negative_bad_scheme("gh", "github", "ftp://h/x", "env:X", false)]
    // adversarial: a traversal id is not a safe segment → expect Err.
    #[case::adversarial_traversal_id("../etc", "github", "", "env:X", false)]
    // adversarial: a separator id is rejected → expect Err.
    #[case::adversarial_separator_id("a/b", "github", "", "env:X", false)]
    // adversarial: a raw secret in token_ref is rejected (must be env:/file:) → Err.
    #[case::adversarial_raw_token("gh", "github", "", "ghp_deadbeefdeadbeef", false)]
    fn validate_matrix(
        #[case] id: &str,
        #[case] kind: &str,
        #[case] base_url: &str,
        #[case] token_ref: &str,
        #[case] ok: bool,
    ) {
        assert_eq!(card(id, kind, base_url, token_ref).validate().is_ok(), ok);
    }

    // adversarial: the raw-secret rejection error never echoes the token value.
    #[test]
    fn adversarial_token_ref_error_never_echoes_secret() {
        let err = card("gh", "github", "", "ghp_supersecretvalue")
            .validate()
            .expect_err("raw token must be rejected");
        assert!(!err.to_string().contains("supersecret"), "leaked: {err}");
    }

    #[rstest]
    // desc (boundary): a hostile-large timeout is clamped to the max.
    #[case::timeout_huge(u32::MAX, 0, TIMEOUT_MAX, 0)]
    // desc (boundary): a zero timeout is clamped up to the min.
    #[case::timeout_zero(0, 3, TIMEOUT_MIN, 3)]
    // desc (boundary): a hostile-large retry budget is clamped to the max.
    #[case::retries_huge(30, u32::MAX, 30, RETRIES_MAX)]
    // desc (positive): in-range values are preserved.
    #[case::in_range(45, 2, 45, 2)]
    fn sanitize_clamps(
        #[case] t_in: u32,
        #[case] r_in: u32,
        #[case] t_out: u32,
        #[case] r_out: u32,
    ) {
        let mut c = card("gh", "github", "", "env:X");
        c.timeout_secs = t_in;
        c.max_retries = r_in;
        c.sanitize();
        assert_eq!(c.timeout_secs, t_out);
        assert_eq!(c.max_retries, r_out);
    }

    #[rstest]
    #[case::owner_name(RepoEncoding::OwnerName)]
    #[case::path(RepoEncoding::Path)]
    fn repo_encoding_round_trips(#[case] e: RepoEncoding) {
        assert_eq!(RepoEncoding::parse(e.as_str()), Some(e));
    }

    #[rstest]
    // adversarial/negative: an absent or unknown encoding never picks a default.
    #[case::empty("")]
    #[case::unknown("subgroup")]
    #[case::injection("path; drop")]
    fn repo_encoding_parse_rejects(#[case] s: &str) {
        assert_eq!(RepoEncoding::parse(s), None);
    }
}
