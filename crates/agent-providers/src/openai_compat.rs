//! An OpenAI-compatible `/chat/completions` provider.
//!
//! Tested against a GLM-5.2 server. GLM is a reasoning model: it emits a
//! `reasoning_content` field (which we log for debugging but do NOT resend, per
//! OpenAI convention) and only fills `content` once reasoning is done — so
//! `max_tokens` needs real headroom.

use agent_core::{
    ChunkStream, CompletionChunk, CompletionRequest, CompletionResponse, Error, LlmProvider,
    Message, ModelCapabilities, Result, Role, ToolCall, Usage,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

pub struct OpenAiCompatConfig {
    /// e.g. `https://host:8000/v1`
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub insecure_tls: bool,
    pub context_window: u32,
}

pub struct OpenAiCompatProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: String,
    context_window: u32,
}

impl OpenAiCompatProvider {
    pub fn new(cfg: OpenAiCompatConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(cfg.insecure_tls)
            .timeout(Duration::from_secs(600)) // reasoning models can be slow
            .build()
            .map_err(|e| Error::Provider(format!("building http client: {e}")))?;
        let endpoint = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
        Ok(Self {
            client,
            endpoint,
            model: cfg.model,
            api_key: cfg.api_key,
            context_window: cfg.context_window,
        })
    }

    fn build_wire<'a>(&'a self, req: &CompletionRequest, stream: bool) -> WireReq<'a> {
        WireReq {
            model: &self.model,
            messages: req.messages.iter().map(WireMsg::from_core).collect(),
            tools: req
                .tools
                .iter()
                .map(|t| WireTool {
                    typ: "function",
                    function: WireFn {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        parameters: t.parameters.clone(),
                    },
                })
                .collect(),
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stream,
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatProvider {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: true,
            context_window: self.context_window,
        }
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let wire = self.build_wire(&req, false);

        let resp = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&wire)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("request failed: {e}")))?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| Error::Provider(format!("reading body: {e}")))?;

        if !status.is_success() {
            return Err(Error::Provider(format!("http {status}: {body}")));
        }

        let parsed: WireResp = serde_json::from_str(&body)
            .map_err(|e| Error::Provider(format!("decoding response: {e}; body={body}")))?;

        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| Error::Provider("no choices in response".into()))?;

        if let Some(reasoning) = &choice.message.reasoning_content {
            if !reasoning.is_empty() {
                tracing::debug!(
                    chars = reasoning.len(),
                    "model reasoning_content (not resent)"
                );
            }
        }

        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments: parse_tool_args(&tc.function.arguments),
            })
            .collect();

        let message = Message {
            role: Role::Assistant,
            content: choice.message.content.unwrap_or_default(),
            tool_calls,
            tool_call_id: None,
        };

        Ok(CompletionResponse {
            message,
            finish_reason: choice.finish_reason.unwrap_or_else(|| "stop".into()),
            usage: parsed.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
                // OpenAI reports prompt-cache hits under `prompt_tokens_details.
                // cached_tokens`; there is no separate cache-write line (writes are
                // billed as normal input), so `cache_write_tokens` stays 0.
                cache_read_tokens: u.prompt_tokens_details.cached_tokens,
                cache_write_tokens: 0,
                cost: None,
            }),
        })
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        let wire = self.build_wire(&req, true);
        let resp = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&wire)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("http {status}: {body}")));
        }

        // Text streams directly; tool-call `arguments` arrive as fragments keyed
        // by `index` and are finalized when the stream ends. (Usage is not
        // requested in stream mode; use `stream=false` for token counts.)
        let stream = async_stream::stream! {
            let mut bytes = resp.bytes_stream();
            let mut buf = String::new();
            let mut tools_acc: BTreeMap<u32, ToolAcc> = BTreeMap::new();
            let mut finish: Option<String> = None;

            'read: while let Some(next) = bytes.next().await {
                let b = match next {
                    Ok(b) => b,
                    Err(e) => {
                        yield Err(Error::Provider(format!("stream error: {e}")));
                        break;
                    }
                };
                buf.push_str(&String::from_utf8_lossy(&b));
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim_end_matches('\r').to_string();
                    buf.drain(..pos + 1);
                    let Some(data) = line.strip_prefix("data:") else { continue };
                    let data = data.trim();
                    if data.is_empty() {
                        continue;
                    }
                    if data == "[DONE]" {
                        break 'read;
                    }
                    let ev: StreamEvent = match serde_json::from_str(data) {
                        Ok(ev) => ev,
                        Err(_) => continue,
                    };
                    for choice in ev.choices {
                        if let Some(delta) = choice.delta {
                            if let Some(text) = delta.content {
                                if !text.is_empty() {
                                    yield Ok(CompletionChunk { delta_text: text, ..Default::default() });
                                }
                            }
                            for tc in delta.tool_calls.unwrap_or_default() {
                                let acc = tools_acc.entry(tc.index).or_default();
                                if let Some(id) = tc.id {
                                    if !id.is_empty() {
                                        acc.id = id;
                                    }
                                }
                                if let Some(f) = tc.function {
                                    if let Some(n) = f.name {
                                        if !n.is_empty() {
                                            acc.name = n;
                                        }
                                    }
                                    if let Some(a) = f.arguments {
                                        acc.args.push_str(&a);
                                    }
                                }
                            }
                        }
                        if let Some(fr) = choice.finish_reason {
                            finish = Some(fr);
                        }
                    }
                }
            }

            for (_i, acc) in tools_acc {
                yield Ok(CompletionChunk { tool_call: Some(acc.into_tool_call()), ..Default::default() });
            }
            yield Ok(CompletionChunk {
                finish_reason: Some(finish.unwrap_or_else(|| "stop".into())),
                ..Default::default()
            });
        };
        Ok(Box::pin(stream))
    }
}

