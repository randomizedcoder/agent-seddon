//! `tool-web` — the `web_fetch` tool over the `WebBackend` seam (parity spec 11).
//!
//! The backend (`agent-web`) is transport-only; this tool owns the model-facing
//! logic: argument validation (scheme, `format`, `timeout` range), MIME-gating
//! (only textual bodies are decoded), the HTML→markdown/text conversion, and the
//! `MAX_OUTPUT` preview cap. The SSRF destination screen lives in the `Policy`
//! guard (`agent-runtime`), which runs *before* this tool executes.

use crate::{arg_str, truncate};
use agent_core::{
    Observation, Result, Tool, ToolContext, ToolSchema, WebBackend, WebFormat, WebRequest,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// The `web_fetch` tool. Holds the (already metered) backend + the caps a single
/// fetch is bounded by.
pub struct WebFetchTool {
    backend: Arc<dyn WebBackend>,
    max_bytes: u64,
    default_timeout_secs: u64,
    max_timeout_secs: u64,
    max_redirects: u32,
}

impl WebFetchTool {
    pub fn new(
        backend: Arc<dyn WebBackend>,
        max_bytes: u64,
        default_timeout_secs: u64,
        max_timeout_secs: u64,
        max_redirects: u32,
    ) -> Self {
        Self {
            backend,
            max_bytes,
            default_timeout_secs,
            max_timeout_secs,
            max_redirects,
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_fetch".into(),
            description: "Fetch a web page or file over HTTP(S) and return its text. \
                          `format` is markdown (default), text, or html. Private / \
                          loopback / cloud-metadata addresses are blocked."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The http(s) URL to fetch." },
                    "format": {
                        "type": "string",
                        "enum": ["markdown", "text", "html"],
                        "description": "How to reduce the body (default markdown)."
                    },
                    "timeout": { "type": "integer", "description": "Per-request timeout in seconds." }
                },
                "required": ["url"]
            }),
        }
    }
    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<Observation> {
        let url = arg_str(&args, "url")?;
        // Cheap scheme reject (the Policy guard also screens this + SSRF).
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Ok(Observation::error(
                "refusing to fetch: URL must use http/https",
            ));
        }
        let format = match args.get("format").and_then(Value::as_str) {
            None | Some("markdown") => WebFormat::Markdown,
            Some("text") => WebFormat::Text,
            Some("html") => WebFormat::Html,
            Some(other) => {
                return Ok(Observation::error(format!(
                    "unknown format `{other}` (use markdown|text|html)"
                )))
            }
        };
        // Timeout: default, or a caller value clamped to the valid open interval.
        let timeout_secs = match args.get("timeout").and_then(Value::as_u64) {
            None => self.default_timeout_secs,
            Some(t) if (1..=self.max_timeout_secs).contains(&t) => t,
            Some(_) => {
                return Ok(Observation::error(format!(
                    "timeout must be between 1 and {}s",
                    self.max_timeout_secs
                )))
            }
        };

        let req = WebRequest {
            url: url.to_string(),
            format,
            timeout_secs,
            max_bytes: self.max_bytes,
            max_redirects: self.max_redirects,
        };
        let resp = match self.backend.fetch(&req).await {
            Ok(r) => r,
            // Opaque, no stage disclosure (matches the Policy "no why-oracle").
            Err(e) => return Ok(Observation::error(format!("web_fetch failed: {e}"))),
        };

        // MIME gate: only textual bodies are decoded (an image/PDF is a typed
        // error, not mojibake).
        if !is_textual(&resp.content_type) {
            return Ok(Observation::error(format!(
                "unsupported content type `{}` (only text is fetched)",
                resp.content_type
            )));
        }

        let reduced = reduce(&resp.body, &resp.content_type, format);
        Ok(Observation::ok(truncate(reduced)))
    }
}

/// A textual MIME type we're willing to decode. An empty content-type (common for
/// raw files) is treated as text; images / PDFs / octet-streams are rejected.
fn is_textual(content_type: &str) -> bool {
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    ct.is_empty()
        || ct.starts_with("text/")
        || ct == "application/json"
        || ct.ends_with("+json")
        || ct == "application/xml"
        || ct.ends_with("+xml")
        || ct.contains("javascript")
        || ct.contains("ecmascript")
}

fn is_html(content_type: &str) -> bool {
    content_type.to_ascii_lowercase().contains("html")
}

