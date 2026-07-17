//! LLM provider implementations behind the `LlmProvider` seam.
//!
//! v1 ships a single OpenAI-compatible chat-completions client, which covers a
//! large swath of providers (GLM, OpenAI, local vLLM/Ollama, …). Adding an
//! Anthropic-native or `genai`-wrapping provider is a matter of another impl
//! here — the loop never changes.

mod openai_compat;

pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};