/// A streamed tool call being assembled from `arguments` fragments.
#[derive(Default)]
struct ToolAcc {
    id: String,
    name: String,
    args: String,
}

impl ToolAcc {
    fn into_tool_call(self) -> ToolCall {
        ToolCall {
            arguments: parse_tool_args(&self.args),
            id: self.id,
            name: self.name,
        }
    }
}

/// Parse a tool call's `arguments`, which OpenAI-compatible APIs send as a JSON
/// *string*. Empty/whitespace ⇒ empty object; unparseable ⇒ keep the raw text as a
/// string (so the model at least sees what it sent). Shared by the buffered and
/// streaming paths.
fn parse_tool_args(raw: &str) -> Value {
    if raw.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
    }
}

// --- wire types (streaming) -----------------------------------------------

#[derive(Deserialize)]
struct StreamEvent {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFn>,
}

#[derive(Deserialize)]
struct StreamFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

// --- wire types -----------------------------------------------------------

#[derive(Serialize)]
struct WireReq<'a> {
    model: &'a str,
    messages: Vec<WireMsg>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

#[derive(Serialize)]
struct WireMsg {
    role: &'static str,
    content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<WireToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl WireMsg {
    fn from_core(m: &Message) -> Self {
        WireMsg {
            role: m.role.as_str(),
            content: m.content.clone(),
            tool_calls: m
                .tool_calls
                .iter()
                .map(|tc| WireToolCall {
                    id: tc.id.clone(),
                    typ: "function",
                    function: WireToolCallFn {
                        name: tc.name.clone(),
                        arguments: serde_json::to_string(&tc.arguments)
                            .unwrap_or_else(|_| "{}".into()),
                    },
                })
                .collect(),
            tool_call_id: m.tool_call_id.clone(),
        }
    }
}

#[derive(Serialize)]
struct WireTool {
    #[serde(rename = "type")]
    typ: &'static str,
    function: WireFn,
}

#[derive(Serialize)]
struct WireFn {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Serialize)]
struct WireToolCall {
    id: String,
    #[serde(rename = "type")]
    typ: &'static str,
    function: WireToolCallFn,
}

#[derive(Serialize)]
struct WireToolCallFn {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct WireResp {
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireRespMsg,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireRespMsg {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireRespToolCall>>,
}

#[derive(Deserialize)]
struct WireRespToolCall {
    id: String,
    function: WireRespFn,
}

#[derive(Deserialize)]
struct WireRespFn {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: PromptTokensDetails,
}

/// The `usage.prompt_tokens_details` sub-object (OpenAI + compatible gateways);
/// `cached_tokens` is the prompt-cache hit count. Defaulted so providers that omit
/// it (vLLM, Ollama, older OpenAI) parse to zero.
#[derive(Deserialize, Default)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::parse_tool_args;
    use rstest::rstest;
    use serde_json::{json, Value};

    /// OpenAI sends tool-call `arguments` as a JSON string.
    #[rstest]
    #[case::positive_object("{\"a\":1}", json!({"a":1}))]
    #[case::positive_nested("{\"a\":{\"b\":[1,2]}}", json!({"a":{"b":[1,2]}}))]
    #[case::boundary_empty("", json!({}))]
    #[case::boundary_whitespace("   \n", json!({}))]
    #[case::negative_invalid_kept_as_string("not json", Value::String("not json".into()))]
    #[case::corner_partial_json("{oops", Value::String("{oops".into()))]
    fn parse_tool_args_cases(#[case] raw: &str, #[case] expected: Value) {
        assert_eq!(parse_tool_args(raw), expected);
    }
}
