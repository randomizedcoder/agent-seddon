//! Policy implementations behind the `Policy` seam — the tool approval gate.

use agent_core::{Decision, Policy, ToolCall};
use async_trait::async_trait;

/// Approve every tool call. Convenient for unattended runs / experiments.
pub struct AutoApprove;

#[async_trait]
impl Policy for AutoApprove {
    async fn authorize(&self, _call: &ToolCall) -> Decision {
        Decision::Allow
    }
}

/// Map an operator's typed answer to a decision: `y`/`Y`/`yes` (whitespace
/// tolerated) ⇒ allow, anything else (including a bare Enter) ⇒ deny. Shared by
/// `Interactive` and its test double so the mapping is tested without a TTY.
fn decide_from_answer(answer: &str) -> Decision {
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        Decision::Allow
    } else {
        Decision::Deny("operator denied".into())
    }
}

/// Prompt the operator on stdin for each call (y/N).
pub struct Interactive;

#[async_trait]
impl Policy for Interactive {
    async fn authorize(&self, call: &ToolCall) -> Decision {
        let prompt = format!(
            "Allow tool `{}` with args {}? [y/N] ",
            call.name, call.arguments
        );
        // Block on a stdin read on a blocking thread so we don't stall the runtime.
        let answer = tokio::task::spawn_blocking(move || {
            use std::io::Write;
            print!("{prompt}");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            line
        })
        .await
        .unwrap_or_default();

        decide_from_answer(&answer)
    }
}

/// Allow only tool calls matching one of a set of `(tool_glob, arg_substring)`
/// rules; deny everything else. A rule matches when the tool name matches
/// `tool_glob` (a minimal `*` glob) and — if `arg_substring` is `Some` — the
/// call's serialized arguments contain that substring. Empty rule set ⇒ deny all.
///
/// Every denial carries the same opaque reason (`"not in allow-list"`), so a
/// caller can't distinguish "no matching rule" from "explicitly out of policy" —
/// no oracle for probing which tools/args are permitted.
pub struct AllowList {
    rules: Vec<(String, Option<String>)>,
}

impl AllowList {
    pub fn new(rules: Vec<(String, Option<String>)>) -> Self {
        Self { rules }
    }
}

const ALLOWLIST_DENY: &str = "not in allow-list";

#[async_trait]
impl Policy for AllowList {
    async fn authorize(&self, call: &ToolCall) -> Decision {
        // `to_string()` gives a stable serialized form to substring-match against.
        let args = call.arguments.to_string();
        for (tool_glob, arg_substring) in &self.rules {
            if !glob_match(tool_glob, &call.name) {
                continue;
            }
            match arg_substring {
                None => return Decision::Allow,
                Some(sub) if args.contains(sub.as_str()) => return Decision::Allow,
                // Tool matched but the required arg substring didn't — a later rule
                // may still allow this call, so keep looking.
                Some(_) => {}
            }
        }
        Decision::Deny(ALLOWLIST_DENY.into())
    }
}

/// Minimal glob match: `*` matches any (possibly empty) run of characters;
/// every other byte is literal. Enough for `read_file`, `git_*`, `*` families.
fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            Some(&c) => !t.is_empty() && t[0] == c && go(&p[1..], &t[1..]),
        }
    }
    go(pattern.as_bytes(), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "c0".into(),
            name: name.into(),
            arguments: args,
        }
    }

    /// An `Interactive` whose operator answer is injected instead of read from
    /// stdin, so the answer→decision mapping is testable without a TTY.
    struct ScriptedInteractive(&'static str);
    #[async_trait]
    impl Policy for ScriptedInteractive {
        async fn authorize(&self, _call: &ToolCall) -> Decision {
            decide_from_answer(self.0)
        }
    }

    // AutoApprove allows everything, including a destructive `bash`.
    #[rstest]
    #[case::positive_bash(call("bash", json!({"cmd": "rm -rf /"})))]
    #[case::positive_edit(call("edit", json!({"path": "x"})))]
    #[case::corner_empty_args(call("noop", json!({})))]
    #[tokio::test]
    async fn auto_approve_always_allows(#[case] c: ToolCall) {
        assert_eq!(AutoApprove.authorize(&c).await, Decision::Allow);
    }

    // Interactive maps a scripted answer to a decision (bare Enter ⇒ deny).
    #[rstest]
    #[case::positive_y("y", true)]
    #[case::positive_yes_ws("yes\n", true)]
    #[case::positive_upper("Y", true)]
    #[case::negative_n("n", false)]
    #[case::negative_empty("", false)]
    #[case::negative_garbage("maybe", false)]
    #[tokio::test]
    async fn interactive_maps_answer(#[case] answer: &'static str, #[case] allow: bool) {
        let dec = ScriptedInteractive(answer)
            .authorize(&call("edit", json!({})))
            .await;
        assert_eq!(dec == Decision::Allow, allow);
        if !allow {
            assert!(matches!(dec, Decision::Deny(_)));
        }
    }

    fn allowlist() -> AllowList {
        AllowList::new(vec![
            ("read_file".into(), None),         // any read
            ("bash".into(), Some("ls".into())), // only ls-ish bash
            ("git_*".into(), None),             // wildcard tool family
        ])
    }

    // AllowList allows matching tool+arg patterns and denies the rest, with a
    // uniform reason.
    #[rstest]
    #[case::positive_read_any("read_file", json!({"path": "a"}), true)]
    #[case::positive_bash_ls("bash", json!({"cmd": "ls -la"}), true)]
    #[case::positive_wildcard_git("git_diff", json!({}), true)]
    #[case::negative_bash_rm("bash", json!({"cmd": "rm -rf /"}), false)]
    #[case::negative_unlisted_tool("write_file", json!({}), false)]
    #[tokio::test]
    async fn allowlist_decides(
        #[case] tool: &str,
        #[case] args: serde_json::Value,
        #[case] allow: bool,
    ) {
        let dec = allowlist().authorize(&call(tool, args)).await;
        assert_eq!(dec == Decision::Allow, allow, "tool `{tool}`");
        if let Decision::Deny(reason) = dec {
            assert_eq!(reason, "not in allow-list"); // uniform: no why-oracle
        }
    }

    // An empty rule set denies everything.
    #[tokio::test]
    async fn allowlist_empty_denies_all() {
        let dec = AllowList::new(vec![])
            .authorize(&call("read_file", json!({})))
            .await;
        assert_eq!(dec, Decision::Deny("not in allow-list".into()));
    }

    // The glob matcher: literals, prefix `*`, `*` alone.
    #[rstest]
    #[case::exact("read_file", "read_file", true)]
    #[case::exact_mismatch("read_file", "write_file", false)]
    #[case::prefix_star("git_*", "git_diff", true)]
    #[case::prefix_star_empty_tail("git_*", "git_", true)]
    #[case::prefix_star_no_match("git_*", "bash", false)]
    #[case::star_all("*", "anything", true)]
    #[case::mid_star("a*z", "abcz", true)]
    #[case::mid_star_no_match("a*z", "abc", false)]
    fn glob_match_cases(#[case] pattern: &str, #[case] text: &str, #[case] expected: bool) {
        assert_eq!(glob_match(pattern, text), expected);
    }
}
