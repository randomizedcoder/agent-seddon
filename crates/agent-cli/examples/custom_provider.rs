//! Out-of-tree extension example: register a custom provider on the `Registry`
//! and build the agent from it — no fork of agent-seddon required.
//!
//! Run with:  cargo run -p agent-cli --example custom_provider
//!
//! See `docs/extending.md` for the full contributor workflow.

use agent_core::{CompletionRequest, CompletionResponse, LlmProvider, Message, ModelCapabilities};
use agent_runtime::{build_agent_with, parse_config, register_builtins, Metrics, Registry};
use async_trait::async_trait;
use std::sync::Arc;

/// A trivial provider that ignores the request and returns a fixed answer.
/// A real one would call an API here (see `crates/agent-providers`).
struct EchoProvider;

#[async_trait]
impl LlmProvider for EchoProvider {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: false,
            context_window: 8192,
            supports_response_format: false,
        }
    }
    async fn complete(&self, _req: CompletionRequest) -> agent_core::Result<CompletionResponse> {
        Ok(CompletionResponse {
            message: Message::assistant("hello from a custom out-of-tree provider"),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Minimal config that selects our provider by name.
    let config = parse_config(
        r#"
        [agent]
        provider = "echo"
        policy = "auto-approve"
        stream = false
        [provider]
        model = "none"
    "#,
    )?;

    // Start from the built-ins, then register our own factory under "echo".
    let mut registry = Registry::new();
    register_builtins(&mut registry);
    registry.provider("echo", |_cfg| {
        Ok(Arc::new(EchoProvider) as Arc<dyn LlmProvider>)
    });

    let agent = build_agent_with(&registry, config, None, String::new(), Metrics::new()).await?;
    let answer = agent.run("say hi").await?;
    println!("{answer}");
    Ok(())
}
