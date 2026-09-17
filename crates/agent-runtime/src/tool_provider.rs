//! [`agent_core::ToolProvider`] implementations (review-analysis-depth Inc 1 + 5c).
//!
//! `PathToolProvider` trusts the process `PATH` — the nix-wrapped agent carries the
//! review toolbox (nix/review-tools.nix) on its runtime PATH — so it resolves a bare
//! tool name to itself. `NixRunToolProvider` (Inc 5c) is the general escape hatch:
//! it resolves a name to `nix run <locked-nixpkgs>#<name> --`, reaching **any** nixpkgs
//! package without vendoring it — fail-closed behind an operator allowlist and the
//! agent's own **locked** nixpkgs ref (so it stays reproducible). Selected by
//! `[review] tool_provider` and registered in `register_builtins` like every other seam.

use agent_core::{ToolCommand, ToolProvider};
use std::collections::BTreeSet;

/// Resolves an analysis-tool name to the bare program on `PATH`. Fail-closed on a
/// name that is not a plain tool identifier — a path separator or shell metacharacter
/// never belongs in one (names are fixed in-code, but this is defense-in-depth) → the
/// resolve returns `None` and the caller skips the tool.
#[derive(Debug, Default)]
pub(crate) struct PathToolProvider;

impl PathToolProvider {
    pub(crate) fn new() -> Self {
        Self
    }
}

/// A plain tool identifier: non-empty, ≤ 64 bytes, only `[A-Za-z0-9._-]`. Rejects
/// path separators, whitespace, and shell metacharacters.
fn is_plain_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[async_trait::async_trait]
impl ToolProvider for PathToolProvider {
    async fn resolve(&self, tool: &str) -> Option<ToolCommand> {
        is_plain_tool_name(tool).then(|| ToolCommand {
            program: tool.to_string(),
            prefix_args: Vec::new(),
        })
    }
}

/// Resolves an analysis-tool name to `nix run <locked-ref>#<name> --` (review-analysis-depth
/// Inc 5c) — the general escape hatch reaching any nixpkgs package on demand. **Fail-closed**
/// on three independent conditions, any of which yields `None` (the analyzer then skips the
/// tool): the name is not a plain tool identifier, it is not in the operator `allow`list, or
/// no locked flake ref is configured (an un-wrapped agent has none, so it can't run nix-run —
/// preserving reproducibility). The ref is the agent's own **locked** nixpkgs, baked onto the
/// wrapper (`AGENT_NIXPKGS_FLAKEREF`) from `flake.lock`, never the user's ambient registry.
#[derive(Debug)]
pub(crate) struct NixRunToolProvider {
    flake_ref: String,
    allow: BTreeSet<String>,
}

impl NixRunToolProvider {
    pub(crate) fn new(flake_ref: String, allow: impl IntoIterator<Item = String>) -> Self {
        Self {
            flake_ref,
            allow: allow.into_iter().collect(),
        }
    }
}

#[async_trait::async_trait]
impl ToolProvider for NixRunToolProvider {
    async fn resolve(&self, tool: &str) -> Option<ToolCommand> {
        if self.flake_ref.is_empty() || !is_plain_tool_name(tool) || !self.allow.contains(tool) {
            return None;
        }
        Some(ToolCommand {
            program: "nix".to_string(),
            prefix_args: vec![
                "run".to_string(),
                format!("{}#{tool}", self.flake_ref),
                "--".to_string(),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // positive: a real linter name resolves to itself, no prefix args.
    #[case::positive_golangci("golangci-lint".to_string(), true)]
    #[case::positive_gosec("gosec".to_string(), true)]
    // corner: dots + underscores are valid (e.g. tool variants).
    #[case::corner_dotted("go.vet_1".to_string(), true)]
    // boundary: exactly 64 chars is accepted; 65 is rejected.
    #[case::boundary_64("a".repeat(64), true)]
    #[case::boundary_65("a".repeat(65), false)]
    // negative: empty name resolves to nothing.
    #[case::negative_empty(String::new(), false)]
    // adversarial: path traversal, shell metachars, spaces, flags — all rejected,
    // so a hostile name can never become an executed program.
    #[case::adversarial_traversal("../../etc/passwd".to_string(), false)]
    #[case::adversarial_shell("gosec; rm -rf /".to_string(), false)]
    #[case::adversarial_space("go vet".to_string(), false)]
    #[case::adversarial_slash("bin/golangci-lint".to_string(), false)]
    #[case::adversarial_subst("$(whoami)".to_string(), false)]
    #[tokio::test]
    async fn path_provider_resolve(#[case] name: String, #[case] resolves: bool) {
        let got = PathToolProvider::new().resolve(&name).await;
        assert_eq!(got.is_some(), resolves, "resolve({name:?})");
        if let Some(cmd) = got {
            assert_eq!(cmd.program, name, "program is the bare name");
            assert!(cmd.prefix_args.is_empty(), "PATH provider adds no prefix");
        }
    }

    // --- NixRunToolProvider (Inc 5c) ------------------------------------------

    const REF: &str = "github:NixOS/nixpkgs/deadbeef";

    #[tokio::test]
    async fn positive_nixrun_resolves_allowlisted_to_locked_ref() {
        let p = NixRunToolProvider::new(REF.to_string(), ["gosec".to_string()]);
        let cmd = p.resolve("gosec").await.expect("allowlisted ⇒ resolves");
        assert_eq!(cmd.program, "nix");
        assert_eq!(
            cmd.prefix_args,
            vec![
                "run".to_string(),
                "github:NixOS/nixpkgs/deadbeef#gosec".to_string(),
                "--".to_string(),
            ],
            "locked ref + `--` separator"
        );
    }

    #[tokio::test]
    async fn negative_nixrun_name_not_in_allowlist_is_none() {
        let p = NixRunToolProvider::new(REF.to_string(), ["gosec".to_string()]);
        assert!(
            p.resolve("golangci-lint").await.is_none(),
            "a name absent from the allowlist never resolves"
        );
    }

    #[tokio::test]
    async fn corner_nixrun_empty_ref_is_none() {
        // An un-wrapped agent has no AGENT_NIXPKGS_FLAKEREF ⇒ empty ref ⇒ resolve nothing.
        let p = NixRunToolProvider::new(String::new(), ["gosec".to_string()]);
        assert!(p.resolve("gosec").await.is_none());
    }

    #[tokio::test]
    async fn corner_nixrun_empty_allowlist_denies_everything() {
        let p = NixRunToolProvider::new(REF.to_string(), std::iter::empty());
        assert!(
            p.resolve("gosec").await.is_none(),
            "empty allowlist ⇒ deny all"
        );
    }

    #[rstest]
    // Even an allowlisted-but-hostile name is fail-closed by the plain-name check, so a
    // traversal / shell metachar / flag can never reach the `nixpkgs#<name>` flake ref.
    #[case::traversal("../../etc/passwd")]
    #[case::shell("gosec; rm -rf /")]
    #[case::space("go vet")]
    #[case::subst("$(whoami)")]
    #[case::hash("evil#attr")]
    #[tokio::test]
    async fn adversarial_nixrun_hostile_name_is_none_even_if_allowlisted(#[case] name: &str) {
        // Put the hostile name IN the allowlist to prove `is_plain_tool_name` still gates it.
        let p = NixRunToolProvider::new(REF.to_string(), [name.to_string()]);
        assert!(
            p.resolve(name).await.is_none(),
            "hostile name rejected by the plain-name gate: {name:?}"
        );
    }
}