/// Reduce a raw body to the requested `format`. HTML→markdown/text conversion
/// only runs when the response is actually HTML; otherwise the body passes
/// through unchanged (a `text/plain` file stays verbatim).
fn reduce(body: &str, content_type: &str, format: WebFormat) -> String {
    match format {
        WebFormat::Html => body.to_string(),
        WebFormat::Text if is_html(content_type) => html_to_text(body),
        WebFormat::Markdown if is_html(content_type) => html_to_markdown(body),
        _ => body.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Dependency-free HTML sanitizer / converter
// ---------------------------------------------------------------------------
//
// A single forward-scan tokenizer feeds two consumers (text + markdown). Active
// content (`script`/`style`/`noscript`/`iframe`/`object`/`embed`/…) is dropped
// entirely — the model never sees executable markup. This is the CPU hot path the
// iai bench (`benches/web.rs`) guards.

/// A parsed HTML token. `Text` borrows from the input; tag names are lowercased.
enum Event<'a> {
    Text(&'a str),
    Open(String),
    Close(String),
}

/// Tags whose *content* is discarded (not just the tags): active/embedded content
/// and document-metadata elements.
fn is_drop(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "noscript"
            | "iframe"
            | "object"
            | "embed"
            | "template"
            | "head"
            | "svg"
            | "canvas"
            | "meta"
            | "link"
            | "title"
    )
}

/// Forward-scan tokenizer: splits HTML into text nodes and open/close tags,
/// skipping comments and declarations, and quote-aware so a `>` inside an
/// attribute value doesn't prematurely close a tag. Self-closing tags emit an
/// `Open` immediately followed by a `Close`.
fn tokenize(html: &str) -> Vec<Event<'_>> {
    let bytes = html.as_bytes();
    let n = bytes.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                i = html[i + 4..]
                    .find("-->")
                    .map(|e| i + 4 + e + 3)
                    .unwrap_or(n);
                continue;
            }
            if html[i..].starts_with("<!") || html[i..].starts_with("<?") {
                i = html[i..].find('>').map(|e| i + e + 1).unwrap_or(n);
                continue;
            }
            let closing = bytes.get(i + 1) == Some(&b'/');
            let name_start = if closing { i + 2 } else { i + 1 };
            let mut j = name_start;
            while j < n && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-') {
                j += 1;
            }
            if j == name_start {
                // A stray `<` (e.g. `a < b`) — emit it as literal text.
                out.push(Event::Text(&html[i..i + 1]));
                i += 1;
                continue;
            }
            let name = html[name_start..j].to_ascii_lowercase();
            // Scan to the tag's `>`, respecting quoted attribute values.
            let mut k = j;
            let mut quote = 0u8;
            while k < n {
                let c = bytes[k];
                if quote != 0 {
                    if c == quote {
                        quote = 0;
                    }
                } else if c == b'"' || c == b'\'' {
                    quote = c;
                } else if c == b'>' {
                    break;
                }
                k += 1;
            }
            let self_closing = html[j..k].trim_end().ends_with('/');
            if closing {
                out.push(Event::Close(name));
            } else {
                out.push(Event::Open(name.clone()));
                if self_closing {
                    out.push(Event::Close(name));
                }
            }
            i = if k < n { k + 1 } else { n };
        } else {
            let start = i;
            while i < n && bytes[i] != b'<' {
                i += 1;
            }
            out.push(Event::Text(&html[start..i]));
        }
    }
    out
}

/// Iterate tokens, invoking `f` for each non-dropped `Event` (content inside a
/// drop element — and nested same-named opens — is skipped). Shared by both
/// consumers so the drop logic lives in one place.
fn walk<'a>(events: &'a [Event<'a>], mut f: impl FnMut(&'a Event<'a>)) {
    let mut skip_depth = 0usize;
    let mut skip_tag: &str = "";
    for ev in events {
        match ev {
            Event::Open(name) if skip_depth > 0 => {
                if name == skip_tag {
                    skip_depth += 1;
                }
            }
            Event::Close(name) if skip_depth > 0 => {
                if name == skip_tag {
                    skip_depth -= 1;
                }
            }
            _ if skip_depth > 0 => {}
            Event::Open(name) if is_drop(name) => {
                skip_tag = name;
                skip_depth = 1;
            }
            other => f(other),
        }
    }
}

