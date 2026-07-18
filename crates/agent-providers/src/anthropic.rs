//! An Anthropic-native provider (the Messages API).
//!
//! Unlike the OpenAI-compatible wire format, Anthropic keeps the system prompt
//! out of the message list, represents tool calls/results as typed content
//! blocks, and requires user/assistant roles to alternate. This impl converts
//! our normalized `Message` list into that shape (coalescing consecutive
//! same-role turns — e.g. several tool results after one assistant turn — into a
//! single message) and parses the typed response back into a `CompletionResponse`.

use agent_core::{
    ChunkStream, CompletionChunk, CompletionRequest, CompletionResponse, Error, LlmProvider,
    Message, ModelCapabilities, Result, Role, ToolCall, Usage,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Duration;

pub struct AnthropicConfig {
    /// e.g. `https://api.anthropic.com/v1`
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    /// The `anthropic-version` header (e.g. `2023-06-01`).
    pub version: String,
    pub context_window: u32,
}

pub struct AnthropicProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: String,
    version: String,
    context_window: u32,
}

impl AnthropicProvider {
    pub fn new(cfg: AnthropicConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|e| Error::Provider(format!("building http client: {e}")))?;
        let endpoint = format!("{}/messages", cfg.base_url.trim_end_matches('/'));
        Ok(Self {
            client,
            endpoint,
            model: cfg.model,
            api_key: cfg.api_key,
            version: cfg.version,
            context_window: cfg.context_window,
        })
    }

    /// Build the JSON request body from a normalized `CompletionRequest`.
    fn build_body(&self, req: &CompletionRequest, stream: bool) -> Value {
        let (system, messages) = to_anthropic_messages(&req.messages);
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "messages": messages,
        });
        if stream {
            body["stream"] = Value::Bool(true);
        }
        if !system.is_empty() {
            body["system"] = Value::String(system);
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        body
    }

    /// Fire the request and return the raw response (shared by complete/stream).
    async fn send(&self, body: &Value) -> Result<reqwest::Response> {
        self.client
            .post(&self.endpoint)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.version)
            .json(body)
            .send()
            .await
            .map_err(|e| Error::Provider(format!("request failed: {e}")))
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            supports_tools: true,
            context_window: self.context_window,
        }
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let body = self.build_body(&req, false);
        let resp = self.send(&body).await?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| Error::Provider(format!("reading body: {e}")))?;
        if !status.is_success() {
            return Err(Error::Provider(format!("http {status}: {text}")));
        }

        let parsed: WireResp = serde_json::from_str(&text)
            .map_err(|e| Error::Provider(format!("decoding response: {e}; body={text}")))?;
        Ok(parsed.into_response())
    }

    async fn stream(&self, req: CompletionRequest) -> Result<ChunkStream> {
        let body = self.build_body(&req, true);
        let resp = self.send(&body).await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(Error::Provider(format!("http {status}: {text}")));
        }

        // Accumulate typed content blocks (text streams directly; tool_use `input`
        // arrives as `input_json_delta` fragments finalized at content_block_stop).
        let stream = async_stream::stream! {
            let mut bytes = resp.bytes_stream();
            let mut buf = String::new();
            let mut blocks: BTreeMap<usize, ToolBlockAcc> = BTreeMap::new();
            let mut input_tokens = 0u32;
            let mut output_tokens = 0u32;
            let mut stop_reason: Option<String> = None;

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
                    let ev: SseEvent = match serde_json::from_str(data) {
                        Ok(ev) => ev,
                        Err(_) => continue,
                    };
                    match ev.typ.as_str() {
                        "message_start" => {
                            if let Some(u) = ev.message.and_then(|m| m.usage) {
                                input_tokens = u.input_tokens;
                            }
                        }
                        "content_block_start" => {
                            if let (Some(i), Some(cb)) = (ev.index, ev.content_block) {
                                if cb.typ.as_deref() == Some("tool_use") {
                                    blocks.insert(
                                        i,
                                        ToolBlockAcc {
                                            id: cb.id.unwrap_or_default(),
                                            name: cb.name.unwrap_or_default(),
                                            json: String::new(),
                                        },
                                    );
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(delta) = ev.delta {
                                if let Some(text) = delta.text {
                                    if !text.is_empty() {
                                        yield Ok(CompletionChunk { delta_text: text, ..Default::default() });
                                    }
                                }
                                if let (Some(i), Some(pj)) = (ev.index, delta.partial_json) {
                                    if let Some(acc) = blocks.get_mut(&i) {
                                        acc.json.push_str(&pj);
                                    }
                                }
                            }
                        }
                        "content_block_stop" => {
                            if let Some(i) = ev.index {
                                if let Some(acc) = blocks.remove(&i) {
                                    yield Ok(CompletionChunk {
                                        tool_call: Some(acc.into_tool_call()),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                        "message_delta" => {
                            if let Some(delta) = &ev.delta {
                                if let Some(sr) = &delta.stop_reason {
                                    stop_reason = Some(sr.clone());
                                }
                            }
                            if let Some(u) = ev.usage {
                                output_tokens = u.output_tokens;
                            }
                        }
                        "message_stop" => break 'read,
                        _ => {}
                    }
                }
            }

            yield Ok(CompletionChunk {
                finish_reason: Some(stop_reason.unwrap_or_else(|| "end_turn".into())),
                usage: Some(Usage {
                    prompt_tokens: input_tokens,
                    completion_tokens: output_tokens,
                    total_tokens: input_tokens + output_tokens,
                }),
                ..Default::default()
            });
        };
        Ok(Box::pin(stream))
    }
}

/// A tool_use content block being assembled from streamed fragments.
struct ToolBlockAcc {
    id: String,
    name: String,
    json: String,
}

impl ToolBlockAcc {
    fn into_tool_call(self) -> ToolCall {
        let arguments = if self.json.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&self.json).unwrap_or(Value::Object(Default::default()))
        };
        ToolCall {
            id: self.id,
            name: self.name,
            arguments,
        }
    }
}

// --- wire types (streaming SSE events) ------------------------------------

#[derive(Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    typ: String,
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    content_block: Option<SseContentBlock>,
    #[serde(default)]
    delta: Option<SseDelta>,
    #[serde(default)]
    message: Option<SseMessage>,
    #[serde(default)]
    usage: Option<SseUsage>,
}

