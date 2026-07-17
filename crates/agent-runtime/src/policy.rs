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

        if matches!(answer.trim(), "y" | "Y" | "yes") {
            Decision::Allow
        } else {
            Decision::Deny("operator denied".into())
        }
    }
}