/// HTML → plain text: keep text nodes, drop all markup + active content, collapse
/// whitespace runs to single spaces, and trim.
pub(crate) fn html_to_text(html: &str) -> String {
    let events = tokenize(html);
    let mut out = String::new();
    walk(&events, |ev| {
        if let Event::Text(t) = ev {
            out.push_str(&decode_entities(t));
        }
    });
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// HTML → markdown: headings, paragraphs, lists, blockquotes, line breaks, and
/// the common inline emphasis/code marks; active content dropped.
pub(crate) fn html_to_markdown(html: &str) -> String {
    let events = tokenize(html);
    let mut out = String::new();
    walk(&events, |ev| match ev {
        Event::Open(name) => match name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                ensure_blank(&mut out);
                let level = name[1..].parse::<usize>().unwrap_or(1);
                out.push_str(&"#".repeat(level));
                out.push(' ');
            }
            "p" | "div" | "section" | "article" | "ul" | "ol" => ensure_blank(&mut out),
            "br" => out.push('\n'),
            "hr" => {
                ensure_blank(&mut out);
                out.push_str("---");
                ensure_blank(&mut out);
            }
            "li" => {
                ensure_nl(&mut out);
                out.push_str("- ");
            }
            "blockquote" => {
                ensure_blank(&mut out);
                out.push_str("> ");
            }
            "strong" | "b" => out.push_str("**"),
            "em" | "i" => out.push('*'),
            "code" => out.push('`'),
            _ => {}
        },
        Event::Close(name) => match name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "div" | "section" | "article"
            | "ul" | "ol" | "blockquote" => ensure_blank(&mut out),
            "li" => ensure_nl(&mut out),
            "strong" | "b" => out.push_str("**"),
            "em" | "i" => out.push('*'),
            "code" => out.push('`'),
            _ => {}
        },
        Event::Text(t) => out.push_str(&collapse_inline(&decode_entities(t))),
    });
    normalize_blanks(&out)
}

/// Ensure `out` ends with a blank line (`\n\n`) — a paragraph/block separator.
/// No-op on empty output so we never emit leading blanks.
fn ensure_blank(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while out.ends_with(' ') {
        out.pop();
    }
    while !out.ends_with("\n\n") {
        out.push('\n');
    }
}

/// Ensure `out` ends with a single newline (list-item separator).
fn ensure_nl(out: &mut String) {
    if out.is_empty() {
        return;
    }
    while out.ends_with(' ') {
        out.pop();
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

/// Collapse internal whitespace runs to single spaces while preserving a single
/// leading/trailing space (so inline word boundaries survive).
fn collapse_inline(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                r.push(' ');
                prev_ws = true;
            }
        } else {
            r.push(c);
            prev_ws = false;
        }
    }
    r
}

/// Collapse 3+ consecutive newlines to a blank line and trim the ends.
fn normalize_blanks(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    let mut nl = 0;
    for c in s.chars() {
        if c == '\n' {
            nl += 1;
            if nl <= 2 {
                r.push('\n');
            }
        } else {
            nl = 0;
            r.push(c);
        }
    }
    r.trim().to_string()
}

/// Decode the handful of HTML entities that matter for readable text (named +
/// numeric decimal/hex). Unknown entities are left verbatim.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s.as_bytes()[i] == b'&' {
            if let Some(rel) = s[i..].find(';') {
                if (1..=10).contains(&rel) {
                    let ent = &s[i + 1..i + rel];
                    if let Some(c) = named_entity(ent) {
                        out.push(c);
                        i += rel + 1;
                        continue;
                    }
                    if let Some(num) = ent.strip_prefix('#') {
                        let cp = num
                            .strip_prefix(['x', 'X'])
                            .and_then(|h| u32::from_str_radix(h, 16).ok())
                            .or_else(|| num.parse::<u32>().ok());
                        if let Some(c) = cp.and_then(char::from_u32) {
                            out.push(c);
                            i += rel + 1;
                            continue;
                        }
                    }
                }
            }
            out.push('&');
            i += 1;
        } else {
            let ch = s[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "copy" => '©',
        "reg" => '®',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        _ => return None,
    })
}