#[derive(Deserialize)]
struct SseContentBlock {
    #[serde(rename = "type", default)]
    typ: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct SseDelta {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct SseMessage {
    #[serde(default)]
    usage: Option<SseUsage>,
}

#[derive(Deserialize, Default)]
struct SseUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

/// Convert our message list into `(system, messages)` for the Messages API.
/// System-role messages are concatenated into the top-level system prompt; the
/// rest become user/assistant turns with typed content blocks, coalescing
/// consecutive same-role turns into one message (required by the API).
fn to_anthropic_messages(messages: &[Message]) -> (String, Vec<Value>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut out: Vec<Value> = Vec::new();
    // The role ("user"/"assistant") and accumulated content blocks of the
    // message currently being built.
    let mut cur_role: Option<&'static str> = None;
    let mut cur_blocks: Vec<Value> = Vec::new();

    let flush = |role: &mut Option<&'static str>, blocks: &mut Vec<Value>, out: &mut Vec<Value>| {
        if let Some(r) = role.take() {
            if !blocks.is_empty() {
                out.push(json!({ "role": r, "content": std::mem::take(blocks) }));
            }
        }
    };

    for m in messages {
        match m.role {
            Role::System => {
                if !m.content.is_empty() {
                    system_parts.push(m.content.clone());
                }
            }
            Role::User => {
                push_role(&mut cur_role, "user", &mut cur_blocks, &mut out, &flush);
                if !m.content.is_empty() {
                    cur_blocks.push(json!({ "type": "text", "text": m.content }));
                }
            }
            Role::Assistant => {
                push_role(
                    &mut cur_role,
                    "assistant",
                    &mut cur_blocks,
                    &mut out,
                    &flush,
                );
                if !m.content.is_empty() {
                    cur_blocks.push(json!({ "type": "text", "text": m.content }));
                }
                for tc in &m.tool_calls {
                    cur_blocks.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
            }
            Role::Tool => {
                // Tool results are `user` messages carrying tool_result blocks.
                push_role(&mut cur_role, "user", &mut cur_blocks, &mut out, &flush);
                cur_blocks.push(json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content,
                }));
            }
        }
    }
    // Flush the final in-progress message.
    if let Some(r) = cur_role.take() {
        if !cur_blocks.is_empty() {
            out.push(json!({ "role": r, "content": cur_blocks }));
        }
    }

    (system_parts.join("\n\n"), out)
}

