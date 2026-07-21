//! Pure parsers from raw LSP JSON values into the seam's typed structs. The
//! diagnostics parse runs on every `publishDiagnostics` after an edit — the
//! deterministic per-edit CPU hot path the iai bench guards.

use agent_core::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, Hover, Location, Position, Range, TextEdit,
    WorkspaceEdit,
};
use serde_json::Value;

pub fn position(v: &Value) -> Position {
    Position {
        line: v.get("line").and_then(Value::as_u64).unwrap_or(0) as u32,
        character: v.get("character").and_then(Value::as_u64).unwrap_or(0) as u32,
    }
}

pub fn range(v: &Value) -> Range {
    Range {
        start: v.get("start").map(position).unwrap_or_default(),
        end: v.get("end").map(position).unwrap_or_default(),
    }
}

/// Parse diagnostics from a `publishDiagnostics` params object (`{uri, diagnostics}`),
/// a pull `{items|diagnostics}` result, or a bare array.
pub fn diagnostics(v: &Value) -> Vec<Diagnostic> {
    let arr = v
        .get("diagnostics")
        .or_else(|| v.get("items"))
        .and_then(Value::as_array)
        .or_else(|| v.as_array());
    arr.map(|items| items.iter().map(diagnostic).collect())
        .unwrap_or_default()
}

fn diagnostic(v: &Value) -> Diagnostic {
    Diagnostic {
        range: v.get("range").map(range).unwrap_or_default(),
        severity: v
            .get("severity")
            .and_then(Value::as_u64)
            .map(DiagnosticSeverity::from_lsp)
            .unwrap_or(DiagnosticSeverity::Error),
        message: v
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        code: v.get("code").and_then(code_string),
        source: v.get("source").and_then(Value::as_str).map(str::to_string),
    }
}

/// A diagnostic `code` may be a string or a number.
fn code_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse a `definition`/`references` result: `Location`, `Location[]`, or
/// `LocationLink[]` (which carries `targetUri`/`targetRange`).
pub fn locations(v: &Value) -> Vec<Location> {
    match v {
        Value::Array(items) => items.iter().filter_map(location).collect(),
        Value::Object(_) => location(v).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn location(v: &Value) -> Option<Location> {
    if let Some(uri) = v.get("uri").and_then(Value::as_str) {
        return Some(Location {
            uri: uri.to_string(),
            range: v.get("range").map(range).unwrap_or_default(),
        });
    }
    // LocationLink
    if let Some(uri) = v.get("targetUri").and_then(Value::as_str) {
        return Some(Location {
            uri: uri.to_string(),
            range: v
                .get("targetSelectionRange")
                .or_else(|| v.get("targetRange"))
                .map(range)
                .unwrap_or_default(),
        });
    }
    None
}

/// Parse a `hover` result: `contents` is a string, a `{kind,value}` MarkupContent,
/// a `{language,value}` MarkedString, or an array of those.
pub fn hover(v: &Value) -> Option<Hover> {
    if v.is_null() {
        return None;
    }
    let contents = v.get("contents")?;
    let text = markup_text(contents);
    if text.trim().is_empty() {
        None
    } else {
        Some(Hover { contents: text })
    }
}

fn markup_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(_) => v
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        Value::Array(items) => items.iter().map(markup_text).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Parse a `documentSymbol` result: `DocumentSymbol[]` (hierarchical, flattened)
/// or `SymbolInformation[]` (flat, has `location`).
pub fn symbols(v: &Value) -> Vec<DocumentSymbol> {
    let Some(items) = v.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        flatten_symbol(item, &mut out);
    }
    out
}

fn flatten_symbol(v: &Value, out: &mut Vec<DocumentSymbol>) {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let kind = v
        .get("kind")
        .and_then(Value::as_u64)
        .map(symbol_kind)
        .unwrap_or("unknown")
        .to_string();
    // DocumentSymbol has `range`; SymbolInformation has `location.range`.
    let range = v
        .get("range")
        .or_else(|| v.get("location").and_then(|l| l.get("range")))
        .map(range)
        .unwrap_or_default();
    out.push(DocumentSymbol { name, kind, range });
    if let Some(children) = v.get("children").and_then(Value::as_array) {
        for c in children {
            flatten_symbol(c, out);
        }
    }
}

/// LSP `SymbolKind` integer → a readable name (the ones that matter for code).
fn symbol_kind(n: u64) -> &'static str {
    match n {
        5 => "class",
        6 => "method",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        23 => "struct",
        _ => "symbol",
    }
}

