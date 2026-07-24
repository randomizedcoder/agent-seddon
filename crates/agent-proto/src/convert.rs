//! Lossless conversions between the canonical [`agent_core`] types and their
//! generated protobuf twins in [`crate::pb`].
//!
//! Direction matters: `agent-core` is the source of truth and never depends on
//! proto, so all the bridging lives here. Outbound (`core → proto`) is infallible
//! ([`From`]); inbound (`proto → core`) is fallible ([`TryFrom`]) because the wire
//! can carry an unset enum, an absent required message, or malformed JSON — see
//! [`ConvertError`].
//!
//! Dynamic `serde_json::Value` fields (tool arguments, JSON Schemas) map to the
//! fully-binary [`crate::pb::JsonValue`] via [`value_to_pb`] / [`pb_to_value`] —
//! lossless (64-bit integers keep their type), never JSON text on the wire. An
//! unset value decodes to [`serde_json::Value::Null`].

use crate::pb;

/// Why a `proto → core` conversion failed.
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("invalid JSON in `{field}`: {source}")]
    Json {
        field: &'static str,
        source: serde_json::Error,
    },
    /// The wire `Role` was `ROLE_UNSPECIFIED` (or an unknown tag).
    #[error("unknown/unspecified role tag: {0}")]
    UnknownRole(i32),
    /// A required singular message field was absent (proto3 makes them optional).
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
}

impl From<ConvertError> for tonic::Status {
    fn from(e: ConvertError) -> Self {
        tonic::Status::invalid_argument(e.to_string())
    }
}

// --- JSON value <-> binary JsonValue ---------------------------------------
//
// A fully-binary, lossless encoding of `serde_json::Value`: integers keep their
// exact type (i64/u64), non-integers use f64, and anything beyond native range
// (only reachable with serde_json's `arbitrary_precision`) rides in `big_number`
// as a decimal string. No JSON text on the wire.

fn value_to_pb(v: &serde_json::Value) -> pb::JsonValue {
    use pb::json_value::Kind;
    use serde_json::Value;
    let kind = match v {
        Value::Null => Kind::NullValue(pb::NullValue::NullValue as i32),
        Value::Bool(b) => Kind::BoolValue(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Kind::IntValue(i)
            } else if let Some(u) = n.as_u64() {
                Kind::UintValue(u)
            } else if let Some(f) = n.as_f64() {
                Kind::DoubleValue(f)
            } else {
                // Only reachable under `arbitrary_precision`: keep the exact literal.
                Kind::BigNumber(n.to_string())
            }
        }
        Value::String(s) => Kind::StringValue(s.clone()),
        Value::Array(a) => Kind::ArrayValue(pb::JsonArray {
            values: a.iter().map(value_to_pb).collect(),
        }),
        Value::Object(o) => Kind::ObjectValue(pb::JsonObject {
            fields: o.iter().map(|(k, v)| (k.clone(), value_to_pb(v))).collect(),
        }),
    };
    pb::JsonValue { kind: Some(kind) }
}

/// Public `serde_json::Value` → [`pb::JsonValue`]. Used where a bare JSON value
/// crosses the wire (e.g. `ExecuteRequest.arguments`), not just nested in a
/// `ToolCall`/`ToolSchema`.
impl From<serde_json::Value> for pb::JsonValue {
    fn from(v: serde_json::Value) -> Self {
        value_to_pb(&v)
    }
}

/// Public [`pb::JsonValue`] → `serde_json::Value` (fallible only via `big_number`).
impl TryFrom<pb::JsonValue> for serde_json::Value {
    type Error = ConvertError;
    fn try_from(v: pb::JsonValue) -> Result<Self, Self::Error> {
        pb_to_value(v, "JsonValue")
    }
}

fn pb_to_value(v: pb::JsonValue, field: &'static str) -> Result<serde_json::Value, ConvertError> {
    use pb::json_value::Kind;
    use serde_json::Value;
    Ok(match v.kind {
        // An unset oneof (e.g. a default-constructed message) is JSON null.
        None | Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::BoolValue(b)) => Value::Bool(b),
        Some(Kind::IntValue(i)) => Value::Number(i.into()),
        Some(Kind::UintValue(u)) => Value::Number(u.into()),
        Some(Kind::DoubleValue(f)) => serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or(Value::Null), // NaN/Inf aren't representable in JSON
        Some(Kind::StringValue(s)) => Value::String(s),
        Some(Kind::ArrayValue(a)) => Value::Array(
            a.values
                .into_iter()
                .map(|v| pb_to_value(v, field))
                .collect::<Result<_, _>>()?,
        ),
        Some(Kind::ObjectValue(o)) => {
            let mut m = serde_json::Map::with_capacity(o.fields.len());
            for (k, val) in o.fields {
                m.insert(k, pb_to_value(val, field)?);
            }
            Value::Object(m)
        }
        Some(Kind::BigNumber(s)) => {
            serde_json::from_str(&s).map_err(|source| ConvertError::Json { field, source })?
        }
    })
}

fn role_from_i32(v: i32) -> Result<agent_core::Role, ConvertError> {
    match pb::Role::try_from(v) {
        Ok(pb::Role::System) => Ok(agent_core::Role::System),
        Ok(pb::Role::User) => Ok(agent_core::Role::User),
        Ok(pb::Role::Assistant) => Ok(agent_core::Role::Assistant),
        Ok(pb::Role::Tool) => Ok(agent_core::Role::Tool),
        Ok(pb::Role::Unspecified) | Err(_) => Err(ConvertError::UnknownRole(v)),
    }
}

/// Map an `agent_core::Error` onto a gRPC status. Used by the (future) seam
/// servers to translate a failed local call into a wire error; kept here so the
/// mapping lives next to the other core↔wire bridges.
pub fn status_from_error(e: &agent_core::Error) -> tonic::Status {
    use agent_core::Error;
    match e {
        Error::Provider(m) => tonic::Status::internal(format!("provider: {m}")),
        Error::Tool(m) => tonic::Status::internal(format!("tool: {m}")),
        Error::Memory(m) => tonic::Status::internal(format!("memory: {m}")),
        Error::Config(m) => tonic::Status::invalid_argument(format!("config: {m}")),
        Error::Io(m) => tonic::Status::unavailable(format!("io: {m}")),
        Error::Json(m) => tonic::Status::invalid_argument(format!("json: {m}")),
        Error::Search(m) => tonic::Status::internal(format!("search: {m}")),
        Error::Repo(m) => tonic::Status::internal(format!("repo: {m}")),
        Error::Tokenizer(m) => tonic::Status::internal(format!("tokenizer: {m}")),
        Error::Web(m) => tonic::Status::internal(format!("web: {m}")),
        Error::Tasks(m) => tonic::Status::internal(format!("tasks: {m}")),
        Error::Scheduler(m) => tonic::Status::internal(format!("scheduler: {m}")),
        Error::Pty(m) => tonic::Status::internal(format!("pty: {m}")),
        Error::Structured(m) => tonic::Status::invalid_argument(format!("structured: {m}")),
        Error::Lsp(m) => tonic::Status::internal(format!("lsp: {m}")),
        Error::Sandbox(m) => tonic::Status::internal(format!("sandbox: {m}")),
        Error::Embed(m) => tonic::Status::internal(format!("embed: {m}")),
        Error::Session(m) => tonic::Status::internal(format!("session: {m}")),
    }
}

// --- Role ------------------------------------------------------------------

impl From<agent_core::Role> for pb::Role {
    fn from(r: agent_core::Role) -> Self {
        match r {
            agent_core::Role::System => pb::Role::System,
            agent_core::Role::User => pb::Role::User,
            agent_core::Role::Assistant => pb::Role::Assistant,
            agent_core::Role::Tool => pb::Role::Tool,
        }
    }
}

// --- ToolCall --------------------------------------------------------------

impl From<agent_core::ToolCall> for pb::ToolCall {
    fn from(c: agent_core::ToolCall) -> Self {
        pb::ToolCall {
            id: c.id,
            name: c.name,
            arguments: Some(value_to_pb(&c.arguments)),
        }
    }
}

impl TryFrom<pb::ToolCall> for agent_core::ToolCall {
    type Error = ConvertError;
    fn try_from(c: pb::ToolCall) -> Result<Self, Self::Error> {
        Ok(agent_core::ToolCall {
            id: c.id,
            name: c.name,
            arguments: pb_to_value(c.arguments.unwrap_or_default(), "ToolCall.arguments")?,
        })
    }
}

// --- ContentBlock ----------------------------------------------------------

impl From<agent_core::ContentBlock> for pb::ContentBlock {
    fn from(b: agent_core::ContentBlock) -> Self {
        use pb::content_block::{Document, Image, Kind, Text};
        let kind = match b {
            agent_core::ContentBlock::Text { text } => Kind::Text(Text { text }),
            agent_core::ContentBlock::Image { media_type, data } => {
                Kind::Image(Image { media_type, data })
            }
            agent_core::ContentBlock::Document {
                media_type,
                data,
                name,
            } => Kind::Document(Document {
                media_type,
                data,
                name,
            }),
        };
        pb::ContentBlock { kind: Some(kind) }
    }
}

impl TryFrom<pb::ContentBlock> for agent_core::ContentBlock {
    type Error = ConvertError;
    fn try_from(b: pb::ContentBlock) -> Result<Self, Self::Error> {
        use pb::content_block::Kind;
        // An unset `kind` is a malformed block from an untrusted peer — fail
        // closed rather than inventing empty content.
        let kind = b
            .kind
            .ok_or(ConvertError::MissingField("ContentBlock.kind"))?;
        Ok(match kind {
            Kind::Text(t) => agent_core::ContentBlock::Text { text: t.text },
            Kind::Image(i) => agent_core::ContentBlock::Image {
                media_type: i.media_type,
                data: i.data,
            },
            Kind::Document(d) => agent_core::ContentBlock::Document {
                media_type: d.media_type,
                data: d.data,
                name: d.name,
            },
        })
    }
}

// --- Message ---------------------------------------------------------------

/// `true` when the block list is exactly zero-or-one **text** block — the shape
/// that field 2 (`string content`) already represents losslessly, so `blocks`
/// can stay unset and the message costs nothing extra on the wire.
fn is_plain_text(blocks: &[agent_core::ContentBlock]) -> bool {
    match blocks {
        [] => true,
        [b] => !b.is_media(),
        _ => false,
    }
}

impl From<agent_core::Message> for pb::Message {
    fn from(m: agent_core::Message) -> Self {
        // `content` is ALWAYS the text, so a peer built before multimodal landed
        // still reads the prose. `blocks` is set only when it carries information
        // `content` cannot (media, or several blocks).
        let content = m.content_text();
        let blocks = if is_plain_text(&m.content) {
            Vec::new()
        } else {
            m.content.into_iter().map(Into::into).collect()
        };
        pb::Message {
            role: pb::Role::from(m.role) as i32,
            content,
            tool_calls: m.tool_calls.into_iter().map(Into::into).collect(),
            tool_call_id: m.tool_call_id,
            blocks,
        }
    }
}