/// Switch the current message to `role`, flushing the previous one first when the
/// role changes (so same-role turns coalesce into a single message).
fn push_role(
    cur_role: &mut Option<&'static str>,
    role: &'static str,
    cur_blocks: &mut Vec<Value>,
    out: &mut Vec<Value>,
    flush: &impl Fn(&mut Option<&'static str>, &mut Vec<Value>, &mut Vec<Value>),
) {
    if *cur_role != Some(role) {
        flush(cur_role, cur_blocks, out);
        *cur_role = Some(role);
    }
}

// --- wire types (response) ------------------------------------------------

#[derive(Deserialize)]
struct WireResp {
    #[serde(default)]
    content: Vec<WireBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireBlock {
    Text {
        #[serde(default)]
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    /// Anything else (e.g. thinking blocks) is ignored.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

impl WireResp {
    fn into_response(self) -> CompletionResponse {
        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        for block in self.content {
            match block {
                WireBlock::Text { text } => content.push_str(&text),
                WireBlock::ToolUse { id, name, input } => tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments: input,
                }),
                WireBlock::Other => {}
            }
        }
        let message = Message {
            role: Role::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
        };
        CompletionResponse {
            message,
            finish_reason: self.stop_reason.unwrap_or_else(|| "end_turn".into()),
            usage: self.usage.map(|u| Usage {
                prompt_tokens: u.input_tokens,
                completion_tokens: u.output_tokens,
                total_tokens: u.input_tokens + u.output_tokens,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolCall;
    use rstest::rstest;
    use serde_json::json;

    fn msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    // --- to_anthropic_messages: system split + role coalescing -------------
    // `(system string, number of wire messages)`.
    #[rstest]
    #[case::positive_system_and_user(vec![(Role::System, "S"), (Role::User, "hi")], "S", 1)]
    #[case::positive_user_only(vec![(Role::User, "hi")], "", 1)]
    #[case::boundary_system_only(vec![(Role::System, "S")], "S", 0)]
    #[case::corner_multi_system_joined(vec![(Role::System, "A"), (Role::System, "B"), (Role::User, "x")], "A\n\nB", 1)]
    #[case::boundary_empty(vec![], "", 0)]
    #[case::corner_consecutive_users_coalesce(vec![(Role::User, "a"), (Role::User, "b")], "", 1)]
    #[case::corner_empty_user_content_no_block(vec![(Role::User, "")], "", 0)]
    fn to_anthropic_messages_shape(
        #[case] msgs: Vec<(Role, &str)>,
        #[case] system: &str,
        #[case] out_len: usize,
    ) {
        let built: Vec<Message> = msgs.into_iter().map(|(r, c)| msg(r, c)).collect();
        let (sys, out) = to_anthropic_messages(&built);
        assert_eq!(sys, system);
        assert_eq!(out.len(), out_len);
    }

    #[test]
    fn to_anthropic_messages_first_is_user_text_block() {
        let (_s, out) = to_anthropic_messages(&[msg(Role::System, "bot"), msg(Role::User, "hi")]);
        assert_eq!(out[0]["role"], "user");
        assert_eq!(out[0]["content"][0]["type"], "text");
        assert_eq!(out[0]["content"][0]["text"], "hi");
    }

    // --- WireResp::into_response: content blocks → message -----------------
    // `(body) → (text, #tool_calls, finish_reason)`.
    #[rstest]
    #[case::positive_text_only(json!({"content":[{"type":"text","text":"hi"}]}), "hi", 0, "end_turn")]
    #[case::positive_tool_only(json!({"content":[{"type":"tool_use","id":"t","name":"ls","input":{}}],"stop_reason":"tool_use"}), "", 1, "tool_use")]
    #[case::positive_text_and_tool(json!({"content":[{"type":"text","text":"a"},{"type":"tool_use","id":"t","name":"b","input":{}}],"stop_reason":"tool_use"}), "a", 1, "tool_use")]
    #[case::boundary_empty_content(json!({}), "", 0, "end_turn")]
    #[case::corner_ignores_unknown_block(json!({"content":[{"type":"thinking","text":"…"},{"type":"text","text":"x"}]}), "x", 0, "end_turn")]
    fn into_response_cases(
        #[case] body: Value,
        #[case] text: &str,
        #[case] n_tools: usize,
        #[case] finish: &str,
    ) {
        let parsed: WireResp = serde_json::from_value(body).unwrap();
        let resp = parsed.into_response();
        assert_eq!(resp.message.content, text);
        assert_eq!(resp.message.tool_calls.len(), n_tools);
        assert_eq!(resp.finish_reason, finish);
    }

    #[test]
    fn into_response_sums_usage_tokens() {
        let body = json!({"content":[], "usage":{"input_tokens":10,"output_tokens":5}});
        let resp = serde_json::from_value::<WireResp>(body)
            .unwrap()
            .into_response();
        assert_eq!(resp.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn tool_calls_and_results_map_to_blocks_and_coalesce() {
        let assistant = Message {
            role: Role::Assistant,
            content: "let me check".into(),
            tool_calls: vec![
                ToolCall {
                    id: "a".into(),
                    name: "bash".into(),
                    arguments: json!({"command": "ls"}),
                },
                ToolCall {
                    id: "b".into(),
                    name: "read_file".into(),
                    arguments: json!({"path": "x"}),
                },
            ],
            tool_call_id: None,
        };
        let msgs = vec![
            msg(Role::User, "go"),
            assistant,
            Message::tool("a", "file1\nfile2"),
            Message::tool("b", "contents"),
        ];
        let (_system, out) = to_anthropic_messages(&msgs);
        // user(go) | assistant(text+2 tool_use) | user(2 tool_result, coalesced)
        assert_eq!(out.len(), 3);
        assert_eq!(out[1]["role"], "assistant");
        assert_eq!(out[1]["content"][0]["type"], "text");
        assert_eq!(out[1]["content"][1]["type"], "tool_use");
        assert_eq!(out[1]["content"][2]["type"], "tool_use");
        assert_eq!(out[2]["role"], "user");
        assert_eq!(out[2]["content"].as_array().unwrap().len(), 2);
        assert_eq!(out[2]["content"][0]["type"], "tool_result");
        assert_eq!(out[2]["content"][0]["tool_use_id"], "a");
    }
}
