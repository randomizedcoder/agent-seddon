//! Textproto (the file form) and proto3 JSON (the portal's asset form) for
//! `agent.v1.Theme`, via `prost-reflect` over the descriptor set `agent-proto`
//! already emits — the same approach as `agent-graph`'s cognition graphs.

use agent_proto::pb;
use prost::Message;
use prost_reflect::text_format::FormatOptions;
use prost_reflect::{DescriptorPool, DynamicMessage, MapKey, MessageDescriptor, Value};
use serde_json::{Map, Value as Json};
use std::sync::OnceLock;

use crate::{ThemeError, ThemeErrorKind, MAX_THEME_BYTES};

fn theme_descriptor() -> MessageDescriptor {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(agent_proto::FILE_DESCRIPTOR_SET)
            .expect("emitted descriptor set decodes")
    })
    .get_message_by_name("agent.v1.Theme")
    .expect("Theme is compiled into the descriptor set")
}

/// Parse a textproto theme. Size-capped before parsing. This proves shape only:
/// call [`crate::validate`] before trusting the result.
///
/// Unknown fields are a parse error (the textproto parser is strict), so a typo
/// in a field name is reported rather than silently ignored.
pub fn parse(text: &str) -> Result<pb::Theme, ThemeError> {
    if text.len() > MAX_THEME_BYTES {
        return Err(ThemeError::new(
            ThemeErrorKind::TooLarge,
            format!(
                "{} bytes, cap {MAX_THEME_BYTES}; refusing to parse",
                text.len()
            ),
        ));
    }
    let msg = DynamicMessage::parse_text_format(theme_descriptor(), text)
        .map_err(|e| ThemeError::new(ThemeErrorKind::Parse, format!("textproto: {e}")))?;
    msg.transcode_to()
        .map_err(|e| ThemeError::new(ThemeErrorKind::Parse, format!("transcode: {e}")))
}

/// Print a theme as pretty textproto, the form [`parse`] reads back. Capped like
/// [`parse`], so an export is always importable.
pub fn print(theme: &pb::Theme) -> Result<String, ThemeError> {
    let len = theme.encoded_len();
    if len > MAX_THEME_BYTES {
        return Err(ThemeError::new(
            ThemeErrorKind::TooLarge,
            format!("{len} bytes, cap {MAX_THEME_BYTES}; refusing to print"),
        ));
    }
    let mut msg = DynamicMessage::new(theme_descriptor());
    msg.transcode_from(theme)
        .expect("a pb::Theme transcodes to its own descriptor");
    let text = msg.to_text_format_with_options(&FormatOptions::new().pretty(true));
    if text.len() > MAX_THEME_BYTES {
        return Err(ThemeError::new(
            ThemeErrorKind::TooLarge,
            format!("{} bytes of text, cap {MAX_THEME_BYTES}", text.len()),
        ));
    }
    Ok(text)
}

/// The proto3 JSON form of a theme (lowerCamelCase names, enums by name, unset
/// fields omitted), as the portal reads it with `Theme.mergeFromProto3Json`.
pub fn to_json(theme: &pb::Theme) -> Json {
    let mut msg = DynamicMessage::new(theme_descriptor());
    msg.transcode_from(theme)
        .expect("a pb::Theme transcodes to its own descriptor");
    message_json(&msg)
}

fn message_json(msg: &DynamicMessage) -> Json {
    let mut out = Map::new();
    for (field, value) in msg.fields() {
        let json = match value {
            Value::Map(m) => {
                // Sorted, so the asset files are stable across runs.
                let mut entries: Vec<_> = m.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                Json::Object(
                    entries
                        .into_iter()
                        .map(|(k, v)| (map_key(k), scalar_json(v, &field.kind())))
                        .collect(),
                )
            }
            Value::List(items) => Json::Array(
                items
                    .iter()
                    .map(|v| scalar_json(v, &field.kind()))
                    .collect(),
            ),
            v => scalar_json(v, &field.kind()),
        };
        out.insert(field.json_name().to_owned(), json);
    }
    Json::Object(out)
}

fn map_key(k: &MapKey) -> String {
    match k {
        MapKey::String(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn scalar_json(v: &Value, kind: &prost_reflect::Kind) -> Json {
    match v {
        Value::Bool(b) => Json::Bool(*b),
        Value::I32(n) => Json::from(*n),
        Value::U32(n) => Json::from(*n),
        // proto3 JSON writes 64-bit integers as strings.
        Value::I64(n) => Json::String(n.to_string()),
        Value::U64(n) => Json::String(n.to_string()),
        Value::F32(f) => Json::from(f64::from(*f)),
        Value::F64(f) => Json::from(*f),
        Value::String(s) => Json::String(s.clone()),
        Value::Bytes(_) => unreachable!("Theme has no bytes fields"),
        Value::EnumNumber(n) => match kind {
            prost_reflect::Kind::Enum(e) => e
                .get_value(*n)
                .map_or_else(|| Json::from(*n), |ev| Json::String(ev.name().to_owned())),
            _ => Json::from(*n),
        },
        Value::Message(m) => message_json(m),
        Value::List(_) | Value::Map(_) => {
            unreachable!("nested repeated/map values are not in Theme")
        }
    }
}
