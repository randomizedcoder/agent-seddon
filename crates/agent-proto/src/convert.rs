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

// --- Message ---------------------------------------------------------------

impl From<agent_core::Message> for pb::Message {
    fn from(m: agent_core::Message) -> Self {
        pb::Message {
            role: pb::Role::from(m.role) as i32,
            content: m.content,
            tool_calls: m.tool_calls.into_iter().map(Into::into).collect(),
            tool_call_id: m.tool_call_id,
        }
    }
}

impl TryFrom<pb::Message> for agent_core::Message {
    type Error = ConvertError;
    fn try_from(m: pb::Message) -> Result<Self, Self::Error> {
        Ok(agent_core::Message {
            role: role_from_i32(m.role)?,
            content: m.content,
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
        }
    }
}

impl From<pb::Observation> for agent_core::Observation {
    fn from(o: pb::Observation) -> Self {
        agent_core::Observation {
            content: o.content,
            is_error: o.is_error,
        }
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
        }
    }
}

impl From<pb::ModelCapabilities> for agent_core::ModelCapabilities {
    fn from(c: pb::ModelCapabilities) -> Self {
        agent_core::ModelCapabilities {
            supports_tools: c.supports_tools,
            context_window: c.context_window,
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
        agent_core::Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn msg_with_calls() -> agent_core::Message {
        agent_core::Message {
            role: agent_core::Role::Assistant,
            content: "hi".into(),
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
    #[case(Some(agent_core::Usage { prompt_tokens: 3, completion_tokens: 5, total_tokens: 8 }), Some(7))]
    fn memory_event_roundtrip(#[case] usage: Option<agent_core::Usage>, #[case] iter: Option<u32>) {
        let core = agent_core::MemoryEvent {
            kind: "assistant".into(),
            message: msg_with_calls(),
            ts_ms: 1_700_000_000_000,
            session_id: "sess".into(),
            usage,
            iter,
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
        };
        let pc = pb::ModelCapabilities::from(caps);
        assert_eq!(
            pb::ModelCapabilities::from(agent_core::ModelCapabilities::from(pc)),
            pc
        );

        let obs = agent_core::Observation::error("boom");
        let po = pb::Observation::from(obs);
        assert_eq!(
            pb::Observation::from(agent_core::Observation::from(po.clone())),
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
