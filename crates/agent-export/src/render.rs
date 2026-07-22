//! The three renderers: markdown, JSON, and a self-contained HTML page.
//!
//! All three are **pure functions of the transcript**. No wall clock, no random
//! ids, no map-iteration order reaches the output — the same session exports to
//! the same bytes every time. That is what makes the output diffable, cacheable,
//! golden-testable, and what makes the instruction-count bench meaningful.

use agent_core::{ContentBlock, Message, Role};

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Json,
    Html,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "md" | "markdown" => Format::Markdown,
            "json" => Format::Json,
            "html" => Format::Html,
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Json => "json",
            Format::Html => "html",
        }
    }
}

fn role_label(r: Role) -> &'static str {
    match r {
        Role::System => "System",
        Role::User => "User",
        Role::Assistant => "Assistant",
        Role::Tool => "Tool",
    }
}

/// Describe a message's content as text, naming media rather than dropping it.
fn body_text(m: &Message) -> String {
    let mut out = String::new();
    for b in &m.content {
        match b {
            ContentBlock::Text { text } => out.push_str(text),
            ContentBlock::Image { media_type, data } => {
                out.push_str(&format!("[image: {media_type}, {} bytes]", data.len()))
            }
            ContentBlock::Document {
                media_type,
                data,
                name,
            } => out.push_str(&format!(
                "[document: {}{media_type}, {} bytes]",
                name.as_deref()
                    .map(|n| format!("{n}, "))
                    .unwrap_or_default(),
                data.len()
            )),
        }
    }
    out
}

/// Render to markdown.
pub fn markdown(session_id: &str, messages: &[Message]) -> String {
    let mut out = String::with_capacity(messages.len() * 128);
    out.push_str(&format!("# Session `{session_id}`\n\n"));
    out.push_str(&format!("{} messages\n", messages.len()));
    for m in messages {
        out.push_str(&format!("\n## {}\n\n", role_label(m.role)));
        let body = body_text(m);
        if !body.is_empty() {
            out.push_str(&body);
            out.push('\n');
        }
        for tc in &m.tool_calls {
            // Sort keys so the emitted JSON is stable, not map-order dependent.
            out.push_str(&format!(
                "\n```json\n// tool_call: {}\n{}\n```\n",
                tc.name,
                stable_json(&tc.arguments)
            ));
        }
    }
    out
}

/// Render to JSON — the structured transcript, for tooling.
pub fn json(session_id: &str, messages: &[Message]) -> String {
    let doc = serde_json::json!({
        "session": session_id,
        "message_count": messages.len(),
        "messages": messages,
    });
    stable_json(&doc)
}

/// Serialize with **sorted keys**, so output does not depend on map iteration
/// order. `serde_json`'s default object is order-preserving for structs, but a
/// `Value` built from a map is not guaranteed — sorting makes it explicit.
fn stable_json(v: &serde_json::Value) -> String {
    let sorted = sort_value(v);
    serde_json::to_string_pretty(&sorted).unwrap_or_else(|_| "null".into())
}

fn sort_value(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::with_capacity(m.len());
            for k in keys {
                out.insert(k.clone(), sort_value(&m[k]));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(a) => serde_json::Value::Array(a.iter().map(sort_value).collect()),
        other => other.clone(),
    }
}

/// Escape text for HTML **text and attribute** contexts.
///
/// The transcript is fully attacker-influenced — a web page the agent fetched, a
/// file it read, a model completion — and the export is a document people open
/// in a browser. Everything interpolated is escaped; nothing is trusted.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            // C0 controls (except tab/newline/CR) can smuggle terminal escapes
            // and confuse parsers; drop them.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Render a self-contained HTML page: inline CSS, no external fetch.
