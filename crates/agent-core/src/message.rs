//! The shared message vocabulary — the *only* currency between seams: `Role`,
//! `ToolCall`, `ContentBlock`, `Message`, plus the binary base64 helpers and the
//! `ContentRepr` serde bridge. Extracted from `lib.rs` and re-exported at the crate
//! root, so `agent_core::{Message, ToolCall, ContentBlock, Role}` is unchanged. Treat
//! these as a deliberately stable API — extend additively (serde-defaulted fields).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Messages — the common currency between every seam.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// A single tool invocation requested by the model. Provider impls are
/// responsible for parsing their own on-the-wire format (native JSON,
/// XML-tagged, …) into this normalized shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// One typed piece of message content. A message is an ordered list of these, so
/// a turn can interleave prose with an image or a document (parity spec 26).
///
/// Serde is `tag = "type"`, matching the shape both vendors use on the wire and
/// the peers use internally (`{"type":"text","text":…}` /
/// `{"type":"image","media_type":…,"data":…}`). `data` is raw bytes, base64-encoded
/// by serde so a block round-trips through JSON losslessly; the provider adapters
/// re-encode into each vendor's own envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        media_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
    },
    Document {
        media_type: String,
        #[serde(with = "base64_bytes")]
        data: Vec<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

impl ContentBlock {
    /// A text block (the overwhelmingly common case).
    pub fn text(s: impl Into<String>) -> Self {
        ContentBlock::Text { text: s.into() }
    }
    /// An image block from raw (already-decoded) bytes.
    pub fn image(media_type: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        ContentBlock::Image {
            media_type: media_type.into(),
            data: data.into(),
        }
    }
    /// The block's text, if it is a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }
    /// `true` for anything a text-only model cannot accept.
    pub fn is_media(&self) -> bool {
        !matches!(self, ContentBlock::Text { .. })
    }
    /// The metric/span label for this block's modality.
    pub fn modality(&self) -> &'static str {
        match self {
            ContentBlock::Text { .. } => "text",
            ContentBlock::Image { .. } => "image",
            ContentBlock::Document { .. } => "document",
        }
    }
}

/// Base64 for the `data` field of a media block, so a `ContentBlock` survives a
/// JSON round-trip (session files, gRPC JSON, provider payloads) unchanged.
mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&super::b64_encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        super::b64_decode(&s).map_err(serde::de::Error::custom)
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding. Dependency-free (the crate carries no base64
/// dep, matching the other hand-rolled primitives in-tree) and used for every
/// media block's bytes on the JSON/provider path.
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decode standard base64, rejecting any non-alphabet byte. Input is untrusted
/// (a provider response or a session file), so this fails closed rather than
/// skipping junk.
pub fn b64_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    let mut acc: u32 = 0;
    let mut bits = 0u8;
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for c in s.bytes() {
        if c == b'=' || c == b'\n' || c == b'\r' {
            continue;
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("invalid base64 character: {:?}", c as char)),
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// Message content: a bare string (legacy / text-only) or an explicit block list.
///
/// This exists so `Message` deserializes **both** shapes — every session file,
/// config, and gRPC JSON payload written before spec 26 carries a bare string, and
/// must keep loading. Serialization always emits the block list.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ContentRepr {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl From<ContentRepr> for Vec<ContentBlock> {
    fn from(r: ContentRepr) -> Self {
        match r {
            // A bare string folds into exactly one text block; an empty string is
            // no content at all (the old `content: ""` default).
            ContentRepr::Text(s) if s.is_empty() => Vec::new(),
            ContentRepr::Text(s) => vec![ContentBlock::text(s)],
            ContentRepr::Blocks(b) => b,
        }
    }
}

fn deserialize_content<'de, D>(d: D) -> std::result::Result<Vec<ContentBlock>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(ContentRepr::deserialize(d)?.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// Ordered content blocks. Text-only messages hold a single
    /// [`ContentBlock::Text`]; use [`Message::content_text`] to read them as a
    /// string. Deserializes from a bare string too (pre-spec-26 data).
    #[serde(default, deserialize_with = "deserialize_content")]
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    fn new(role: Role, content: impl Into<String>, tool_call_id: Option<String>) -> Self {
        let s = content.into();
        Self {
            role,
            content: if s.is_empty() {
                Vec::new()
            } else {
                vec![ContentBlock::text(s)]
            },
            tool_calls: Vec::new(),
            tool_call_id,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content, None)
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content, None)
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content, None)
    }
    /// A tool-result message, linked back to the call that produced it.
    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::new(Role::Tool, content, Some(call_id.into()))
    }

    /// A message carrying explicit blocks (the multimodal constructor).
    pub fn with_blocks(role: Role, content: Vec<ContentBlock>) -> Self {
        Self {
            role,
            content,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// A tool result that carries typed blocks alongside its text summary.
    pub fn tool_with_blocks(call_id: impl Into<String>, content: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Tool,
            content,
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
        }
    }

    /// The message's text: every [`ContentBlock::Text`] concatenated. Media blocks
    /// contribute nothing, so a text-only consumer (token estimation, logging, the
    /// summarizer) reads exactly what it did before spec 26.
    pub fn content_text(&self) -> String {
        let mut out = String::new();
        for b in &self.content {
            if let Some(t) = b.as_text() {
                out.push_str(t);
            }
        }
        out
    }

    /// `true` when the message has no content blocks at all.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// `true` when any block is an image/document.
    pub fn has_media(&self) -> bool {
        self.content.iter().any(ContentBlock::is_media)
    }

    /// Drop every media block, replacing them with `note` (once) when any were
    /// dropped. Used to degrade a turn for a model without vision support rather
    /// than sending a block the provider would reject outright.
    pub fn strip_media(&mut self, note: &str) -> usize {
        let before = self.content.len();
        self.content.retain(|b| !b.is_media());
        let dropped = before - self.content.len();
        if dropped > 0 {
            self.content.push(ContentBlock::text(note));
        }
        dropped
    }
}