/// Parse a `rename` result (`WorkspaceEdit`): `changes` (`{uri: TextEdit[]}`) or
/// `documentChanges` (`[{textDocument:{uri}, edits}]`).
pub fn workspace_edit(v: &Value) -> WorkspaceEdit {
    let mut changes = Vec::new();
    if let Some(map) = v.get("changes").and_then(Value::as_object) {
        for (uri, edits) in map {
            changes.push((uri.clone(), text_edits(edits)));
        }
    }
    if let Some(docs) = v.get("documentChanges").and_then(Value::as_array) {
        for d in docs {
            if let Some(uri) = d
                .get("textDocument")
                .and_then(|t| t.get("uri"))
                .and_then(Value::as_str)
            {
                changes.push((
                    uri.to_string(),
                    text_edits(d.get("edits").unwrap_or(&Value::Null)),
                ));
            }
        }
    }
    WorkspaceEdit { changes }
}

fn text_edits(v: &Value) -> Vec<TextEdit> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .map(|e| TextEdit {
                    range: e.get("range").map(range).unwrap_or_default(),
                    new_text: e
                        .get("newText")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Bench hook (dependency of `benches/lsp_parse.rs`): parse a publishDiagnostics
/// payload into the typed structs and report the count.
#[doc(hidden)]
pub fn bench_parse_diagnostics(params: &Value) -> usize {
    diagnostics(params).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diagnostics_from_publish_params() {
        let params = json!({
            "uri": "file:///x.rs",
            "diagnostics": [
                {"range": {"start": {"line": 41, "character": 8}, "end": {"line": 41, "character": 12}},
                 "severity": 1, "message": "E0308 expected u32, found String", "code": "E0308"},
                {"severity": 2, "message": "unused variable"}
            ]
        });
        let d = diagnostics(&params);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].severity, DiagnosticSeverity::Error);
        assert_eq!(d[0].code.as_deref(), Some("E0308"));
        assert_eq!(d[1].severity, DiagnosticSeverity::Warning);
    }

    #[test]
    fn locations_from_single_and_array_and_link() {
        let single = json!({"uri": "file:///a.rs", "range": {}});
        assert_eq!(locations(&single).len(), 1);
        let arr =
            json!([{"uri": "file:///a.rs", "range": {}}, {"uri": "file:///b.rs", "range": {}}]);
        assert_eq!(locations(&arr).len(), 2);
        let link = json!([{"targetUri": "file:///c.rs", "targetRange": {}}]);
        assert_eq!(locations(&link)[0].uri, "file:///c.rs");
    }

    #[test]
    fn hover_extracts_markup_text() {
        assert_eq!(
            hover(&json!({"contents": {"kind": "markdown", "value": "fn resolve_within"}}))
                .unwrap()
                .contents,
            "fn resolve_within"
        );
        assert!(hover(&json!({"contents": ""})).is_none());
        assert!(hover(&Value::Null).is_none());
    }

    #[test]
    fn symbols_flatten_hierarchy() {
        let v = json!([
            {"name": "EditTool", "kind": 23, "range": {},
             "children": [{"name": "execute", "kind": 6, "range": {}}]}
        ]);
        let s = symbols(&v);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "EditTool");
        assert_eq!(s[0].kind, "struct");
        assert_eq!(s[1].name, "execute");
    }

    #[test]
    fn workspace_edit_from_changes_and_document_changes() {
        let changes = json!({"changes": {
            "file:///a.rs": [{"range": {}, "newText": "x"}],
            "file:///b.rs": [{"range": {}, "newText": "y"}]}});
        assert_eq!(workspace_edit(&changes).changes.len(), 2);
        let docs = json!({"documentChanges": [
            {"textDocument": {"uri": "file:///a.rs"}, "edits": [{"range": {}, "newText": "z"}]}]});
        assert_eq!(workspace_edit(&docs).changes.len(), 1);
    }
}