/// Bench hook (dependency of `benches/web.rs`): run both conversions over a fixed
/// HTML fixture. Public but hidden so the iai bench can reach the hot path.
#[doc(hidden)]
pub fn bench_sanitize(html: &str) -> usize {
    html_to_markdown(html).len() + html_to_text(html).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::WebResponse;
    use rstest::rstest;

    fn ctx() -> ToolContext {
        ToolContext {
            cwd: std::path::PathBuf::from("."),
        }
    }

    // A one-shot backend double: returns a canned response, or a canned error
    // (e.g. "too large") regardless of the request. Kept local to the tool test.
    struct OneShot {
        content_type: String,
        body: String,
        error: Option<String>,
    }
    #[async_trait]
    impl WebBackend for OneShot {
        async fn fetch(&self, req: &WebRequest) -> Result<WebResponse> {
            if let Some(e) = &self.error {
                return Err(agent_core::Error::Web(e.clone()));
            }
            Ok(WebResponse {
                final_url: req.url.clone(),
                status: 200,
                content_type: self.content_type.clone(),
                format: req.format,
                body: self.body.clone(),
                bytes: self.body.len() as u64,
            })
        }
    }

    fn tool(content_type: &str, body: &str, error: Option<&str>) -> WebFetchTool {
        WebFetchTool::new(
            Arc::new(OneShot {
                content_type: content_type.into(),
                body: body.into(),
                error: error.map(String::from),
            }),
            5 * 1024 * 1024,
            30,
            120,
            5,
        )
    }

    async fn run(t: &WebFetchTool, args: Value) -> Observation {
        t.execute(args, &ctx())
            .await
            .unwrap_or_else(|e| Observation::error(e.to_string()))
    }

    // --- html conversion (pure) --------------------------------------------
    #[rstest]
    #[case::strips_active(
        "<h1>Hello</h1><script>bad()</script><p>world <strong>wide</strong></p><style>.x{}</style>",
        "Helloworld wide"
    )]
    #[case::entities("<p>a &amp; b &lt; c</p>", "a & b < c")]
    #[case::plain("<p>just text</p>", "just text")]
    fn html_to_text_cases(#[case] html: &str, #[case] expected: &str) {
        assert_eq!(html_to_text(html), expected);
    }

    #[rstest]
    #[case::heading_and_para("<h1>Hello</h1><p>world</p>", "# Hello\n\nworld")]
    #[case::inline_strong(
        "<h1>Hello</h1><p>world <strong>wide</strong></p>",
        "# Hello\n\nworld **wide**"
    )]
    #[case::list("<ul><li>one</li><li>two</li></ul>", "- one\n- two")]
    #[case::drops_script("<p>keep</p><script>drop()</script>", "keep")]
    fn html_to_markdown_cases(#[case] html: &str, #[case] expected: &str) {
        assert_eq!(html_to_markdown(html), expected);
    }

    // --- web_fetch tool over the OneShot double ----------------------------
    // `Ok(substr)` ⇒ ok output contains substr; `Err(substr)` ⇒ error contains it.
    #[rstest]
    #[case::positive_plaintext(
        "text/plain", "hello", None,
        json!({"url": "http://example.com/public", "format": "text"}),
        Ok("hello"))]
    #[case::positive_markdown_default(
        "text/html; charset=utf-8", "<h1>Hello</h1><p>world</p><script>bad()</script>", None,
        json!({"url": "https://example.com"}),
        Ok("# Hello\n\nworld"))]
    #[case::corner_html_to_text(
        "text/html", "<h1>Hello</h1><script>bad()</script><p>world <strong>wide</strong></p>", None,
        json!({"url": "https://example.com", "format": "text"}),
        Ok("Helloworld wide"))]
    #[case::corner_html_passthrough(
        "text/html", "<h1>Hi</h1>", None,
        json!({"url": "https://example.com", "format": "html"}),
        Ok("<h1>Hi</h1>"))]
    #[case::negative_scheme_rejected(
        "text/plain", "root:x", None,
        json!({"url": "file:///etc/passwd", "format": "text"}),
        Err("must use http"))]
    #[case::negative_non_textual_mime(
        "application/pdf", "%PDF-1.7", None,
        json!({"url": "https://example.com/x.pdf"}),
        Err("unsupported"))]
    #[case::negative_image_mime(
        "image/png", "\u{89}PNG", None,
        json!({"url": "https://example.com/p.png", "format": "html"}),
        Err("unsupported"))]
    #[case::boundary_too_large(
        "text/plain", "", Some("response too large (5242881 bytes)"),
        json!({"url": "https://example.com/big", "format": "text"}),
        Err("too large"))]
    #[case::boundary_timeout_zero(
        "text/plain", "x", None,
        json!({"url": "https://example.com", "timeout": 0}),
        Err("timeout"))]
    #[case::boundary_timeout_over_max(
        "text/plain", "x", None,
        json!({"url": "https://example.com", "timeout": 121}),
        Err("timeout"))]
    #[case::negative_bad_format(
        "text/plain", "x", None,
        json!({"url": "https://example.com", "format": "pdf"}),
        Err("unknown format"))]
    #[case::negative_missing_url(
        "text/plain", "x", None,
        json!({}),
        Err("missing string argument"))]
    #[tokio::test]
    async fn web_fetch_cases(
        #[case] content_type: &str,
        #[case] body: &str,
        #[case] error: Option<&str>,
        #[case] args: Value,
        #[case] expected: std::result::Result<&str, &str>,
    ) {
        let t = tool(content_type, body, error);
        let obs = run(&t, args).await;
        match expected {
            Ok(substr) => {
                assert!(!obs.is_error, "unexpected error: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "output `{}` missing `{substr}`",
                    obs.content
                );
            }
            Err(substr) => {
                assert!(obs.is_error, "expected error, got ok: {}", obs.content);
                assert!(
                    obs.content.contains(substr),
                    "error `{}` missing `{substr}`",
                    obs.content
                );
            }
        }
    }

    // A body over the preview cap comes back truncated with the marker.
    #[tokio::test]
    async fn web_fetch_boundary_output_capped() {
        let big = "a".repeat(crate::MAX_OUTPUT + 500);
        let t = tool("text/plain", &big, None);
        let obs = run(&t, json!({"url": "https://example.com", "format": "text"})).await;
        assert!(!obs.is_error);
        assert!(obs.content.ends_with("[output truncated]"));
    }
}
