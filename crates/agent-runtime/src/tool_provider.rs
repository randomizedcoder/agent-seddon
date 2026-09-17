//! [`agent_core::ToolProvider`] implementations (review-analysis-depth Inc 1).
//!
//! `PathToolProvider` trusts the process `PATH` — the nix-wrapped agent carries the
//! review toolbox (nix/review-tools.nix) on its runtime PATH — so it resolves a bare
//! tool name to itself. The `nix run`-backed general escape hatch (`NixRunToolProvider`)
//! is a later increment. Selected by `[review] tool_provider` and registered in
//! `register_builtins` like every other seam.

use agent_core::{ToolCommand, ToolProvider};

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
}