impl TryFrom<pb::Message> for agent_core::Message {
    type Error = ConvertError;
    fn try_from(m: pb::Message) -> Result<Self, Self::Error> {
        // Prefer the full block list; fall back to folding `content` into one
        // text block (a legacy peer, or a plain-text message).
        let content = if m.blocks.is_empty() {
            if m.content.is_empty() {
                Vec::new()
            } else {
                vec![agent_core::ContentBlock::text(m.content)]
            }
        } else {
            m.blocks
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(agent_core::Message {
            role: role_from_i32(m.role)?,
            content,
            tool_calls: m
                .tool_calls
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            tool_call_id: m.tool_call_id,
        })
    }
}

// --- ToolSchema ------------------------------------------------------------

impl From<agent_core::ToolSchema> for pb::ToolSchema {
    fn from(s: agent_core::ToolSchema) -> Self {
        pb::ToolSchema {
            name: s.name,
            description: s.description,
            parameters: Some(value_to_pb(&s.parameters)),
            // `ToolSchema` (agent-core) carries no concurrency flag — it lives on the
            // `Tool`. Default to the trait default (`true`); the tools server sets the
            // real per-tool value in `describe_all` (see agent-grpc `server.rs`).
            parallel_safe: true,
        }
    }
}

impl TryFrom<pb::ToolSchema> for agent_core::ToolSchema {
    type Error = ConvertError;
    fn try_from(s: pb::ToolSchema) -> Result<Self, Self::Error> {
        Ok(agent_core::ToolSchema {
            name: s.name,
            description: s.description,
            parameters: pb_to_value(s.parameters.unwrap_or_default(), "ToolSchema.parameters")?,
        })
    }
}

// --- Observation -----------------------------------------------------------

impl From<agent_core::Observation> for pb::Observation {
    fn from(o: agent_core::Observation) -> Self {
        pb::Observation {
            content: o.content,
            is_error: o.is_error,
            blocks: o.blocks.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<pb::Observation> for agent_core::Observation {
    type Error = ConvertError;
    fn try_from(o: pb::Observation) -> Result<Self, Self::Error> {
        Ok(agent_core::Observation {
            content: o.content,
            blocks: o
                .blocks
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            is_error: o.is_error,
        })
    }
}

// --- ToolContext -----------------------------------------------------------

impl From<&agent_core::ToolContext> for pb::ToolContext {
    fn from(c: &agent_core::ToolContext) -> Self {
        pb::ToolContext {
            cwd: c.cwd.to_string_lossy().into_owned(),
        }
    }
}

impl From<pb::ToolContext> for agent_core::ToolContext {
    fn from(c: pb::ToolContext) -> Self {
        agent_core::ToolContext {
            cwd: std::path::PathBuf::from(c.cwd),
        }
    }
}

// --- ModelCapabilities -----------------------------------------------------

impl From<agent_core::ModelCapabilities> for pb::ModelCapabilities {
    fn from(c: agent_core::ModelCapabilities) -> Self {
        pb::ModelCapabilities {
            supports_tools: c.supports_tools,
            context_window: c.context_window,
            supports_vision: c.supports_vision,
        }
    }
}

impl From<pb::ModelCapabilities> for agent_core::ModelCapabilities {
    fn from(c: pb::ModelCapabilities) -> Self {
        agent_core::ModelCapabilities {
            supports_tools: c.supports_tools,
            context_window: c.context_window,
            // Not on the wire yet (native response_format is a follow-up, spec 16).
            supports_response_format: false,
            supports_vision: c.supports_vision,
        }
    }
}

// --- Usage -----------------------------------------------------------------

impl From<agent_core::Usage> for pb::Usage {
    fn from(u: agent_core::Usage) -> Self {
        pb::Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }
    }
}

impl From<pb::Usage> for agent_core::Usage {
    fn from(u: pb::Usage) -> Self {
        // NOTE: the `pb::Usage` wire message does not yet carry the cache-token /
        // cost fields (added to `agent_core::Usage` in parity spec 23); they
        // default here and will traverse gRPC once `common.proto` is extended in
        // the tokenizer-gRPC follow-up.
        agent_core::Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
            ..Default::default()
        }
    }
}

// --- CompletionRequest -----------------------------------------------------

impl From<agent_core::CompletionRequest> for pb::CompletionRequest {
    fn from(r: agent_core::CompletionRequest) -> Self {
        pb::CompletionRequest {
            messages: r.messages.into_iter().map(Into::into).collect(),
            tools: r.tools.into_iter().map(Into::into).collect(),
            max_tokens: r.max_tokens,
            temperature: r.temperature,
        }
    }
}

impl TryFrom<pb::CompletionRequest> for agent_core::CompletionRequest {
    type Error = ConvertError;
    fn try_from(r: pb::CompletionRequest) -> Result<Self, Self::Error> {
        Ok(agent_core::CompletionRequest {
            messages: r
                .messages
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            tools: r
                .tools
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
            max_tokens: r.max_tokens,
            temperature: r.temperature,
            // `response_format` is not yet on the wire — the Validator gRPC service
            // + proto field are a documented follow-up (parity spec 16).
            response_format: None,
        })
    }
}

// --- CompletionResponse ----------------------------------------------------

impl From<agent_core::CompletionResponse> for pb::CompletionResponse {
    fn from(r: agent_core::CompletionResponse) -> Self {
        pb::CompletionResponse {
            message: Some(r.message.into()),
            finish_reason: r.finish_reason,
            usage: r.usage.map(Into::into),
        }
    }
}

impl TryFrom<pb::CompletionResponse> for agent_core::CompletionResponse {
    type Error = ConvertError;
    fn try_from(r: pb::CompletionResponse) -> Result<Self, Self::Error> {
        Ok(agent_core::CompletionResponse {
            message: r
                .message
                .ok_or(ConvertError::MissingField("CompletionResponse.message"))?
                .try_into()?,
            finish_reason: r.finish_reason,
            usage: r.usage.map(Into::into),
        })
    }
}

// --- CompletionChunk -------------------------------------------------------

impl From<agent_core::CompletionChunk> for pb::CompletionChunk {
    fn from(c: agent_core::CompletionChunk) -> Self {
        pb::CompletionChunk {
            delta_text: c.delta_text,
            tool_call: c.tool_call.map(Into::into),
            finish_reason: c.finish_reason,
            usage: c.usage.map(Into::into),
        }
    }
}

impl TryFrom<pb::CompletionChunk> for agent_core::CompletionChunk {
    type Error = ConvertError;
    fn try_from(c: pb::CompletionChunk) -> Result<Self, Self::Error> {
        Ok(agent_core::CompletionChunk {
            delta_text: c.delta_text,
            tool_call: c.tool_call.map(TryInto::try_into).transpose()?,
            finish_reason: c.finish_reason,
            usage: c.usage.map(Into::into),
        })
    }
}

// --- MemoryItem ------------------------------------------------------------

impl From<agent_core::MemoryItem> for pb::MemoryItem {
    fn from(i: agent_core::MemoryItem) -> Self {
        pb::MemoryItem {
            source: i.source,
            content: i.content,
        }
    }
}

impl From<pb::MemoryItem> for agent_core::MemoryItem {
    fn from(i: pb::MemoryItem) -> Self {
        agent_core::MemoryItem {
            source: i.source,
            content: i.content,
        }
    }
}

// --- RecallQuery -----------------------------------------------------------

impl From<agent_core::RecallQuery> for pb::RecallQuery {
    fn from(q: agent_core::RecallQuery) -> Self {
        pb::RecallQuery {
            text: q.text,
            limit: q.limit as u64,
        }
    }
}

impl From<pb::RecallQuery> for agent_core::RecallQuery {
    fn from(q: pb::RecallQuery) -> Self {
        agent_core::RecallQuery {
            text: q.text,
            limit: q.limit as usize,
        }
    }
}

// --- MemoryEvent -----------------------------------------------------------

impl From<agent_core::MemoryEvent> for pb::MemoryEvent {
    fn from(e: agent_core::MemoryEvent) -> Self {
        pb::MemoryEvent {
            kind: e.kind,
            message: Some(e.message.into()),
            ts_ms: e.ts_ms,
            session_id: e.session_id,
            usage: e.usage.map(Into::into),
            iter: e.iter,
        }
    }
}

impl TryFrom<pb::MemoryEvent> for agent_core::MemoryEvent {
    type Error = ConvertError;
    fn try_from(e: pb::MemoryEvent) -> Result<Self, Self::Error> {
        Ok(agent_core::MemoryEvent {
            kind: e.kind,
            message: e
                .message
                .ok_or(ConvertError::MissingField("MemoryEvent.message"))?
                .try_into()?,
            ts_ms: e.ts_ms,
            session_id: e.session_id,
            usage: e.usage.map(Into::into),
            iter: e.iter,
            // Telemetry-local (recorded via the `CompositeMemory` mirror, not the
            // wire); the gRPC `MemoryEvent` has no such field, so it is `None` here.
            verification: None,
            review: None,
        })
    }
}

// --- ContextBlock ----------------------------------------------------------

impl From<agent_core::ContextBlock> for pb::ContextBlock {
    fn from(b: agent_core::ContextBlock) -> Self {
        pb::ContextBlock {
            source: b.source,
            content: b.content,
        }
    }
}

impl From<pb::ContextBlock> for agent_core::ContextBlock {
    fn from(b: pb::ContextBlock) -> Self {
        agent_core::ContextBlock {
            source: b.source,
            content: b.content,
        }
    }
}

// --- ContextInput ----------------------------------------------------------

impl From<agent_core::ContextInput> for pb::ContextInput {
    fn from(i: agent_core::ContextInput) -> Self {
        pb::ContextInput {
            system_prompt: i.system_prompt,
            prepend: i.prepend.into_iter().map(Into::into).collect(),
            recalled: i.recalled.into_iter().map(Into::into).collect(),
            goal: i.goal,
            append: i.append.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<pb::ContextInput> for agent_core::ContextInput {
    fn from(i: pb::ContextInput) -> Self {
        agent_core::ContextInput {
            system_prompt: i.system_prompt,
            prepend: i.prepend.into_iter().map(Into::into).collect(),
            recalled: i.recalled.into_iter().map(Into::into).collect(),
            goal: i.goal,
            append: i.append.into_iter().map(Into::into).collect(),
        }
    }
}

// --- WorkingSet ------------------------------------------------------------

impl From<agent_core::WorkingSet> for pb::WorkingSet {
    fn from(w: agent_core::WorkingSet) -> Self {
        pb::WorkingSet {
            messages: w.messages.into_iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<pb::WorkingSet> for agent_core::WorkingSet {
    type Error = ConvertError;
    fn try_from(w: pb::WorkingSet) -> Result<Self, Self::Error> {
        Ok(agent_core::WorkingSet {
            messages: w
                .messages
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

// --- TokenBudget -----------------------------------------------------------

impl From<agent_core::TokenBudget> for pb::TokenBudget {
    fn from(b: agent_core::TokenBudget) -> Self {
        pb::TokenBudget {
            max_context_tokens: b.max_context_tokens,
            reserve_output: b.reserve_output,
        }
    }
}

impl From<pb::TokenBudget> for agent_core::TokenBudget {
    fn from(b: pb::TokenBudget) -> Self {
        agent_core::TokenBudget {
            max_context_tokens: b.max_context_tokens,
            reserve_output: b.reserve_output,
        }
    }
}

// --- Decision --------------------------------------------------------------

impl From<agent_core::Decision> for pb::Decision {
    fn from(d: agent_core::Decision) -> Self {
        match d {
            agent_core::Decision::Allow => pb::Decision {
                allowed: true,
                deny_reason: None,
            },
            agent_core::Decision::Deny(reason) => pb::Decision {
                allowed: false,
                deny_reason: Some(reason),
            },
        }
    }
}

impl From<pb::Decision> for agent_core::Decision {
    fn from(d: pb::Decision) -> Self {
        if d.allowed {
            agent_core::Decision::Allow
        } else {
            agent_core::Decision::Deny(d.deny_reason.unwrap_or_default())
        }
    }
}

// --- Search: SearchMode / IndexState ---------------------------------------

impl From<agent_core::SearchMode> for pb::SearchMode {
    fn from(m: agent_core::SearchMode) -> Self {
        match m {
            agent_core::SearchMode::Literal => pb::SearchMode::Literal,
            agent_core::SearchMode::Phrase => pb::SearchMode::Phrase,
            agent_core::SearchMode::Fuzzy => pb::SearchMode::Fuzzy,
            agent_core::SearchMode::Regex => pb::SearchMode::Regex,
            // `semantic`/`hybrid` are not on the wire yet (the vector backend over
            // gRPC is a follow-up); map to Literal so the enum stays exhaustive.
            agent_core::SearchMode::Semantic | agent_core::SearchMode::Hybrid => {
                pb::SearchMode::Literal
            }
        }
    }
}

/// Wire `SearchMode` tag → core. An unknown tag decodes to `Literal` (the zero
/// value and the mode every backend supports), never an error.
fn search_mode_from_i32(v: i32) -> agent_core::SearchMode {
    match pb::SearchMode::try_from(v) {
        Ok(pb::SearchMode::Phrase) => agent_core::SearchMode::Phrase,
        Ok(pb::SearchMode::Fuzzy) => agent_core::SearchMode::Fuzzy,
        Ok(pb::SearchMode::Regex) => agent_core::SearchMode::Regex,
        Ok(pb::SearchMode::Literal) | Err(_) => agent_core::SearchMode::Literal,
    }
}

impl From<agent_core::IndexState> for pb::IndexState {
    fn from(s: agent_core::IndexState) -> Self {
        match s {
            agent_core::IndexState::Fresh => pb::IndexState::Fresh,
            agent_core::IndexState::Stale => pb::IndexState::Stale,
            agent_core::IndexState::Missing => pb::IndexState::Missing,
            agent_core::IndexState::Building => pb::IndexState::Building,
        }
    }
}

fn index_state_from_i32(v: i32) -> agent_core::IndexState {
    match pb::IndexState::try_from(v) {
        Ok(pb::IndexState::Stale) => agent_core::IndexState::Stale,
        Ok(pb::IndexState::Missing) => agent_core::IndexState::Missing,
        Ok(pb::IndexState::Building) => agent_core::IndexState::Building,
        Ok(pb::IndexState::Fresh) | Err(_) => agent_core::IndexState::Fresh,
    }
}

// --- SearchQuery -----------------------------------------------------------

impl From<agent_core::SearchQuery> for pb::SearchQuery {
    fn from(q: agent_core::SearchQuery) -> Self {
        pb::SearchQuery {
            text: q.text,
            mode: pb::SearchMode::from(q.mode) as i32,
            path_globs: q.path_globs,
            lang: q.lang,
            limit: q.limit as u64,
            fuzzy_distance: q.fuzzy_distance.map(u32::from),
        }
    }
}

impl From<pb::SearchQuery> for agent_core::SearchQuery {
    fn from(q: pb::SearchQuery) -> Self {
        agent_core::SearchQuery {
            text: q.text,
            mode: search_mode_from_i32(q.mode),
            path_globs: q.path_globs,
            lang: q.lang,
            limit: q.limit as usize,
            fuzzy_distance: q.fuzzy_distance.map(|d| d.min(u8::MAX as u32) as u8),
        }
    }
}

// --- SearchHit -------------------------------------------------------------

impl From<agent_core::SearchHit> for pb::SearchHit {
    fn from(h: agent_core::SearchHit) -> Self {
        pb::SearchHit {
            path: h.path.to_string_lossy().into_owned(),
            line: h.line,
            col_start: h.col_start,
            col_end: h.col_end,
            score: h.score,
            snippet: h.snippet,
        }
    }
}

impl From<pb::SearchHit> for agent_core::SearchHit {
    fn from(h: pb::SearchHit) -> Self {
        agent_core::SearchHit {
            path: std::path::PathBuf::from(h.path),
            line: h.line,
            col_start: h.col_start,
            col_end: h.col_end,
            score: h.score,
            snippet: h.snippet,
        }
    }
}

// --- SearchCapabilities ----------------------------------------------------

impl From<agent_core::SearchCapabilities> for pb::SearchCapabilities {
    fn from(c: agent_core::SearchCapabilities) -> Self {
        pb::SearchCapabilities {
            backend: c.backend,
            modes: c
                .modes
                .into_iter()
                .map(|m| pb::SearchMode::from(m) as i32)
                .collect(),
            content_search: c.content_search,
            scored: c.scored,
            incremental: c.incremental,
            max_concurrent_queries: c.max_concurrent_queries,
        }
    }
}

impl From<pb::SearchCapabilities> for agent_core::SearchCapabilities {
    fn from(c: pb::SearchCapabilities) -> Self {
        agent_core::SearchCapabilities {
            backend: c.backend,
            modes: c.modes.into_iter().map(search_mode_from_i32).collect(),
            content_search: c.content_search,
            scored: c.scored,
            incremental: c.incremental,
            max_concurrent_queries: c.max_concurrent_queries,
        }
    }
}

// --- IndexStatus (core lacks the wire `backend` field; server fills it) -----

impl From<agent_core::IndexStatus> for pb::IndexStatus {
    fn from(s: agent_core::IndexStatus) -> Self {
        pb::IndexStatus {
            state: pb::IndexState::from(s.state) as i32,
            indexed_files: s.indexed_files,
            last_indexed_ms: s.last_indexed_ms,
            manifest_digest: s.manifest_digest,
            backend: String::new(),
        }
    }
}

impl From<pb::IndexStatus> for agent_core::IndexStatus {
    fn from(s: pb::IndexStatus) -> Self {
        agent_core::IndexStatus {
            state: index_state_from_i32(s.state),
            indexed_files: s.indexed_files,
            last_indexed_ms: s.last_indexed_ms,
            manifest_digest: s.manifest_digest,
        }
    }
}

// --- ReindexProgress -------------------------------------------------------

impl From<agent_core::ReindexProgress> for pb::ReindexProgress {
    fn from(p: agent_core::ReindexProgress) -> Self {
        pb::ReindexProgress {
            files_done: p.files_done,
            files_total: p.files_total,
            done: p.done,
            backend: String::new(),
        }
    }
}

impl From<pb::ReindexProgress> for agent_core::ReindexProgress {
    fn from(p: pb::ReindexProgress) -> Self {
        agent_core::ReindexProgress {
            files_done: p.files_done,
            files_total: p.files_total,
            done: p.done,
        }
    }
}

// --- Repo: EntryKind / ChangeKind ------------------------------------------

impl From<agent_core::EntryKind> for pb::EntryKind {
    fn from(k: agent_core::EntryKind) -> Self {
        match k {
            agent_core::EntryKind::Blob => pb::EntryKind::Blob,
            agent_core::EntryKind::Tree => pb::EntryKind::Tree,
            agent_core::EntryKind::Symlink => pb::EntryKind::Symlink,
            agent_core::EntryKind::Submodule => pb::EntryKind::Submodule,
        }
    }
}

fn entry_kind_from_i32(v: i32) -> agent_core::EntryKind {
    match pb::EntryKind::try_from(v) {
        Ok(pb::EntryKind::Tree) => agent_core::EntryKind::Tree,
        Ok(pb::EntryKind::Symlink) => agent_core::EntryKind::Symlink,
        Ok(pb::EntryKind::Submodule) => agent_core::EntryKind::Submodule,
        Ok(pb::EntryKind::Blob) | Err(_) => agent_core::EntryKind::Blob,
    }
}

impl From<agent_core::ChangeKind> for pb::ChangeKind {
    fn from(k: agent_core::ChangeKind) -> Self {
        match k {
            agent_core::ChangeKind::Modified => pb::ChangeKind::Modified,
            agent_core::ChangeKind::Added => pb::ChangeKind::Added,
            agent_core::ChangeKind::Deleted => pb::ChangeKind::Deleted,
            agent_core::ChangeKind::Renamed => pb::ChangeKind::Renamed,
            agent_core::ChangeKind::Copied => pb::ChangeKind::Copied,
            agent_core::ChangeKind::TypeChange => pb::ChangeKind::TypeChange,
        }
    }
}

fn change_kind_from_i32(v: i32) -> agent_core::ChangeKind {
    match pb::ChangeKind::try_from(v) {
        Ok(pb::ChangeKind::Added) => agent_core::ChangeKind::Added,
        Ok(pb::ChangeKind::Deleted) => agent_core::ChangeKind::Deleted,
        Ok(pb::ChangeKind::Renamed) => agent_core::ChangeKind::Renamed,
        Ok(pb::ChangeKind::Copied) => agent_core::ChangeKind::Copied,
        Ok(pb::ChangeKind::TypeChange) => agent_core::ChangeKind::TypeChange,
        Ok(pb::ChangeKind::Modified) | Err(_) => agent_core::ChangeKind::Modified,
    }
}

// --- Repo: TreeEntry / BlobContent -----------------------------------------

impl From<agent_core::TreeEntry> for pb::TreeEntry {
    fn from(e: agent_core::TreeEntry) -> Self {
        pb::TreeEntry {
            path: e.path.to_string_lossy().into_owned(),
            oid: e.oid.0,
            kind: pb::EntryKind::from(e.kind) as i32,
            mode: e.mode,
            size: e.size,
        }
    }
}

impl From<pb::TreeEntry> for agent_core::TreeEntry {
    fn from(e: pb::TreeEntry) -> Self {
        agent_core::TreeEntry {
            path: std::path::PathBuf::from(e.path),
            oid: agent_core::Oid(e.oid),
            kind: entry_kind_from_i32(e.kind),
            mode: e.mode,
            size: e.size,
        }
    }
}

impl From<agent_core::BlobContent> for pb::BlobContent {
    fn from(b: agent_core::BlobContent) -> Self {
        pb::BlobContent {
            oid: b.oid.0,
            path: b.path.to_string_lossy().into_owned(),
            bytes_len: b.bytes_len,
            is_binary: b.is_binary,
            text: b.text,
        }
    }
}

impl From<pb::BlobContent> for agent_core::BlobContent {
    fn from(b: pb::BlobContent) -> Self {
        agent_core::BlobContent {
            oid: agent_core::Oid(b.oid),
            path: std::path::PathBuf::from(b.path),
            bytes_len: b.bytes_len,
            is_binary: b.is_binary,
            text: b.text,
        }
    }
}

// --- Repo: FileDiff / DiffResult -------------------------------------------

impl From<agent_core::FileDiff> for pb::FileDiff {
    fn from(f: agent_core::FileDiff) -> Self {
        pb::FileDiff {
            change: pb::ChangeKind::from(f.change) as i32,
            old_path: f.old_path.map(|p| p.to_string_lossy().into_owned()),
            new_path: f.new_path.map(|p| p.to_string_lossy().into_owned()),
            old_oid: f.old_oid.map(|o| o.0),
            new_oid: f.new_oid.map(|o| o.0),
            additions: f.additions,
            deletions: f.deletions,
            patch: f.patch,
        }
    }
}

impl From<pb::FileDiff> for agent_core::FileDiff {
    fn from(f: pb::FileDiff) -> Self {
        agent_core::FileDiff {
            change: change_kind_from_i32(f.change),
            old_path: f.old_path.map(std::path::PathBuf::from),
            new_path: f.new_path.map(std::path::PathBuf::from),
            old_oid: f.old_oid.map(agent_core::Oid),
            new_oid: f.new_oid.map(agent_core::Oid),
            additions: f.additions,
            deletions: f.deletions,
            patch: f.patch,
        }
    }
}

impl From<agent_core::DiffResult> for pb::DiffResult {
    fn from(d: agent_core::DiffResult) -> Self {
        pb::DiffResult {
            base: d.base.0,
            target: d.target.0,
            files: d.files.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<pb::DiffResult> for agent_core::DiffResult {
    fn from(d: pb::DiffResult) -> Self {
        agent_core::DiffResult {
            base: agent_core::Oid(d.base),
            target: agent_core::Oid(d.target),
            files: d.files.into_iter().map(Into::into).collect(),
        }
    }
}

// --- Repo: CommitInfo / GrepHit --------------------------------------------

impl From<agent_core::CommitInfo> for pb::CommitInfo {
    fn from(c: agent_core::CommitInfo) -> Self {
        pb::CommitInfo {
            oid: c.oid.0,
            parents: c.parents.into_iter().map(|o| o.0).collect(),
            author: c.author,
            author_email: c.author_email,
            committed_ms: c.committed_ms,
            summary: c.summary,
            body: c.body,
        }
    }
}

impl From<pb::CommitInfo> for agent_core::CommitInfo {
    fn from(c: pb::CommitInfo) -> Self {
        agent_core::CommitInfo {
            oid: agent_core::Oid(c.oid),
            parents: c.parents.into_iter().map(agent_core::Oid).collect(),
            author: c.author,
            author_email: c.author_email,
            committed_ms: c.committed_ms,
            summary: c.summary,
            body: c.body,
        }
    }
}

impl From<agent_core::GrepHit> for pb::GrepHit {
    fn from(h: agent_core::GrepHit) -> Self {
        pb::GrepHit {
            path: h.path.to_string_lossy().into_owned(),
            line: h.line,
            text: h.text,
        }
    }
}

impl From<pb::GrepHit> for agent_core::GrepHit {
    fn from(h: pb::GrepHit) -> Self {
        agent_core::GrepHit {
            path: std::path::PathBuf::from(h.path),
            line: h.line,
            text: h.text,
        }
    }
}

// --- Repo: WorktreeHandle / WorktreeSpec / Checkpoint ----------------------

impl From<agent_core::WorktreeHandle> for pb::WorktreeHandle {
    fn from(w: agent_core::WorktreeHandle) -> Self {
        pb::WorktreeHandle {
            id: w.id,
            path: w.path.to_string_lossy().into_owned(),
            head: w.head.0,
            revision: w.revision.0,
            writable: w.writable,
        }
    }
}

impl From<pb::WorktreeHandle> for agent_core::WorktreeHandle {
    fn from(w: pb::WorktreeHandle) -> Self {
        agent_core::WorktreeHandle {
            id: w.id,
            path: std::path::PathBuf::from(w.path),
            head: agent_core::Oid(w.head),
            revision: agent_core::Revision(w.revision),
            writable: w.writable,
        }
    }
}

impl From<agent_core::WorktreeSpec> for pb::WorktreeSpec {
    fn from(s: agent_core::WorktreeSpec) -> Self {
        pb::WorktreeSpec {
            revision: s.revision.0,
            writable: s.writable,
            id: s.id,
        }
    }
}

impl From<pb::WorktreeSpec> for agent_core::WorktreeSpec {
    fn from(s: pb::WorktreeSpec) -> Self {
        agent_core::WorktreeSpec {
            revision: agent_core::Revision(s.revision),
            writable: s.writable,
            id: s.id,
        }
    }
}

impl From<agent_core::Checkpoint> for pb::Checkpoint {
    fn from(c: agent_core::Checkpoint) -> Self {
        pb::Checkpoint {
            name: c.name,
            oid: c.oid.0,
            ref_name: c.ref_name,
        }
    }
}

impl From<pb::Checkpoint> for agent_core::Checkpoint {
    fn from(c: pb::Checkpoint) -> Self {
        agent_core::Checkpoint {
            name: c.name,
            oid: agent_core::Oid(c.oid),
            ref_name: c.ref_name,
        }
    }
}

// --- Repo: RepoStatus (heads map ↔ repeated Branch) ------------------------

impl From<agent_core::RepoStatus> for pb::RepoStatus {
    fn from(s: agent_core::RepoStatus) -> Self {
        pb::RepoStatus {
            mirror_path: s.mirror_path.to_string_lossy().into_owned(),
            last_fetch_ms: s.last_fetch_ms,
            live_worktrees: s.live_worktrees,
            heads: s
                .heads
                .into_iter()
                .map(|(name, oid)| pb::Branch { name, oid: oid.0 })
                .collect(),
        }
    }
}

impl From<pb::RepoStatus> for agent_core::RepoStatus {
    fn from(s: pb::RepoStatus) -> Self {
        agent_core::RepoStatus {
            mirror_path: std::path::PathBuf::from(s.mirror_path),
            last_fetch_ms: s.last_fetch_ms,
            live_worktrees: s.live_worktrees,
            heads: s
                .heads
                .into_iter()
                .map(|b| (b.name, agent_core::Oid(b.oid)))
                .collect(),
        }
    }
}

// --- Lsp -------------------------------------------------------------------

pub fn lsp_method_from_i32(v: i32) -> agent_core::LspMethod {
    match v {
        1 => agent_core::LspMethod::Hover,
        2 => agent_core::LspMethod::Definition,
        3 => agent_core::LspMethod::References,
        4 => agent_core::LspMethod::Rename,
        5 => agent_core::LspMethod::DocumentSymbols,
        _ => agent_core::LspMethod::Diagnostics,
    }
}

/// Saturating decode. An unknown severity reads as `Hint` — the LEAST severe. A
/// garbled field must not manufacture an `Error` the caller then reports as a
/// real compile failure.
pub fn lsp_severity_from_i32(v: i32) -> agent_core::DiagnosticSeverity {
    match v {
        0 => agent_core::DiagnosticSeverity::Error,
        1 => agent_core::DiagnosticSeverity::Warning,
        2 => agent_core::DiagnosticSeverity::Information,
        _ => agent_core::DiagnosticSeverity::Hint,
    }
}

impl From<agent_core::LspMethod> for pb::LspMethod {
    fn from(m: agent_core::LspMethod) -> Self {
        match m {
            agent_core::LspMethod::Diagnostics => pb::LspMethod::Diagnostics,
            agent_core::LspMethod::Hover => pb::LspMethod::Hover,
            agent_core::LspMethod::Definition => pb::LspMethod::Definition,
            agent_core::LspMethod::References => pb::LspMethod::References,
            agent_core::LspMethod::Rename => pb::LspMethod::Rename,
            agent_core::LspMethod::DocumentSymbols => pb::LspMethod::DocumentSymbols,
        }
    }
}

impl From<agent_core::DiagnosticSeverity> for pb::LspDiagnosticSeverity {
    fn from(s: agent_core::DiagnosticSeverity) -> Self {
        match s {
            agent_core::DiagnosticSeverity::Error => pb::LspDiagnosticSeverity::Error,
            agent_core::DiagnosticSeverity::Warning => pb::LspDiagnosticSeverity::Warning,
            agent_core::DiagnosticSeverity::Information => pb::LspDiagnosticSeverity::Information,
            agent_core::DiagnosticSeverity::Hint => pb::LspDiagnosticSeverity::Hint,
        }
    }
}

impl From<agent_core::Position> for pb::LspPosition {
    fn from(p: agent_core::Position) -> Self {
        pb::LspPosition {
            line: p.line,
            character: p.character,
        }
    }
}

impl From<pb::LspPosition> for agent_core::Position {
    fn from(p: pb::LspPosition) -> Self {
        agent_core::Position {
            line: p.line,
            character: p.character,
        }
    }
}

impl From<agent_core::Range> for pb::LspRange {
    fn from(r: agent_core::Range) -> Self {
        pb::LspRange {
            start: Some(r.start.into()),
            end: Some(r.end.into()),
        }
    }
}

impl From<pb::LspRange> for agent_core::Range {
    fn from(r: pb::LspRange) -> Self {
        // A missing endpoint defaults to 0:0 rather than erroring: a range is
        // positional metadata, and losing a whole diagnostic because one bound
        // was absent would hide the message the caller actually needs.
        agent_core::Range {
            start: r.start.map(Into::into).unwrap_or_default(),
            end: r.end.map(Into::into).unwrap_or_default(),
        }
    }
}

impl From<agent_core::Location> for pb::LspLocation {
    fn from(l: agent_core::Location) -> Self {
        pb::LspLocation {
            uri: l.uri,
            range: Some(l.range.into()),
        }
    }
}

impl From<pb::LspLocation> for agent_core::Location {
    fn from(l: pb::LspLocation) -> Self {
        agent_core::Location {
            uri: l.uri,
            range: l.range.map(Into::into).unwrap_or_default(),
        }
    }
}

impl From<agent_core::Diagnostic> for pb::LspDiagnostic {
    fn from(d: agent_core::Diagnostic) -> Self {
        pb::LspDiagnostic {
            range: Some(d.range.into()),
            severity: pb::LspDiagnosticSeverity::from(d.severity) as i32,
            message: d.message,
            code: d.code,
            source: d.source,
        }
    }
}

impl From<pb::LspDiagnostic> for agent_core::Diagnostic {
    fn from(d: pb::LspDiagnostic) -> Self {
        agent_core::Diagnostic {
            range: d.range.map(Into::into).unwrap_or_default(),
            severity: lsp_severity_from_i32(d.severity),
            message: d.message,
            code: d.code,
            source: d.source,
        }
    }
}

impl From<agent_core::TextEdit> for pb::LspTextEdit {
    fn from(e: agent_core::TextEdit) -> Self {
        pb::LspTextEdit {
            range: Some(e.range.into()),
            new_text: e.new_text,
        }
    }
}

impl From<pb::LspTextEdit> for agent_core::TextEdit {
    fn from(e: pb::LspTextEdit) -> Self {
        agent_core::TextEdit {
            range: e.range.map(Into::into).unwrap_or_default(),
            new_text: e.new_text,
        }
    }
}

impl From<agent_core::WorkspaceEdit> for pb::LspWorkspaceEdit {
    fn from(w: agent_core::WorkspaceEdit) -> Self {
        pb::LspWorkspaceEdit {
            changes: w
                .changes
                .into_iter()
                .map(|(uri, edits)| pb::LspFileEdits {
                    uri,
                    edits: edits.into_iter().map(Into::into).collect(),
                })
                .collect(),
        }
    }
}

impl From<pb::LspWorkspaceEdit> for agent_core::WorkspaceEdit {
    fn from(w: pb::LspWorkspaceEdit) -> Self {
        agent_core::WorkspaceEdit {
            changes: w
                .changes
                .into_iter()
                .map(|f| (f.uri, f.edits.into_iter().map(Into::into).collect()))
                .collect(),
        }
    }
}

impl From<agent_core::DocumentSymbol> for pb::LspDocumentSymbol {
    fn from(s: agent_core::DocumentSymbol) -> Self {
        pb::LspDocumentSymbol {
            name: s.name,
            kind: s.kind,
            range: Some(s.range.into()),
        }
    }
}

impl From<pb::LspDocumentSymbol> for agent_core::DocumentSymbol {
    fn from(s: pb::LspDocumentSymbol) -> Self {
        agent_core::DocumentSymbol {
            name: s.name,
            kind: s.kind,
            range: s.range.map(Into::into).unwrap_or_default(),
        }
    }
}

impl From<agent_core::LspRequest> for pb::LspRequestMsg {
    fn from(r: agent_core::LspRequest) -> Self {
        pb::LspRequestMsg {
            method: pb::LspMethod::from(r.method) as i32,
            uri: r.uri,
            position: r.position.map(Into::into),
            new_name: r.new_name,
        }
    }
}

impl From<pb::LspRequestMsg> for agent_core::LspRequest {
    fn from(r: pb::LspRequestMsg) -> Self {
        agent_core::LspRequest {
            method: lsp_method_from_i32(r.method),
            uri: r.uri,
            position: r.position.map(Into::into),
            new_name: r.new_name,
        }
    }
}

impl From<agent_core::LspResult> for pb::LspResultMsg {
    fn from(r: agent_core::LspResult) -> Self {
        use pb::lsp_result_msg::Kind;
        pb::LspResultMsg {
            kind: Some(match r {
                agent_core::LspResult::Diagnostics(d) => Kind::Diagnostics(pb::LspDiagnostics {
                    items: d.into_iter().map(Into::into).collect(),
                }),
                agent_core::LspResult::Hover(h) => Kind::Hover(pb::LspHoverResult {
                    hover: h.map(|h| pb::LspHover {
                        contents: h.contents,
                    }),
                }),
                agent_core::LspResult::Locations(l) => Kind::Locations(pb::LspLocations {
                    items: l.into_iter().map(Into::into).collect(),
                }),
                agent_core::LspResult::Symbols(s) => Kind::Symbols(pb::LspSymbols {
                    items: s.into_iter().map(Into::into).collect(),
                }),
                agent_core::LspResult::Rename(w) => Kind::Rename(w.into()),
            }),
        }
    }
}

impl TryFrom<pb::LspResultMsg> for agent_core::LspResult {
    type Error = ConvertError;
    fn try_from(r: pb::LspResultMsg) -> Result<Self, Self::Error> {
        use pb::lsp_result_msg::Kind;
        // No default: the variant IS the answer's type. Guessing (say, empty
        // diagnostics) would report "no problems found" for a request that in
        // fact returned nothing at all.
        Ok(
            match r
                .kind
                .ok_or(ConvertError::MissingField("lsp_result.kind"))?
            {
                Kind::Diagnostics(d) => agent_core::LspResult::Diagnostics(
                    d.items.into_iter().map(Into::into).collect(),
                ),
                Kind::Hover(h) => {
                    agent_core::LspResult::Hover(h.hover.map(|h| agent_core::Hover {
                        contents: h.contents,
                    }))
                }
                Kind::Locations(l) => {
                    agent_core::LspResult::Locations(l.items.into_iter().map(Into::into).collect())
                }
                Kind::Symbols(s) => {
                    agent_core::LspResult::Symbols(s.items.into_iter().map(Into::into).collect())
                }
                Kind::Rename(w) => agent_core::LspResult::Rename(w.into()),
            },
        )
    }
}

impl From<agent_core::LspCapabilities> for pb::LspCapabilities {
    fn from(c: agent_core::LspCapabilities) -> Self {
        pb::LspCapabilities {
            server: c.server,
            methods: c
                .methods
                .into_iter()
                .map(|m| pb::LspMethod::from(m) as i32)
                .collect(),
        }
    }
}

impl From<pb::LspCapabilities> for agent_core::LspCapabilities {
    fn from(c: pb::LspCapabilities) -> Self {
        agent_core::LspCapabilities {
            server: c.server,
            methods: c.methods.into_iter().map(lsp_method_from_i32).collect(),
        }
    }
}

// --- Forge -----------------------------------------------------------------

/// Saturating decode. An unknown verdict reads as `Comment` — the INERT one. A
/// garbled field must never be read as `Approve`, which would let a malformed
/// message approve a pull request.
pub fn forge_verdict_from_i32(v: i32) -> agent_core::ReviewVerdict {
    match v {
        1 => agent_core::ReviewVerdict::Approve,
        2 => agent_core::ReviewVerdict::RequestChanges,
        _ => agent_core::ReviewVerdict::Comment,
    }
}

impl From<agent_core::ReviewVerdict> for pb::ForgeReviewVerdict {
    fn from(v: agent_core::ReviewVerdict) -> Self {
        match v {
            agent_core::ReviewVerdict::Approve => pb::ForgeReviewVerdict::Approve,
            agent_core::ReviewVerdict::RequestChanges => pb::ForgeReviewVerdict::RequestChanges,
            agent_core::ReviewVerdict::Comment => pb::ForgeReviewVerdict::Comment,
        }
    }
}

impl From<agent_core::Comment> for pb::ForgeComment {
    fn from(c: agent_core::Comment) -> Self {
        pb::ForgeComment {
            author: c.author,
            body: c.body,
            url: c.url,
        }
    }
}

impl From<pb::ForgeComment> for agent_core::Comment {
    fn from(c: pb::ForgeComment) -> Self {
        agent_core::Comment {
            author: c.author,
            body: c.body,
            url: c.url,
        }
    }
}

impl From<agent_core::PullRequest> for pb::ForgePullRequest {
    fn from(p: agent_core::PullRequest) -> Self {
        pb::ForgePullRequest {
            number: p.number,
            title: p.title,
            body: p.body,
            state: p.state,
            author: p.author,
            url: p.url,
            source_branch: p.source_branch,
            target_branch: p.target_branch,
            draft: p.draft,
        }
    }
}

impl From<pb::ForgePullRequest> for agent_core::PullRequest {
    fn from(p: pb::ForgePullRequest) -> Self {
        agent_core::PullRequest {
            number: p.number,
            title: p.title,
            body: p.body,
            state: p.state,
            author: p.author,
            url: p.url,
            source_branch: p.source_branch,
            target_branch: p.target_branch,
            draft: p.draft,
        }
    }
}

impl From<agent_core::Issue> for pb::ForgeIssue {
    fn from(i: agent_core::Issue) -> Self {
        pb::ForgeIssue {
            number: i.number,
            title: i.title,
            body: i.body,
            state: i.state,
            author: i.author,
            url: i.url,
            labels: i.labels,
            comments: i.comments.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<pb::ForgeIssue> for agent_core::Issue {
    fn from(i: pb::ForgeIssue) -> Self {
        agent_core::Issue {
            number: i.number,
            title: i.title,
            body: i.body,
            state: i.state,
            author: i.author,
            url: i.url,
            labels: i.labels,
            comments: i.comments.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<agent_core::CreatePrRequest> for pb::ForgeCreatePrRequest {
    fn from(r: agent_core::CreatePrRequest) -> Self {
        pb::ForgeCreatePrRequest {
            title: r.title,
            body: r.body,
            source_branch: r.source_branch,
            target_branch: r.target_branch,
            draft: r.draft,
        }
    }
}

impl From<pb::ForgeCreatePrRequest> for agent_core::CreatePrRequest {
    fn from(r: pb::ForgeCreatePrRequest) -> Self {
        agent_core::CreatePrRequest {
            title: r.title,
            body: r.body,
            source_branch: r.source_branch,
            target_branch: r.target_branch,
            draft: r.draft,
        }
    }
}

// --- TaskTracker -----------------------------------------------------------

/// Saturating decode. An unknown status reads as `Pending` — the neutral one. In
/// particular it must NOT read as `Completed`: a garbled field marking work done
/// would make the agent skip it.
pub fn task_status_from_i32(v: i32) -> agent_core::TodoStatus {
    match v {
        1 => agent_core::TodoStatus::InProgress,
        2 => agent_core::TodoStatus::Completed,
        3 => agent_core::TodoStatus::Cancelled,
        _ => agent_core::TodoStatus::Pending,
    }
}

pub fn task_priority_from_i32(v: i32) -> agent_core::TodoPriority {
    match v {
        1 => agent_core::TodoPriority::High,
        2 => agent_core::TodoPriority::Low,
        _ => agent_core::TodoPriority::Medium,
    }
}

impl From<agent_core::TodoStatus> for pb::TaskStatus {
    fn from(s: agent_core::TodoStatus) -> Self {
        match s {
            agent_core::TodoStatus::Pending => pb::TaskStatus::Pending,
            agent_core::TodoStatus::InProgress => pb::TaskStatus::InProgress,
            agent_core::TodoStatus::Completed => pb::TaskStatus::Completed,
            agent_core::TodoStatus::Cancelled => pb::TaskStatus::Cancelled,
        }
    }
}

impl From<agent_core::TodoPriority> for pb::TaskPriority {
    fn from(p: agent_core::TodoPriority) -> Self {
        match p {
            agent_core::TodoPriority::High => pb::TaskPriority::High,
            agent_core::TodoPriority::Medium => pb::TaskPriority::Medium,
            agent_core::TodoPriority::Low => pb::TaskPriority::Low,
        }
    }
}

impl From<agent_core::Todo> for pb::Task {
    fn from(t: agent_core::Todo) -> Self {
        pb::Task {
            content: t.content,
            status: pb::TaskStatus::from(t.status) as i32,
            priority: pb::TaskPriority::from(t.priority) as i32,
        }
    }
}

impl From<pb::Task> for agent_core::Todo {
    fn from(t: pb::Task) -> Self {
        agent_core::Todo {
            content: t.content,
            status: task_status_from_i32(t.status),
            priority: task_priority_from_i32(t.priority),
        }
    }
}

impl From<agent_core::TodoPatch> for pb::TaskUpdateRequest {
    fn from(p: agent_core::TodoPatch) -> Self {
        pb::TaskUpdateRequest {
            content: p.content,
            status: p.status.map(|s| pb::TaskStatus::from(s) as i32),
            priority: p.priority.map(|p| pb::TaskPriority::from(p) as i32),
        }
    }
}

impl From<pb::TaskUpdateRequest> for agent_core::TodoPatch {
    fn from(p: pb::TaskUpdateRequest) -> Self {
        agent_core::TodoPatch {
            content: p.content,
            // `None` means "leave unchanged", which is meaningfully different
            // from any concrete value — so the optionality is carried, not
            // flattened to a default.
            status: p.status.map(task_status_from_i32),
            priority: p.priority.map(task_priority_from_i32),
        }
    }
}

// --- Sandbox ---------------------------------------------------------------

/// Saturating decode. An unknown network policy reads as `Off` — the RESTRICTIVE
/// value. A garbled field must not silently grant network access.
pub fn exec_network_from_i32(v: i32) -> agent_core::NetworkPolicy {
    match v {
        0 => agent_core::NetworkPolicy::On,
        2 => agent_core::NetworkPolicy::Loopback,
        _ => agent_core::NetworkPolicy::Off,
    }
}

/// Saturating decode. An unknown env policy reads as `Scrub` — the restrictive
/// value, so a garbled field cannot silently leak the environment.
pub fn exec_env_from_i32(v: i32) -> agent_core::EnvPolicy {
    match v {
        0 => agent_core::EnvPolicy::Inherit,
        _ => agent_core::EnvPolicy::Scrub,
    }
}

impl From<agent_core::NetworkPolicy> for pb::ExecNetworkPolicy {
    fn from(p: agent_core::NetworkPolicy) -> Self {
        match p {
            agent_core::NetworkPolicy::On => pb::ExecNetworkPolicy::On,
            agent_core::NetworkPolicy::Off => pb::ExecNetworkPolicy::Off,
            agent_core::NetworkPolicy::Loopback => pb::ExecNetworkPolicy::Loopback,
        }
    }
}

impl From<agent_core::EnvPolicy> for pb::ExecEnvPolicy {
    fn from(p: agent_core::EnvPolicy) -> Self {
        match p {
            agent_core::EnvPolicy::Inherit => pb::ExecEnvPolicy::Inherit,
            agent_core::EnvPolicy::Scrub => pb::ExecEnvPolicy::Scrub,
        }
    }
}

impl From<agent_core::ExecSpec> for pb::ExecRequest {
    fn from(s: agent_core::ExecSpec) -> Self {
        pb::ExecRequest {
            command: s.command,
            cwd: s.cwd.to_string_lossy().into_owned(),
            network: pb::ExecNetworkPolicy::from(s.network) as i32,
            env: pb::ExecEnvPolicy::from(s.env) as i32,
            timeout_secs: s.timeout_secs,
        }
    }
}

impl From<pb::ExecRequest> for agent_core::ExecSpec {
    fn from(s: pb::ExecRequest) -> Self {
        agent_core::ExecSpec {
            command: s.command,
            cwd: std::path::PathBuf::from(s.cwd),
            network: exec_network_from_i32(s.network),
            env: exec_env_from_i32(s.env),
            timeout_secs: s.timeout_secs,
        }
    }
}

impl From<agent_core::ExecOutput> for pb::ExecResult {
    fn from(o: agent_core::ExecOutput) -> Self {
        pb::ExecResult {
            stdout: o.stdout,
            stderr: o.stderr,
            exit_code: o.exit_code,
            timed_out: o.timed_out,
        }
    }
}

impl From<pb::ExecResult> for agent_core::ExecOutput {
    fn from(o: pb::ExecResult) -> Self {
        agent_core::ExecOutput {
            stdout: o.stdout,
            stderr: o.stderr,
            exit_code: o.exit_code,
            timed_out: o.timed_out,
        }
    }
}

impl From<agent_core::SandboxCapabilities> for pb::ExecCapabilities {
    fn from(c: agent_core::SandboxCapabilities) -> Self {
        pb::ExecCapabilities {
            backend: c.backend,
            available: c.available,
            network_off: c.network_off,
            private_tmp: c.private_tmp,
            content_addressed: c.content_addressed,
        }
    }
}

impl From<pb::ExecCapabilities> for agent_core::SandboxCapabilities {
    fn from(c: pb::ExecCapabilities) -> Self {
        agent_core::SandboxCapabilities {
            backend: c.backend,
            available: c.available,
            network_off: c.network_off,
            private_tmp: c.private_tmp,
            content_addressed: c.content_addressed,
        }
    }
}

// --- Pty -------------------------------------------------------------------

impl From<agent_core::PtyState> for pb::PtyStateMsg {
    fn from(s: agent_core::PtyState) -> Self {
        match s {
            agent_core::PtyState::Running => pb::PtyStateMsg {
                running: true,
                closed: false,
                exit_code: None,
            },
            agent_core::PtyState::Exited { code } => pb::PtyStateMsg {
                running: false,
                closed: false,
                exit_code: Some(code),
            },
            agent_core::PtyState::Closed => pb::PtyStateMsg {
                running: false,
                closed: true,
                exit_code: None,
            },
        }
    }
}

impl From<pb::PtyStateMsg> for agent_core::PtyState {
    fn from(s: pb::PtyStateMsg) -> Self {
        // Order matters: `running` wins, then `closed`, then an exit code. A
        // garbled combination must resolve to something, and reporting a live
        // session as `Closed` would strand a real child process.
        if s.running {
            agent_core::PtyState::Running
        } else if s.closed {
            agent_core::PtyState::Closed
        } else {
            agent_core::PtyState::Exited {
                code: s.exit_code.unwrap_or(-1),
            }
        }
    }
}

/// Terminal dimensions are `u16` in core but `u32` on the wire. Clamp to the
/// same 1..=1000 range the local pty tool enforces: a 0- or 60000-column ioctl
/// is nonsense, and 0 rows can wedge a child.
fn pty_dim(v: u32) -> u16 {
    v.clamp(1, 1000) as u16
}

impl From<agent_core::PtySpec> for pb::PtyOpenRequest {
    fn from(s: agent_core::PtySpec) -> Self {
        pb::PtyOpenRequest {
            command: s.command,
            args: s.args,
            cols: s.cols as u32,
            rows: s.rows as u32,
            cwd: s.cwd,
        }
    }
}

impl From<pb::PtyOpenRequest> for agent_core::PtySpec {
    fn from(s: pb::PtyOpenRequest) -> Self {
        agent_core::PtySpec {
            command: s.command,
            args: s.args,
            cols: pty_dim(s.cols),
            rows: pty_dim(s.rows),
            cwd: s.cwd,
        }
    }
}

impl From<agent_core::PtySessionInfo> for pb::PtySessionInfo {
    fn from(i: agent_core::PtySessionInfo) -> Self {
        pb::PtySessionInfo {
            id: i.id,
            command: i.command,
            state: Some(i.state.into()),
            cols: i.cols as u32,
            rows: i.rows as u32,
            bytes_out: i.bytes_out,
            first_retained: i.first_retained,
            next_cursor: i.next_cursor,
        }
    }
}

impl TryFrom<pb::PtySessionInfo> for agent_core::PtySessionInfo {
    type Error = ConvertError;
    fn try_from(i: pb::PtySessionInfo) -> Result<Self, Self::Error> {
        Ok(agent_core::PtySessionInfo {
            id: i.id,
            command: i.command,
            // No sensible default: guessing `Running` would report a dead
            // session as live, and guessing `Closed` would strand a live one.
            state: i
                .state
                .ok_or(ConvertError::MissingField("pty_session.state"))?
                .into(),
            cols: pty_dim(i.cols),
            rows: pty_dim(i.rows),
            bytes_out: i.bytes_out,
            first_retained: i.first_retained,
            next_cursor: i.next_cursor,
        })
    }
}

// --- Web (WebBackend + WebSearch) ------------------------------------------

pub fn web_format_from_i32(v: i32) -> agent_core::WebFormat {
    match v {
        1 => agent_core::WebFormat::Text,
        2 => agent_core::WebFormat::Html,
        _ => agent_core::WebFormat::Markdown,
    }
}

/// Saturating decode: an unknown state reads as `Missing`, the conservative
/// answer — it means "fetch it", never "serve something stale as fresh".
pub fn web_cache_state_from_i32(v: i32) -> agent_core::CacheState {
    match v {
        1 => agent_core::CacheState::Fresh,
        2 => agent_core::CacheState::Stale,
        _ => agent_core::CacheState::Missing,
    }
}

impl From<agent_core::WebFormat> for pb::WebFormat {
    fn from(f: agent_core::WebFormat) -> Self {
        match f {
            agent_core::WebFormat::Markdown => pb::WebFormat::Markdown,
            agent_core::WebFormat::Text => pb::WebFormat::Text,
            agent_core::WebFormat::Html => pb::WebFormat::Html,
        }
    }
}

impl From<agent_core::CacheState> for pb::WebCacheState {
    fn from(s: agent_core::CacheState) -> Self {
        match s {
            agent_core::CacheState::Missing => pb::WebCacheState::Missing,
            agent_core::CacheState::Fresh => pb::WebCacheState::Fresh,
            agent_core::CacheState::Stale => pb::WebCacheState::Stale,
        }
    }
}

impl From<agent_core::WebRequest> for pb::WebFetchRequest {
    fn from(r: agent_core::WebRequest) -> Self {
        pb::WebFetchRequest {
            url: r.url,
            format: pb::WebFormat::from(r.format) as i32,
            timeout_secs: r.timeout_secs,
            max_bytes: r.max_bytes,
            max_redirects: r.max_redirects,
        }
    }
}

impl From<pb::WebFetchRequest> for agent_core::WebRequest {
    fn from(r: pb::WebFetchRequest) -> Self {
        agent_core::WebRequest {
            url: r.url,
            format: web_format_from_i32(r.format),
            timeout_secs: r.timeout_secs,
            max_bytes: r.max_bytes,
            max_redirects: r.max_redirects,
        }
    }
}

impl From<agent_core::WebResponse> for pb::WebFetchResponse {
    fn from(r: agent_core::WebResponse) -> Self {
        pb::WebFetchResponse {
            final_url: r.final_url,
            status: r.status as u32,
            content_type: r.content_type,
            format: pb::WebFormat::from(r.format) as i32,
            body: r.body,
            bytes: r.bytes,
        }
    }
}

impl From<pb::WebFetchResponse> for agent_core::WebResponse {
    fn from(r: pb::WebFetchResponse) -> Self {
        agent_core::WebResponse {
            final_url: r.final_url,
            // A status past u16 is a malformed server, not a real HTTP code;
            // saturate rather than wrapping into a plausible-looking one (a
            // wrapped 65536 would read as 0, and 65736 as 200 — "success").
            status: u16::try_from(r.status).unwrap_or(u16::MAX),
            content_type: r.content_type,
            format: web_format_from_i32(r.format),
            body: r.body,
            bytes: r.bytes,
        }
    }
}

impl From<agent_core::WebQuery> for pb::WebSearchRequest {
    fn from(q: agent_core::WebQuery) -> Self {
        pb::WebSearchRequest {
            text: q.text,
            limit: q.limit,
            freshness_days: q.freshness_days,
            backend: q.backend,
        }
    }
}

impl From<pb::WebSearchRequest> for agent_core::WebQuery {
    fn from(q: pb::WebSearchRequest) -> Self {
        agent_core::WebQuery {
            text: q.text,
            limit: q.limit,
            freshness_days: q.freshness_days,
            backend: q.backend,
        }
    }
}

impl From<agent_core::WebResult> for pb::WebSearchResult {
    fn from(r: agent_core::WebResult) -> Self {
        pb::WebSearchResult {
            url: r.url,
            title: r.title,
            snippet: r.snippet,
            score: r.score,
            published_ms: r.published_ms,
        }
    }
}

impl From<pb::WebSearchResult> for agent_core::WebResult {
    fn from(r: pb::WebSearchResult) -> Self {
        agent_core::WebResult {
            url: r.url,
            title: r.title,
            snippet: r.snippet,
            // A NaN or out-of-range score scrambles ordering: `partial_cmp`
            // returns `None`, which collapses to `Equal` and corrupts the whole
            // sort. Sanitise at the boundary.
            score: if r.score.is_finite() {
                r.score.clamp(0.0, 1.0)
            } else {
                0.0
            },
            published_ms: r.published_ms,
        }
    }
}

impl From<agent_core::WebSearchCapabilities> for pb::WebSearchCapabilities {
    fn from(c: agent_core::WebSearchCapabilities) -> Self {
        pb::WebSearchCapabilities {
            backend: c.backend,
            scored: c.scored,
            freshness: c.freshness,
            max_results: c.max_results,
        }
    }
}

impl From<pb::WebSearchCapabilities> for agent_core::WebSearchCapabilities {
    fn from(c: pb::WebSearchCapabilities) -> Self {
        agent_core::WebSearchCapabilities {
            backend: c.backend,
            scored: c.scored,
            freshness: c.freshness,
            max_results: c.max_results,
        }
    }
}

// --- ReferenceResolver -----------------------------------------------------

impl From<agent_core::Resolution> for pb::RefResolution {
    fn from(r: agent_core::Resolution) -> Self {
        pb::RefResolution {
            blocks: r.blocks.into_iter().map(Into::into).collect(),
            warnings: r.warnings,
            blocked: r.blocked,
        }
    }
}

impl From<pb::RefResolution> for agent_core::Resolution {
    fn from(r: pb::RefResolution) -> Self {
        agent_core::Resolution {
            blocks: r.blocks.into_iter().map(Into::into).collect(),
            warnings: r.warnings,
            blocked: r.blocked,
        }
    }
}

// --- Scheduler -------------------------------------------------------------

/// Saturating decode: an unknown wire outcome reads as `Failed` rather than
/// `Completed`. A garbled history entry must not be reported as a success.
pub fn sched_outcome_from_i32(v: i32) -> agent_core::RunOutcome {
    match v {
        0 => agent_core::RunOutcome::Completed,
        2 => agent_core::RunOutcome::Skipped,
        _ => agent_core::RunOutcome::Failed,
    }
}

impl From<agent_core::RunOutcome> for pb::SchedRunOutcome {
    fn from(o: agent_core::RunOutcome) -> Self {
        match o {
            agent_core::RunOutcome::Completed => pb::SchedRunOutcome::Completed,
            agent_core::RunOutcome::Failed => pb::SchedRunOutcome::Failed,
            agent_core::RunOutcome::Skipped => pb::SchedRunOutcome::Skipped,
        }
    }
}

impl From<agent_core::Schedule> for pb::SchedSchedule {
    fn from(s: agent_core::Schedule) -> Self {
        use pb::sched_schedule::Kind;
        pb::SchedSchedule {
            kind: Some(match s {
                agent_core::Schedule::Interval { secs } => Kind::IntervalSecs(secs),
                agent_core::Schedule::Cron { expr } => Kind::CronExpr(expr),
                agent_core::Schedule::Once { at_ms } => Kind::OnceAtMs(at_ms),
            }),
        }
    }
}

impl TryFrom<pb::SchedSchedule> for agent_core::Schedule {
    type Error = ConvertError;
    fn try_from(s: pb::SchedSchedule) -> Result<Self, Self::Error> {
        use pb::sched_schedule::Kind;
        // An absent kind is a malformed job, not a defaultable one: guessing
        // (say, "every 0s") would spin, and guessing a far-future one-shot would
        // silently never fire. Reject it.
        match s.kind.ok_or(ConvertError::MissingField("schedule.kind"))? {
            Kind::IntervalSecs(secs) => Ok(agent_core::Schedule::Interval { secs }),
            Kind::CronExpr(expr) => Ok(agent_core::Schedule::Cron { expr }),
            Kind::OnceAtMs(at_ms) => Ok(agent_core::Schedule::Once { at_ms }),
        }
    }
}

impl From<agent_core::Job> for pb::SchedJob {
    fn from(j: agent_core::Job) -> Self {
        pb::SchedJob {
            id: j.id,
            spec: j.spec,
            schedule: Some(j.schedule.into()),
            goal: j.goal,
            next_fire_ms: j.next_fire_ms,
            enabled: j.enabled,
        }
    }
}

impl TryFrom<pb::SchedJob> for agent_core::Job {
    type Error = ConvertError;
    fn try_from(j: pb::SchedJob) -> Result<Self, Self::Error> {
        Ok(agent_core::Job {
            id: j.id,
            spec: j.spec,
            schedule: j
                .schedule
                .ok_or(ConvertError::MissingField("job.schedule"))?
                .try_into()?,
            goal: j.goal,
            next_fire_ms: j.next_fire_ms,
            enabled: j.enabled,
        })
    }
}

impl From<agent_core::Run> for pb::SchedRun {
    fn from(r: agent_core::Run) -> Self {
        pb::SchedRun {
            job_id: r.job_id,
            started_ms: r.started_ms,
            finished_ms: r.finished_ms,
            outcome: pb::SchedRunOutcome::from(r.outcome) as i32,
            detail: r.detail,
        }
    }
}

impl From<pb::SchedRun> for agent_core::Run {
    fn from(r: pb::SchedRun) -> Self {
        agent_core::Run {
            job_id: r.job_id,
            started_ms: r.started_ms,
            finished_ms: r.finished_ms,
            outcome: sched_outcome_from_i32(r.outcome),
            detail: r.detail,
        }
    }
}

// --- Scanner ---------------------------------------------------------------

/// Saturating decode: an unknown wire enum degrades to the least-severe value
/// rather than erroring. A garbled severity must not be read as `Critical` (a
/// remote could then deny everything) nor fail the scan outright.
pub fn scan_severity_from_i32(v: i32) -> agent_core::Severity {
    match v {
        1 => agent_core::Severity::Low,
        2 => agent_core::Severity::Medium,
        3 => agent_core::Severity::High,
        4 => agent_core::Severity::Critical,
        _ => agent_core::Severity::Info,
    }
}

pub fn scan_kind_from_i32(v: i32) -> agent_core::ScanKind {
    match v {
        1 => agent_core::ScanKind::FileBody,
        2 => agent_core::ScanKind::WebContent,
        3 => agent_core::ScanKind::Lockfile,
        _ => agent_core::ScanKind::ToolInput,
    }
}

impl From<agent_core::Severity> for pb::ScanSeverity {
    fn from(s: agent_core::Severity) -> Self {
        match s {
            agent_core::Severity::Info => pb::ScanSeverity::Info,
            agent_core::Severity::Low => pb::ScanSeverity::Low,
            agent_core::Severity::Medium => pb::ScanSeverity::Medium,
            agent_core::Severity::High => pb::ScanSeverity::High,
            agent_core::Severity::Critical => pb::ScanSeverity::Critical,
        }
    }
}

impl From<agent_core::ScanKind> for pb::ScanKind {
    fn from(k: agent_core::ScanKind) -> Self {
        match k {
            agent_core::ScanKind::ToolInput => pb::ScanKind::ToolInput,
            agent_core::ScanKind::FileBody => pb::ScanKind::FileBody,
            agent_core::ScanKind::WebContent => pb::ScanKind::WebContent,
            agent_core::ScanKind::Lockfile => pb::ScanKind::Lockfile,
        }
    }
}

impl From<agent_core::Finding> for pb::ScanFinding {
    fn from(f: agent_core::Finding) -> Self {
        pb::ScanFinding {
            rule: f.rule,
            severity: pb::ScanSeverity::from(f.severity) as i32,
            category: f.category.to_string(),
            span_start: f.span.start as u64,
            span_end: f.span.end as u64,
        }
    }
}

impl From<pb::ScanFinding> for agent_core::Finding {
    fn from(f: pb::ScanFinding) -> Self {
        // `category` is `&'static str` in core but arbitrary text on the wire.
        // Map the known set and funnel anything else to a single static label
        // rather than leaking a remote-controlled string via `Box::leak` — an
        // unbounded leak an attacker could drive.
        let category = match f.category.as_str() {
            "secret" => "secret",
            "threat" => "threat",
            "injection" => "injection",
            "vuln" => "vuln",
            _ => "unknown",
        };
        // A hostile span must not panic a slicing caller: keep it ordered and
        // let the consumer clamp against its own content length.
        let start = usize::try_from(f.span_start).unwrap_or(usize::MAX);
        let end = usize::try_from(f.span_end).unwrap_or(usize::MAX);
        agent_core::Finding {
            rule: f.rule,
            severity: scan_severity_from_i32(f.severity),
            category,
            span: start..end.max(start),
        }
    }
}

// --- SessionStore (checkpoint metadata) ------------------------------------

impl From<agent_core::CheckpointMeta> for pb::SessionCheckpointMeta {
    fn from(m: agent_core::CheckpointMeta) -> Self {
        pb::SessionCheckpointMeta {
            id: m.id,
            parent: m.parent,
            branch: m.branch,
            turn: m.turn,
            label: m.label,
            created_ms: m.created_ms,
        }
    }
}

impl From<pb::SessionCheckpointMeta> for agent_core::CheckpointMeta {
    fn from(m: pb::SessionCheckpointMeta) -> Self {
        agent_core::CheckpointMeta {
            id: m.id,
            parent: m.parent,
            branch: m.branch,
            turn: m.turn,
            label: m.label,
            created_ms: m.created_ms,
        }
    }
}

impl From<agent_core::CheckpointDiff> for pb::SessionCheckpointDiff {
    fn from(d: agent_core::CheckpointDiff) -> Self {
        pb::SessionCheckpointDiff {
            added: d.added as u64,
            removed: d.removed as u64,
        }
    }
}

impl From<pb::SessionCheckpointDiff> for agent_core::CheckpointDiff {
    fn from(d: pb::SessionCheckpointDiff) -> Self {
        // A hostile/garbled server can send a count past `usize` on a 32-bit
        // target; saturate rather than wrap into a small number.
        agent_core::CheckpointDiff {
            added: usize::try_from(d.added).unwrap_or(usize::MAX),
            removed: usize::try_from(d.removed).unwrap_or(usize::MAX),
        }
    }
}

// --- LlmPool ---------------------------------------------------------------

impl From<agent_core::PoolTier> for pb::PoolTier {
    fn from(t: agent_core::PoolTier) -> Self {
        match t {
            agent_core::PoolTier::Light => pb::PoolTier::Light,
            agent_core::PoolTier::Medium => pb::PoolTier::Medium,
            agent_core::PoolTier::Heavy => pb::PoolTier::Heavy,
        }
    }
}

/// A garbled/unspecified tier decodes to `Light` — the cheap floor, never a
/// silent escalation to an expensive tier.
impl From<pb::PoolTier> for agent_core::PoolTier {
    fn from(t: pb::PoolTier) -> Self {
        match t {
            pb::PoolTier::Heavy => agent_core::PoolTier::Heavy,
            pb::PoolTier::Medium => agent_core::PoolTier::Medium,
            _ => agent_core::PoolTier::Light,
        }
    }
}

/// The i32-on-the-wire form of [`From<pb::PoolTier>`]. `pub` so the server maps a
/// request's tier without re-implementing the safe-floor rule.
pub fn pool_tier_from_i32(v: i32) -> agent_core::PoolTier {
    pb::PoolTier::try_from(v)
        .map(agent_core::PoolTier::from)
        .unwrap_or(agent_core::PoolTier::Light)
}

impl From<agent_core::PoolMemberHealth> for pb::PoolMemberHealth {
    fn from(h: agent_core::PoolMemberHealth) -> Self {
        pb::PoolMemberHealth {
            name: h.name,
            tier: pb::PoolTier::from(h.tier) as i32,
            alive: h.alive,
            consecutive_failures: h.consecutive_failures,
            last_probe_ms: h.last_probe_ms,
        }
    }
}
impl From<pb::PoolMemberHealth> for agent_core::PoolMemberHealth {
    fn from(h: pb::PoolMemberHealth) -> Self {
        agent_core::PoolMemberHealth {
            name: h.name,
            tier: pool_tier_from_i32(h.tier),
            alive: h.alive,
            consecutive_failures: h.consecutive_failures,
            last_probe_ms: h.last_probe_ms,
        }
    }
}

impl From<agent_core::HealthReport> for pb::PoolHealthReport {
    fn from(r: agent_core::HealthReport) -> Self {
        pb::PoolHealthReport {
            members: r.members.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<pb::PoolHealthReport> for agent_core::HealthReport {
    fn from(r: pb::PoolHealthReport) -> Self {
        agent_core::HealthReport {
            members: r.members.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<agent_core::PoolMemberResult> for pb::PoolMemberResult {
    fn from(r: agent_core::PoolMemberResult) -> Self {
        pb::PoolMemberResult {
            member: r.member,
            ok: r.response.is_some(),
            error: r.error.unwrap_or_default(),
            duration_ms: r.duration_ms,
            response: r.response.map(Into::into),
        }
    }
}
impl From<pb::PoolMemberResult> for agent_core::PoolMemberResult {
    fn from(r: pb::PoolMemberResult) -> Self {
        // A malformed response decodes to a failed slot rather than a panic.
        let response = if r.ok {
            r.response
                .and_then(|resp| agent_core::CompletionResponse::try_from(resp).ok())
        } else {
            None
        };
        let error = if response.is_some() {
            None
        } else {
            Some(if r.error.is_empty() {
                "remote pool member failed".to_string()
            } else {
                r.error
            })
        };
        agent_core::PoolMemberResult {
            member: r.member,
            duration_ms: r.duration_ms,
            response,
            error,
        }
    }
}

// --- Review ----------------------------------------------------------------

impl From<agent_core::CollectStatus> for pb::ReviewCollectStatus {
    fn from(s: agent_core::CollectStatus) -> Self {
        match s {
            agent_core::CollectStatus::Ok => pb::ReviewCollectStatus::Ok,
            agent_core::CollectStatus::Partial => pb::ReviewCollectStatus::Partial,
            agent_core::CollectStatus::Skipped => pb::ReviewCollectStatus::Skipped,
            agent_core::CollectStatus::Failed => pb::ReviewCollectStatus::Failed,
        }
    }
}
fn collect_status_from_i32(v: i32) -> agent_core::CollectStatus {
    match pb::ReviewCollectStatus::try_from(v) {
        Ok(pb::ReviewCollectStatus::Ok) => agent_core::CollectStatus::Ok,
        Ok(pb::ReviewCollectStatus::Partial) => agent_core::CollectStatus::Partial,
        Ok(pb::ReviewCollectStatus::Skipped) => agent_core::CollectStatus::Skipped,
        // Failed / unspecified / garbled → Failed (the conservative reading).
        _ => agent_core::CollectStatus::Failed,
    }
}

impl From<agent_core::ForgeHost> for pb::ReviewForgeHost {
    fn from(h: agent_core::ForgeHost) -> Self {
        match h {
            agent_core::ForgeHost::GitHub => pb::ReviewForgeHost::Github,
            agent_core::ForgeHost::GitLab => pb::ReviewForgeHost::Gitlab,
            agent_core::ForgeHost::Other => pb::ReviewForgeHost::Other,
            agent_core::ForgeHost::None => pb::ReviewForgeHost::None,
        }
    }
}
fn forge_host_from_i32(v: i32) -> agent_core::ForgeHost {
    match pb::ReviewForgeHost::try_from(v) {
        Ok(pb::ReviewForgeHost::Github) => agent_core::ForgeHost::GitHub,
        Ok(pb::ReviewForgeHost::Gitlab) => agent_core::ForgeHost::GitLab,
        Ok(pb::ReviewForgeHost::None) => agent_core::ForgeHost::None,
        _ => agent_core::ForgeHost::Other,
    }
}

impl From<agent_core::RepoRelation> for pb::ReviewRepoRelation {
    fn from(r: agent_core::RepoRelation) -> Self {
        match r {
            agent_core::RepoRelation::Clone => pb::ReviewRepoRelation::Clone,
            agent_core::RepoRelation::Fork => pb::ReviewRepoRelation::Fork,
            agent_core::RepoRelation::Unknown => pb::ReviewRepoRelation::Unknown,
        }
    }
}
fn repo_relation_from_i32(v: i32) -> agent_core::RepoRelation {
    match pb::ReviewRepoRelation::try_from(v) {
        Ok(pb::ReviewRepoRelation::Clone) => agent_core::RepoRelation::Clone,
        Ok(pb::ReviewRepoRelation::Fork) => agent_core::RepoRelation::Fork,
        _ => agent_core::RepoRelation::Unknown,
    }
}

impl From<agent_core::RepoLanguage> for pb::ReviewRepoLanguage {
    fn from(l: agent_core::RepoLanguage) -> Self {
        match l {
            agent_core::RepoLanguage::Go => pb::ReviewRepoLanguage::Go,
            agent_core::RepoLanguage::Rust => pb::ReviewRepoLanguage::Rust,
            agent_core::RepoLanguage::Mixed => pb::ReviewRepoLanguage::Mixed,
            agent_core::RepoLanguage::Unknown => pb::ReviewRepoLanguage::Unknown,
        }
    }
}
fn repo_language_from_i32(v: i32) -> agent_core::RepoLanguage {
    match pb::ReviewRepoLanguage::try_from(v) {
        Ok(pb::ReviewRepoLanguage::Go) => agent_core::RepoLanguage::Go,
        Ok(pb::ReviewRepoLanguage::Rust) => agent_core::RepoLanguage::Rust,
        Ok(pb::ReviewRepoLanguage::Mixed) => agent_core::RepoLanguage::Mixed,
        _ => agent_core::RepoLanguage::Unknown,
    }
}

impl From<agent_core::CollectorStatus> for pb::ReviewCollectorStatus {
    fn from(c: agent_core::CollectorStatus) -> Self {
        pb::ReviewCollectorStatus {
            collector: c.collector,
            status: pb::ReviewCollectStatus::from(c.status) as i32,
            reason: c.reason,
            duration_ms: c.duration_ms,
        }
    }
}
impl From<pb::ReviewCollectorStatus> for agent_core::CollectorStatus {
    fn from(c: pb::ReviewCollectorStatus) -> Self {
        agent_core::CollectorStatus {
            collector: c.collector,
            status: collect_status_from_i32(c.status),
            reason: c.reason,
            duration_ms: c.duration_ms,
        }
    }
}

impl From<agent_core::ReviewMeta> for pb::ReviewMeta {
    fn from(m: agent_core::ReviewMeta) -> Self {
        pb::ReviewMeta {
            repo_hash: m.repo_hash,
            base_rev: m.base_rev,
            head_rev: m.head_rev,
            total_ms: m.total_ms,
            collectors: m.collectors.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<pb::ReviewMeta> for agent_core::ReviewMeta {
    fn from(m: pb::ReviewMeta) -> Self {
        agent_core::ReviewMeta {
            repo_hash: m.repo_hash,
            base_rev: m.base_rev,
            head_rev: m.head_rev,
            total_ms: m.total_ms,
            collectors: m.collectors.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<agent_core::ChangedFile> for pb::ReviewChangedFile {
    fn from(f: agent_core::ChangedFile) -> Self {
        pb::ReviewChangedFile {
            path: f.path.to_string_lossy().into_owned(),
            change: pb::ChangeKind::from(f.change) as i32,
            additions: f.additions,
            deletions: f.deletions,
            is_binary: f.is_binary,
            lang: f.lang,
            patch: f.patch,
        }
    }
}
impl From<pb::ReviewChangedFile> for agent_core::ChangedFile {
    fn from(f: pb::ReviewChangedFile) -> Self {
        agent_core::ChangedFile {
            path: std::path::PathBuf::from(f.path),
            change: change_kind_from_i32(f.change),
            additions: f.additions,
            deletions: f.deletions,
            is_binary: f.is_binary,
            lang: f.lang,
            patch: f.patch,
        }
    }
}

impl From<agent_core::ReviewCommit> for pb::ReviewCommit {
    fn from(c: agent_core::ReviewCommit) -> Self {
        pb::ReviewCommit {
            short: c.short,
            summary: c.summary,
            body: c.body,
            author: c.author,
            age_days: c.age_days,
        }
    }
}
impl From<pb::ReviewCommit> for agent_core::ReviewCommit {
    fn from(c: pb::ReviewCommit) -> Self {
        agent_core::ReviewCommit {
            short: c.short,
            summary: c.summary,
            body: c.body,
            author: c.author,
            age_days: c.age_days,
        }
    }
}

impl From<agent_core::ChangeSet> for pb::ReviewChangeSet {
    fn from(c: agent_core::ChangeSet) -> Self {
        pb::ReviewChangeSet {
            base_rev: c.base_rev,
            head_rev: c.head_rev,
            files: c.files.into_iter().map(Into::into).collect(),
            repo_file_count: c.repo_file_count,
            commits: c.commits.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<pb::ReviewChangeSet> for agent_core::ChangeSet {
    fn from(c: pb::ReviewChangeSet) -> Self {
        agent_core::ChangeSet {
            base_rev: c.base_rev,
            head_rev: c.head_rev,
            files: c.files.into_iter().map(Into::into).collect(),
            repo_file_count: c.repo_file_count,
            commits: c.commits.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<agent_core::GitState> for pb::ReviewGitState {
    fn from(g: agent_core::GitState) -> Self {
        pb::ReviewGitState {
            remote_url_hash: g.remote_url_hash,
            host: pb::ReviewForgeHost::from(g.host) as i32,
            relationship: pb::ReviewRepoRelation::from(g.relationship) as i32,
            default_branch: g.default_branch,
            project: pb::ReviewRepoLanguage::from(g.project) as i32,
        }
    }
}
impl From<pb::ReviewGitState> for agent_core::GitState {
    fn from(g: pb::ReviewGitState) -> Self {
        agent_core::GitState {
            remote_url_hash: g.remote_url_hash,
            host: forge_host_from_i32(g.host),
            relationship: repo_relation_from_i32(g.relationship),
            default_branch: g.default_branch,
            project: repo_language_from_i32(g.project),
        }
    }
}

impl From<agent_core::AnalysisFinding> for pb::ReviewAnalysisFinding {
    fn from(f: agent_core::AnalysisFinding) -> Self {
        pb::ReviewAnalysisFinding {
            tool: f.tool,
            rule: f.rule,
            severity: f.severity,
            file: f.file,
            line: f.line,
            message: f.message,
            in_change: f.in_change,
        }
    }
}
impl From<pb::ReviewAnalysisFinding> for agent_core::AnalysisFinding {
    fn from(f: pb::ReviewAnalysisFinding) -> Self {
        agent_core::AnalysisFinding {
            tool: f.tool,
            rule: f.rule,
            severity: f.severity,
            file: f.file,
            line: f.line,
            message: f.message,
            in_change: f.in_change,
        }
    }
}

impl From<agent_core::AnalyzerRun> for pb::ReviewAnalyzerRun {
    fn from(r: agent_core::AnalyzerRun) -> Self {
        pb::ReviewAnalyzerRun {
            tool: r.tool,
            status: r.status,
            reason: r.reason,
            duration_ms: r.duration_ms,
            finding_count: r.finding_count,
        }
    }
}
impl From<pb::ReviewAnalyzerRun> for agent_core::AnalyzerRun {
    fn from(r: pb::ReviewAnalyzerRun) -> Self {
        agent_core::AnalyzerRun {
            tool: r.tool,
            status: r.status,
            reason: r.reason,
            duration_ms: r.duration_ms,
            finding_count: r.finding_count,
        }
    }
}

impl From<agent_core::AnalysisReport> for pb::ReviewAnalysisReport {
    fn from(a: agent_core::AnalysisReport) -> Self {
        pb::ReviewAnalysisReport {
            language: a.language,
            runs: a.runs.into_iter().map(Into::into).collect(),
            findings: a.findings.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<pb::ReviewAnalysisReport> for agent_core::AnalysisReport {
    fn from(a: pb::ReviewAnalysisReport) -> Self {
        agent_core::AnalysisReport {
            language: a.language,
            runs: a.runs.into_iter().map(Into::into).collect(),
            findings: a.findings.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<agent_core::SignatureChange> for pb::ReviewSignatureChange {
    fn from(c: agent_core::SignatureChange) -> Self {
        pb::ReviewSignatureChange {
            file: c.file,
            lang: c.lang,
            kind: c.kind,
            name: c.name,
            before: c.before,
            after: c.after,
        }
    }
}
impl From<pb::ReviewSignatureChange> for agent_core::SignatureChange {
    fn from(c: pb::ReviewSignatureChange) -> Self {
        agent_core::SignatureChange {
            file: c.file,
            lang: c.lang,
            kind: c.kind,
            name: c.name,
            before: c.before,
            after: c.after,
        }
    }
}

impl From<agent_core::SignatureReport> for pb::ReviewSignatureReport {
    fn from(r: agent_core::SignatureReport) -> Self {
        pb::ReviewSignatureReport {
            changes: r.changes.into_iter().map(Into::into).collect(),
            files_scanned: r.files_scanned,
            truncated: r.truncated,
        }
    }
}
impl From<pb::ReviewSignatureReport> for agent_core::SignatureReport {
    fn from(r: pb::ReviewSignatureReport) -> Self {
        agent_core::SignatureReport {
            changes: r.changes.into_iter().map(Into::into).collect(),
            files_scanned: r.files_scanned,
            truncated: r.truncated,
        }
    }
}

impl From<agent_core::CallGraphNode> for pb::ReviewCallGraphNode {
    fn from(n: agent_core::CallGraphNode) -> Self {
        pb::ReviewCallGraphNode {
            id: n.id,
            package: n.package,
            name: n.name,
            exported: n.exported,
            file: n.file,
            line: n.line,
            centrality: n.centrality,
        }
    }
}
impl From<pb::ReviewCallGraphNode> for agent_core::CallGraphNode {
    fn from(n: pb::ReviewCallGraphNode) -> Self {
        agent_core::CallGraphNode {
            id: n.id,
            package: n.package,
            name: n.name,
            exported: n.exported,
            file: n.file,
            line: n.line,
            centrality: n.centrality,
        }
    }
}

impl From<agent_core::CallEdge> for pb::ReviewCallEdge {
    fn from(e: agent_core::CallEdge) -> Self {
        pb::ReviewCallEdge {
            caller_id: e.caller_id,
            callee_id: e.callee_id,
        }
    }
}
impl From<pb::ReviewCallEdge> for agent_core::CallEdge {
    fn from(e: pb::ReviewCallEdge) -> Self {
        agent_core::CallEdge {
            caller_id: e.caller_id,
            callee_id: e.callee_id,
        }
    }
}

impl From<agent_core::PackageShape> for pb::ReviewPackageShape {
    fn from(p: agent_core::PackageShape) -> Self {
        pb::ReviewPackageShape {
            package: p.package,
            files: p.files,
            exported_fns: p.exported_fns,
            types: p.types,
        }
    }
}
impl From<pb::ReviewPackageShape> for agent_core::PackageShape {
    fn from(p: pb::ReviewPackageShape) -> Self {
        agent_core::PackageShape {
            package: p.package,
            files: p.files,
            exported_fns: p.exported_fns,
            types: p.types,
        }
    }
}

impl From<agent_core::CallGraph> for pb::ReviewCallGraph {
    fn from(g: agent_core::CallGraph) -> Self {
        pb::ReviewCallGraph {
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            changed_fns: g.changed_fns,
            packages: g.packages.into_iter().map(Into::into).collect(),
            truncated: g.truncated,
        }
    }
}
impl From<pb::ReviewCallGraph> for agent_core::CallGraph {
    fn from(g: pb::ReviewCallGraph) -> Self {
        agent_core::CallGraph {
            nodes: g.nodes.into_iter().map(Into::into).collect(),
            edges: g.edges.into_iter().map(Into::into).collect(),
            changed_fns: g.changed_fns,
            packages: g.packages.into_iter().map(Into::into).collect(),
            truncated: g.truncated,
        }
    }
}

impl From<agent_core::NamingFacts> for pb::ReviewNamingFacts {
    fn from(n: agent_core::NamingFacts) -> Self {
        pb::ReviewNamingFacts {
            functions: n.functions,
            variables: n.variables,
            constants: n.constants,
            exported_ratio: n.exported_ratio,
        }
    }
}
impl From<pb::ReviewNamingFacts> for agent_core::NamingFacts {
    fn from(n: pb::ReviewNamingFacts) -> Self {
        agent_core::NamingFacts {
            functions: n.functions,
            variables: n.variables,
            constants: n.constants,
            exported_ratio: n.exported_ratio,
        }
    }
}

impl From<agent_core::CommitStyleFacts> for pb::ReviewCommitStyleFacts {
    fn from(c: agent_core::CommitStyleFacts) -> Self {
        pb::ReviewCommitStyleFacts {
            conventional_ratio: c.conventional_ratio,
            subject_len_p50: c.subject_len_p50,
            subject_len_p95: c.subject_len_p95,
            body_present_ratio: c.body_present_ratio,
            sampled_commits: c.sampled_commits,
        }
    }
}
impl From<pb::ReviewCommitStyleFacts> for agent_core::CommitStyleFacts {
    fn from(c: pb::ReviewCommitStyleFacts) -> Self {
        agent_core::CommitStyleFacts {
            conventional_ratio: c.conventional_ratio,
            subject_len_p50: c.subject_len_p50,
            subject_len_p95: c.subject_len_p95,
            body_present_ratio: c.body_present_ratio,
            sampled_commits: c.sampled_commits,
        }
    }
}

impl From<agent_core::StyleFacts> for pb::ReviewStyleFacts {
    fn from(s: agent_core::StyleFacts) -> Self {
        pb::ReviewStyleFacts {
            comment_density: s.comment_density,
            doccomment_ratio: s.doccomment_ratio,
            indent_tabs: s.indent_tabs,
            line_len_p95: s.line_len_p95,
            fn_len_median: s.fn_len_median,
            naming: Some(s.naming.into()),
            commits: Some(s.commits.into()),
            diff_matches_style: s.diff_matches_style,
            files_scanned: s.files_scanned,
        }
    }
}
impl From<pb::ReviewStyleFacts> for agent_core::StyleFacts {
    fn from(s: pb::ReviewStyleFacts) -> Self {
        agent_core::StyleFacts {
            comment_density: s.comment_density,
            doccomment_ratio: s.doccomment_ratio,
            indent_tabs: s.indent_tabs,
            line_len_p95: s.line_len_p95,
            fn_len_median: s.fn_len_median,
            naming: s.naming.map(Into::into).unwrap_or_default(),
            commits: s.commits.map(Into::into).unwrap_or_default(),
            diff_matches_style: s.diff_matches_style,
            files_scanned: s.files_scanned,
        }
    }
}

impl From<agent_core::FunctionSummary> for pb::ReviewFunctionSummary {
    fn from(s: agent_core::FunctionSummary) -> Self {
        pb::ReviewFunctionSummary {
            name: s.name,
            file: s.file,
            kind: s.kind,
            summary: s.summary,
            model: s.model,
            duration_ms: s.duration_ms,
        }
    }
}
impl From<pb::ReviewFunctionSummary> for agent_core::FunctionSummary {
    fn from(s: pb::ReviewFunctionSummary) -> Self {
        agent_core::FunctionSummary {
            name: s.name,
            file: s.file,
            kind: s.kind,
            summary: s.summary,
            model: s.model,
            duration_ms: s.duration_ms,
        }
    }
}

impl From<agent_core::SummaryReport> for pb::ReviewSummaryReport {
    fn from(r: agent_core::SummaryReport) -> Self {
        pb::ReviewSummaryReport {
            summaries: r.summaries.into_iter().map(Into::into).collect(),
            requested: r.requested,
            produced: r.produced,
            omitted: r.omitted,
        }
    }
}
impl From<pb::ReviewSummaryReport> for agent_core::SummaryReport {
    fn from(r: pb::ReviewSummaryReport) -> Self {
        agent_core::SummaryReport {
            summaries: r.summaries.into_iter().map(Into::into).collect(),
            requested: r.requested,
            produced: r.produced,
            omitted: r.omitted,
        }
    }
}

impl From<agent_core::CoChangePartner> for pb::ReviewCoChangePartner {
    fn from(p: agent_core::CoChangePartner) -> Self {
        pb::ReviewCoChangePartner {
            path: p.path,
            confidence: p.confidence,
            co_occurrences: p.co_occurrences,
            in_diff: p.in_diff,
        }
    }
}
impl From<pb::ReviewCoChangePartner> for agent_core::CoChangePartner {
    fn from(p: pb::ReviewCoChangePartner) -> Self {
        agent_core::CoChangePartner {
            path: p.path,
            confidence: p.confidence,
            co_occurrences: p.co_occurrences,
            in_diff: p.in_diff,
        }
    }
}
impl From<agent_core::CoChangeEntry> for pb::ReviewCoChangeEntry {
    fn from(e: agent_core::CoChangeEntry) -> Self {
        pb::ReviewCoChangeEntry {
            path: e.path,
            partners: e.partners.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<pb::ReviewCoChangeEntry> for agent_core::CoChangeEntry {
    fn from(e: pb::ReviewCoChangeEntry) -> Self {
        agent_core::CoChangeEntry {
            path: e.path,
            partners: e.partners.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<agent_core::CoChangeReport> for pb::ReviewCoChangeReport {
    fn from(r: agent_core::CoChangeReport) -> Self {
        pb::ReviewCoChangeReport {
            commits_scanned: r.commits_scanned,
            truncated: r.truncated,
            entries: r.entries.into_iter().map(Into::into).collect(),
            missing_partners: r.missing_partners,
        }
    }
}
impl From<pb::ReviewCoChangeReport> for agent_core::CoChangeReport {
    fn from(r: pb::ReviewCoChangeReport) -> Self {
        agent_core::CoChangeReport {
            commits_scanned: r.commits_scanned,
            truncated: r.truncated,
            entries: r.entries.into_iter().map(Into::into).collect(),
            missing_partners: r.missing_partners,
        }
    }
}

impl From<agent_core::FileChurn> for pb::ReviewFileChurn {
    fn from(c: agent_core::FileChurn) -> Self {
        pb::ReviewFileChurn {
            path: c.path,
            commits: c.commits,
            unique_authors: c.unique_authors,
            bus_factor: c.bus_factor,
            top_author_share: c.top_author_share,
            churn_trend: c.churn_trend,
            churn_slope: c.churn_slope,
            total_churn: c.total_churn,
        }
    }
}
impl From<pb::ReviewFileChurn> for agent_core::FileChurn {
    fn from(c: pb::ReviewFileChurn) -> Self {
        agent_core::FileChurn {
            path: c.path,
            commits: c.commits,
            unique_authors: c.unique_authors,
            bus_factor: c.bus_factor,
            top_author_share: c.top_author_share,
            churn_trend: c.churn_trend,
            churn_slope: c.churn_slope,
            total_churn: c.total_churn,
        }
    }
}
impl From<agent_core::ChurnReport> for pb::ReviewChurnReport {
    fn from(r: agent_core::ChurnReport) -> Self {
        pb::ReviewChurnReport {
            commits_scanned: r.commits_scanned,
            files: r.files.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<pb::ReviewChurnReport> for agent_core::ChurnReport {
    fn from(r: pb::ReviewChurnReport) -> Self {
        agent_core::ChurnReport {
            commits_scanned: r.commits_scanned,
            files: r.files.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<agent_core::FileSalience> for pb::ReviewFileSalience {
    fn from(s: agent_core::FileSalience) -> Self {
        pb::ReviewFileSalience {
            file: s.file,
            centrality: s.centrality,
            bus_factor: s.bus_factor,
            churn_increasing: s.churn_increasing,
            class: s.class,
        }
    }
}
impl From<pb::ReviewFileSalience> for agent_core::FileSalience {
    fn from(s: pb::ReviewFileSalience) -> Self {
        agent_core::FileSalience {
            file: s.file,
            centrality: s.centrality,
            bus_factor: s.bus_factor,
            churn_increasing: s.churn_increasing,
            class: s.class,
        }
    }
}
impl From<agent_core::SalienceReport> for pb::ReviewSalienceReport {
    fn from(r: agent_core::SalienceReport) -> Self {
        pb::ReviewSalienceReport {
            files: r.files.into_iter().map(Into::into).collect(),
        }
    }
}
impl From<pb::ReviewSalienceReport> for agent_core::SalienceReport {
    fn from(r: pb::ReviewSalienceReport) -> Self {
        agent_core::SalienceReport {
            files: r.files.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<agent_core::ReviewFacts> for pb::ReviewFacts {
    fn from(f: agent_core::ReviewFacts) -> Self {
        pb::ReviewFacts {
            meta: Some(f.meta.into()),
            change: Some(f.change.into()),
            git_state: Some(f.git_state.into()),
            analysis: Some(f.analysis.into()),
            signatures: Some(f.signatures.into()),
            callgraph: Some(f.callgraph.into()),
            style: Some(f.style.into()),
            summaries: Some(f.summaries.into()),
            cochange: Some(f.cochange.into()),
            churn: Some(f.churn.into()),
            salience: Some(f.salience.into()),
        }
    }
}
impl From<pb::ReviewFacts> for agent_core::ReviewFacts {
    fn from(f: pb::ReviewFacts) -> Self {
        agent_core::ReviewFacts {
            meta: f.meta.map(Into::into).unwrap_or_default(),
            change: f.change.map(Into::into).unwrap_or_default(),
            git_state: f.git_state.map(Into::into).unwrap_or_default(),
            analysis: f.analysis.map(Into::into).unwrap_or_default(),
            signatures: f.signatures.map(Into::into).unwrap_or_default(),
            callgraph: f.callgraph.map(Into::into).unwrap_or_default(),
            style: f.style.map(Into::into).unwrap_or_default(),
            summaries: f.summaries.map(Into::into).unwrap_or_default(),
            cochange: f.cochange.map(Into::into).unwrap_or_default(),
            churn: f.churn.map(Into::into).unwrap_or_default(),
            salience: f.salience.map(Into::into).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn msg_with_calls() -> agent_core::Message {
        agent_core::Message {
            role: agent_core::Role::Assistant,
            content: vec![agent_core::ContentBlock::text("hi")],
            tool_calls: vec![agent_core::ToolCall {
                id: "c1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"cmd": "ls", "n": [1, 2, {"k": true}], "x": null}),
            }],
            tool_call_id: None,
        }
    }

    // Round-trip `core -> proto -> core -> proto` and assert the two proto
    // encodings match. prost messages derive `PartialEq`, so this validates the
    // whole loop without needing `PartialEq` on the (mostly underived) core types.

    #[test]
    fn message_roundtrip_preserves_calls_and_json() {
        let core = msg_with_calls();
        let p1 = pb::Message::from(core);
        let back = agent_core::Message::try_from(p1.clone()).unwrap();
        // JSON args survive verbatim (serde_json::Value is PartialEq).
        assert_eq!(
            back.tool_calls[0].arguments,
            msg_with_calls().tool_calls[0].arguments
        );
        let p2 = pb::Message::from(back);
        assert_eq!(p1, p2);
    }

    #[test]
    fn tool_message_roundtrip_keeps_tool_call_id() {
        let core = agent_core::Message::tool("call-9", "output");
        let p1 = pb::Message::from(core);
        assert_eq!(p1.tool_call_id.as_deref(), Some("call-9"));
        let back = agent_core::Message::try_from(p1.clone()).unwrap();
        assert_eq!(pb::Message::from(back), p1);
    }

    #[rstest]
    #[case(agent_core::Role::System, pb::Role::System)]
    #[case(agent_core::Role::User, pb::Role::User)]
    #[case(agent_core::Role::Assistant, pb::Role::Assistant)]
    #[case(agent_core::Role::Tool, pb::Role::Tool)]
    fn role_roundtrip(#[case] core: agent_core::Role, #[case] wire: pb::Role) {
        assert_eq!(pb::Role::from(core), wire);
        assert_eq!(role_from_i32(wire as i32).unwrap(), core);
    }

    #[test]
    fn unspecified_role_is_rejected() {
        assert!(matches!(
            role_from_i32(pb::Role::Unspecified as i32),
            Err(ConvertError::UnknownRole(0))
        ));
        assert!(matches!(
            role_from_i32(999),
            Err(ConvertError::UnknownRole(999))
        ));
    }

    #[test]
    fn absent_arguments_decodes_to_null() {
        // A default-constructed ToolCall (no `arguments` set) is JSON null, not an
        // error — the binary JsonValue simply has an unset `kind`.
        let pc = pb::ToolCall {
            id: "1".into(),
            name: "t".into(),
            arguments: None,
        };
        let core = agent_core::ToolCall::try_from(pc).unwrap();
        assert_eq!(core.arguments, serde_json::Value::Null);
    }

    #[test]
    fn large_integers_survive_without_precision_loss() {
        // The whole point of the custom JsonValue over google.protobuf.Struct: a
        // u64 above 2^53 (unrepresentable as f64) round-trips exactly.
        let big: u64 = 9_007_199_254_740_993; // 2^53 + 1
        let neg: i64 = i64::MIN;
        let args = serde_json::json!({
            "big_u64": big,
            "i64_min": neg,
            "float": 0.1_f64,
            "nested": [big, {"k": neg}],
        });
        let core = agent_core::ToolCall {
            id: "1".into(),
            name: "t".into(),
            arguments: args.clone(),
        };
        let back = agent_core::ToolCall::try_from(pb::ToolCall::from(core)).unwrap();
        assert_eq!(back.arguments, args);
        // …and confirm the numbers kept their integer identity (not coerced to f64).
        assert_eq!(back.arguments["big_u64"].as_u64(), Some(big));
        assert_eq!(back.arguments["i64_min"].as_i64(), Some(neg));
    }

    #[rstest]
    #[case(agent_core::Decision::Allow)]
    #[case(agent_core::Decision::Deny("nope".into()))]
    fn decision_roundtrip(#[case] core: agent_core::Decision) {
        let wire = pb::Decision::from(core.clone());
        assert_eq!(agent_core::Decision::from(wire), core);
    }

    #[rstest]
    #[case(None, None)]
    #[case(Some(agent_core::Usage { prompt_tokens: 3, completion_tokens: 5, total_tokens: 8, ..Default::default() }), Some(7))]
    fn memory_event_roundtrip(#[case] usage: Option<agent_core::Usage>, #[case] iter: Option<u32>) {
        let core = agent_core::MemoryEvent {
            kind: "assistant".into(),
            message: msg_with_calls(),
            ts_ms: 1_700_000_000_000,
            session_id: "sess".into(),
            usage,
            iter,
            verification: None,
            review: None,
        };
        let p1 = pb::MemoryEvent::from(core);
        let back = agent_core::MemoryEvent::try_from(p1.clone()).unwrap();
        assert_eq!(pb::MemoryEvent::from(back), p1);
    }

    #[test]
    fn completion_response_missing_message_errors() {
        let pr = pb::CompletionResponse {
            message: None,
            finish_reason: "stop".into(),
            usage: None,
        };
        assert!(matches!(
            agent_core::CompletionResponse::try_from(pr),
            Err(ConvertError::MissingField("CompletionResponse.message"))
        ));
    }

    #[test]
    fn completion_request_roundtrip() {
        let core = agent_core::CompletionRequest {
            messages: vec![agent_core::Message::user("go"), msg_with_calls()],
            tools: vec![agent_core::ToolSchema {
                name: "bash".into(),
                description: "run".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            max_tokens: 256,
            temperature: 0.5,
            response_format: None,
        };
        let p1 = pb::CompletionRequest::from(core);
        let back = agent_core::CompletionRequest::try_from(p1.clone()).unwrap();
        assert_eq!(pb::CompletionRequest::from(back), p1);
    }

    #[test]
    fn completion_chunk_roundtrip() {
        let core = agent_core::CompletionChunk {
            delta_text: "he".into(),
            tool_call: Some(msg_with_calls().tool_calls.remove(0)),
            finish_reason: Some("tool_use".into()),
            usage: Some(agent_core::Usage::default()),
        };
        let p1 = pb::CompletionChunk::from(core);
        let back = agent_core::CompletionChunk::try_from(p1.clone()).unwrap();
        assert_eq!(pb::CompletionChunk::from(back), p1);
    }

    #[test]
    fn context_input_and_working_set_roundtrip() {
        let ci = agent_core::ContextInput {
            system_prompt: "sys".into(),
            prepend: vec![agent_core::ContextBlock {
                source: "p".into(),
                content: "a".into(),
            }],
            recalled: vec![agent_core::MemoryItem {
                source: "m".into(),
                content: "fact".into(),
            }],
            goal: "do it".into(),
            append: vec![],
        };
        let p1 = pb::ContextInput::from(ci);
        assert_eq!(
            pb::ContextInput::from(agent_core::ContextInput::from(p1.clone())),
            p1
        );

        let ws = agent_core::WorkingSet {
            messages: vec![msg_with_calls()],
        };
        let pw = pb::WorkingSet::from(ws);
        let back = agent_core::WorkingSet::try_from(pw.clone()).unwrap();
        assert_eq!(pb::WorkingSet::from(back), pw);
    }

    #[test]
    fn scalar_types_roundtrip() {
        let caps = agent_core::ModelCapabilities {
            supports_tools: true,
            context_window: 128_000,
            supports_response_format: false,
            supports_vision: true,
        };
        let pc = pb::ModelCapabilities::from(caps);
        assert_eq!(
            pb::ModelCapabilities::from(agent_core::ModelCapabilities::from(pc)),
            pc
        );

        let obs = agent_core::Observation::error("boom");
        let po = pb::Observation::from(obs);
        assert_eq!(
            pb::Observation::from(
                agent_core::Observation::try_from(po.clone()).expect("observation converts")
            ),
            po
        );

        let tc = agent_core::ToolContext {
            cwd: std::path::PathBuf::from("/tmp/work"),
        };
        let ptc = pb::ToolContext::from(&tc);
        assert_eq!(agent_core::ToolContext::from(ptc.clone()).cwd, tc.cwd);

        let q = agent_core::RecallQuery {
            text: "q".into(),
            limit: 42,
        };
        let pq = pb::RecallQuery::from(q);
        assert_eq!(agent_core::RecallQuery::from(pq.clone()).limit, 42);

        let b = agent_core::TokenBudget {
            max_context_tokens: 8000,
            reserve_output: 1000,
        };
        let pbud = pb::TokenBudget::from(b);
        assert_eq!(
            pb::TokenBudget::from(agent_core::TokenBudget::from(pbud)),
            pbud
        );
    }

    #[rstest]
    #[case(agent_core::SearchMode::Literal, pb::SearchMode::Literal)]
    #[case(agent_core::SearchMode::Phrase, pb::SearchMode::Phrase)]
    #[case(agent_core::SearchMode::Fuzzy, pb::SearchMode::Fuzzy)]
    #[case(agent_core::SearchMode::Regex, pb::SearchMode::Regex)]
    fn search_mode_roundtrip(#[case] core: agent_core::SearchMode, #[case] wire: pb::SearchMode) {
        assert_eq!(pb::SearchMode::from(core), wire);
        assert_eq!(search_mode_from_i32(wire as i32), core);
    }

    #[test]
    fn unknown_search_mode_defaults_to_literal() {
        assert_eq!(search_mode_from_i32(999), agent_core::SearchMode::Literal);
    }

    #[test]
    fn search_query_roundtrip() {
        let core = agent_core::SearchQuery {
            text: "fn main".into(),
            mode: agent_core::SearchMode::Phrase,
            path_globs: vec!["**/*.rs".into()],
            lang: Some("rust".into()),
            limit: 25,
            fuzzy_distance: Some(2),
        };
        let p1 = pb::SearchQuery::from(core);
        let back = agent_core::SearchQuery::from(p1.clone());
        assert_eq!(pb::SearchQuery::from(back), p1);
    }

    #[test]
    fn search_hit_and_status_roundtrip() {
        let hit = agent_core::SearchHit {
            path: std::path::PathBuf::from("src/main.rs"),
            line: 12,
            col_start: 0,
            col_end: 0,
            score: 1.5,
            snippet: "fn main()".into(),
        };
        let p = pb::SearchHit::from(hit);
        assert_eq!(agent_core::SearchHit::from(p.clone()).line, 12);

        let status = agent_core::IndexStatus {
            state: agent_core::IndexState::Stale,
            indexed_files: 42,
            last_indexed_ms: 1000,
            manifest_digest: "abcd".into(),
        };
        let ps = pb::IndexStatus::from(status);
        let back = agent_core::IndexStatus::from(ps);
        assert_eq!(back.state, agent_core::IndexState::Stale);
        assert_eq!(back.indexed_files, 42);
    }

    #[test]
    fn search_capabilities_roundtrip() {
        let caps = agent_core::SearchCapabilities {
            backend: "tantivy".into(),
            modes: vec![
                agent_core::SearchMode::Literal,
                agent_core::SearchMode::Regex,
            ],
            content_search: true,
            scored: true,
            incremental: true,
            max_concurrent_queries: 0,
        };
        let p1 = pb::SearchCapabilities::from(caps);
        let back = agent_core::SearchCapabilities::from(p1.clone());
        assert_eq!(pb::SearchCapabilities::from(back), p1);
    }

    #[test]
    fn error_maps_to_status_codes() {
        assert_eq!(
            status_from_error(&agent_core::Error::Config("bad".into())).code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            status_from_error(&agent_core::Error::Provider("x".into())).code(),
            tonic::Code::Internal
        );
    }
}