pub fn html(session_id: &str, messages: &[Message]) -> String {
    let mut out = String::with_capacity(messages.len() * 256 + 2048);
    out.push_str(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n",
    );
    out.push_str(&format!(
        "<title>Session {}</title>\n",
        escape_html(session_id)
    ));
    out.push_str(STYLE);
    out.push_str("</head>\n<body>\n<main>\n");
    out.push_str(&format!(
        "<h1>Session <code>{}</code></h1>\n<p class=\"meta\">{} messages</p>\n",
        escape_html(session_id),
        messages.len()
    ));
    for m in messages {
        let role = role_label(m.role);
        out.push_str(&format!(
            "<section class=\"msg {}\">\n<h2>{}</h2>\n",
            role.to_ascii_lowercase(),
            escape_html(role)
        ));
        let body = body_text(m);
        if !body.is_empty() {
            out.push_str(&format!("<pre>{}</pre>\n", escape_html(&body)));
        }
        for tc in &m.tool_calls {
            out.push_str(&format!(
                "<details><summary>tool_call: {}</summary><pre>{}</pre></details>\n",
                escape_html(&tc.name),
                escape_html(&stable_json(&tc.arguments))
            ));
        }
        out.push_str("</section>\n");
    }
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

const STYLE: &str = "<style>\n\
body{font:16px/1.6 system-ui,sans-serif;margin:0;background:#fbfbfd;color:#1a1a1c}\n\
main{max-width:52rem;margin:0 auto;padding:2rem 1rem}\n\
h1{font-size:1.5rem}h2{font-size:.8rem;text-transform:uppercase;letter-spacing:.06em;color:#666;margin:0 0 .5rem}\n\
.meta{color:#666;font-size:.9rem}\n\
.msg{border:1px solid #e3e3e8;border-radius:.6rem;padding:1rem;margin:1rem 0;background:#fff}\n\
.msg.user{background:#f4f7ff}.msg.tool{background:#fafafa}\n\
pre{white-space:pre-wrap;word-wrap:break-word;margin:0;font:14px/1.55 ui-monospace,monospace}\n\
details{margin-top:.75rem}summary{cursor:pointer;color:#555;font-size:.85rem}\n\
@media(prefers-color-scheme:dark){body{background:#141416;color:#e8e8ea}\n\
.msg{background:#1c1c1f;border-color:#2c2c31}.msg.user{background:#1a1f2e}.msg.tool{background:#191919}\n\
h2,.meta,summary{color:#9a9aa2}}\n\
</style>\n";

/// Render `messages` in `format`.
pub fn render(format: Format, session_id: &str, messages: &[Message]) -> String {
    match format {
        Format::Markdown => markdown(session_id, messages),
        Format::Json => json(session_id, messages),
        Format::Html => html(session_id, messages),
    }
}

/// Bench hook: render all three formats (the deterministic CPU path).
#[doc(hidden)]
pub fn bench_render(messages: &[Message]) -> usize {
    markdown("s", messages).len() + json("s", messages).len() + html("s", messages).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn transcript() -> Vec<Message> {
        vec![
            Message::system("You are helpful."),
            Message::user("hello"),
            Message::assistant("hi there"),
        ]
    }

    /// The property everything else relies on: same input, same bytes.
    #[rstest]
    #[case::positive_markdown_is_stable(Format::Markdown)]
    #[case::positive_json_is_stable(Format::Json)]
    #[case::positive_html_is_stable(Format::Html)]
    fn render_is_byte_stable(#[case] f: Format) {
        let a = render(f, "sess-1", &transcript());
        let b = render(f, "sess-1", &transcript());
        assert_eq!(a, b, "{} render is not deterministic", f.as_str());
    }

    /// Tool-call arguments come from a `Value` (map-backed), so key order is the
    /// obvious source of nondeterminism. Sorting must remove it.
    #[test]
    fn positive_tool_call_json_key_order_is_stable() {
        let mut m = Message::assistant("");
        m.tool_calls.push(agent_core::ToolCall {
            id: "1".into(),
            name: "t".into(),
            arguments: serde_json::json!({"z": 1, "a": 2, "m": {"y": 1, "b": 2}}),
        });
        let out = markdown("s", &[m]);
        let a_at = out.find("\"a\"").unwrap();
        let z_at = out.find("\"z\"").unwrap();
        assert!(a_at < z_at, "keys must be sorted: {out}");
    }

    /// The transcript is attacker-influenced and the export opens in a browser.
    #[rstest]
    #[case::adversarial_script_tag("<script>alert(1)</script>")]
    #[case::adversarial_img_onerror("<img src=x onerror=alert(1)>")]
    #[case::adversarial_attr_breakout("\" onmouseover=\"alert(1)")]
    #[case::adversarial_single_quote_breakout("' onfocus='alert(1)")]
    #[case::adversarial_closing_pre("</pre><script>alert(1)</script><pre>")]
    fn adversarial_html_is_escaped(#[case] payload: &str) {
        let out = html("s", &[Message::user(payload)]);
        // The security property is that no payload character can START markup or
        // break out of an attribute — NOT that the substring disappears. An
        // escaped `&lt;img src=x onerror=…` still contains "onerror=" as inert
        // text, which is correct and must not be asserted against.
        assert!(
            !out.contains(payload),
            "payload survived verbatim (unescaped): {out}"
        );
        for raw in ['<', '>', '"', '\''] {
            let dangerous = format!("{raw}");
            let body_start = out.find("<pre>").expect("a body block") + 5;
            let body_end = out[body_start..].find("</pre>").expect("closed") + body_start;
            assert!(
                !out[body_start..body_end].contains(&dangerous),
                "raw `{raw}` reached the rendered body: {}",
                &out[body_start..body_end]
            );
        }
    }

    /// A hostile session id reaches both the <title> and a <code> block.
    #[test]
    fn adversarial_session_id_is_escaped() {
        let out = html("</title><script>alert(1)</script>", &[]);
        assert!(!out.contains("<script"), "id escaped into markup: {out}");
    }

    /// C0 controls can smuggle terminal escape sequences into a rendered page.
    #[test]
    fn adversarial_control_characters_are_stripped() {
        let out = html("s", &[Message::user("before\u{1b}[31mred\u{7}after")]);
        assert!(!out.contains('\u{1b}'), "escape char survived");
        assert!(!out.contains('\u{7}'), "bell char survived");
        assert!(out.contains("before"), "legitimate text was dropped");
    }

    /// The page must not fetch anything: it is opened from disk, possibly
    /// offline, and must not phone home either.
    #[test]
    fn positive_html_is_self_contained() {
        let out = html("s", &transcript());
        assert!(out.contains("<style>"), "CSS must be inlined");
        for bad in ["http://", "https://", "<link", "<script", "src="] {
            assert!(!out.contains(bad), "external reference `{bad}`: {out}");
        }
    }

    /// Media is named, not silently dropped — a transcript that omits an image
    /// misrepresents what happened.
    #[test]
    fn positive_media_is_described_not_dropped() {
        let m = Message::with_blocks(
            Role::User,
            vec![
                ContentBlock::text("look:"),
                ContentBlock::image("image/png", vec![0u8; 12]),
            ],
        );
        for f in [Format::Markdown, Format::Html] {
            let out = render(f, "s", std::slice::from_ref(&m));
            assert!(out.contains("image/png"), "{} lost the image", f.as_str());
        }
    }

    #[rstest]
    #[case::positive_md("md", Some(Format::Markdown))]
    #[case::positive_markdown("MARKDOWN", Some(Format::Markdown))]
    #[case::positive_json("json", Some(Format::Json))]
    #[case::positive_html("html", Some(Format::Html))]
    #[case::negative_unknown("pdf", None)]
    #[case::boundary_empty("", None)]
    fn format_parse_cases(#[case] s: &str, #[case] want: Option<Format>) {
        assert_eq!(Format::parse(s), want);
    }

    #[rstest]
    #[case::boundary_empty_transcript(Format::Markdown)]
    #[case::boundary_empty_json(Format::Json)]
    #[case::boundary_empty_html(Format::Html)]
    fn boundary_empty_transcript_renders(#[case] f: Format) {
        let out = render(f, "empty", &[]);
        assert!(!out.is_empty(), "must still produce a document");
    }

    /// The JSON export must be parseable — it exists for tooling.
    #[test]
    fn positive_json_export_round_trips() {
        let out = json("s", &transcript());
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["session"], "s");
        assert_eq!(v["message_count"], 3);
        assert_eq!(v["messages"].as_array().unwrap().len(), 3);
    }
}
