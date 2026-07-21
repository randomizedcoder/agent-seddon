# Parity spec 11 — web_fetch

Per-feature parity spec for a read-only HTTP fetch tool: GET a URL → sanitized
markdown / text / HTML, under size + timeout + redirect caps, with an SSRF guard.
Tracks the intended `WebBackend` seam, what the peers assert, and the concrete
behaviour + tests needed to be the most complete of the four.

> **Status: implemented** (seam + `local` transport + tool + SSRF guard +
> observability + bench + leak). The **`WebBackend` seam** (`async fn
> fetch(WebRequest) -> WebResponse` in `agent-core`) ships with a `local`
> reqwest-backed transport ([`agent-web`](../../crates/agent-web)) and a
> `web_fetch` **`Tool`** ([`agent-tools`](../../crates/agent-tools/src/web.rs))
> in front of it, wired by the builder and config-selected (`[web] backend`).
> The **differentiator vs the peers** landed: an **SSRF / private-IP guard wired
> through the existing `Policy` seam**
> ([`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs),
> `scan_ssrf_target`, `ssrf_target` category) that screens the destination
> *before the socket opens* and denies localhost / loopback / link-local /
> RFC-1918 / cloud-metadata (`169.254.169.254`) targets by default — including
> the obfuscated-IP encodings (decimal/hex/short/IPv4-mapped/userinfo), which
> `Url::parse` normalises before classification — through the same
> `Decision::Deny("blocked by policy guard: …")` path as the dangerous-command
> guard, with an opaque reason. Plus outcome Prometheus metrics + an OTel
> `web.fetch` span carrying `host`/`format`/`status`/`bytes`. **Deferred to a
> follow-up** (staged like the tokenizer seam): the `agent.v1.WebService` gRPC
> worker (`agent --serve-web`) and DNS-name→IP resolution screening (a public
> name resolving to a private address; per-redirect-hop re-screening). Metrics
> are **not** labelled by host (untrusted URL → cardinality DoS); the host is a
> span attribute. See [`docs/components/web-fetch.md`](../components/web-fetch.md).

## Feature & why it matters

`web_fetch` lets the model pull a documentation page, a raw file, or an API
response into context: HTTP(S) GET → the body decoded and reduced to
model-friendly text (markdown by default, or `text`/`html`). It is the agent's
one legitimate outbound-network primitive, which is exactly why it is dangerous.

An unguarded fetch tool is a textbook **SSRF vector**: the model (or a prompt
injected into a page it just read) can be steered to `http://169.254.169.254/…`
to exfiltrate cloud IAM credentials, to `http://localhost:<port>` to reach a
service bound to loopback, or to a link-local / RFC-1918 address to pivot inside
the private network. It is also a **resource** and **context** hazard: an
unbounded body blows the context window, an unbounded redirect chain loops, and
a stalled server hangs the turn. How aggressively the destination is screened,
the body sanitized, and the caps enforced is where the peers diverge — and where
agent-seddon's `Policy`-wired guard lets it exceed them.

## agent-seddon today

**Absent.** There is no `web_fetch` tool, no `WebBackend` seam, and no outbound
HTTP client in the tool surface. The model's only route to the network today is
`bash` (`curl`/`wget`), which the policy guard already screens for the
*download-piped-to-a-shell* RCE shape
([`scan_dangerous` / `is_remote_exec`](../../crates/agent-runtime/src/policy.rs))
but which is otherwise unconfined by design (`bash` is the intentional escape
hatch). Nothing screens the *destination IP* of an outbound request.

Closest existing seams / harness to reuse (do **not** rebuild):

- **`Policy` seam** — [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs).
  The `Guard` wrapper already screens a `ToolCall` and returns a uniform
  `Decision::Deny(reason)`; its `scan_sensitive_path` / `scan_dangerous`
  pattern is the exact shape an **SSRF screen** (`scan_ssrf_target`) plugs into.
  The guard is metered via `Metrics::on_policy_guard(category, outcome)` — an
  `ssrf_target` category label slots straight in.
- **Tool + shared safety** — [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)
  (`truncate` / `MAX_OUTPUT = 12_000` for the model-visible preview cap) and the
  `Tool` trait / `Observation::error` model-visible-error convention used by
  every existing tool (`crates/agent-tools/src/core.rs`).
- **Seam-over-gRPC harness** — the `SearchBackend` seam is the template: a
  `<seam>.proto` ([`crates/agent-proto/proto/agent/v1/search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto)),
  a metered decorator in [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs),
  a registry factory in [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`), a `--serve-<seam>` server with reflection, and the
  roundtrip test in `crates/agent-grpc/tests/`.
- **Test doubles** — [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`tempdir()`, `bench`, `observe`/`captured_spans`); a new `FakeWebBackend`
  double (canned `WebResponse` per URL, records requests) belongs here alongside
  `ScriptedProvider`.

## Peer implementations & their tests

| Peer     | Impl path | Test path | Framework |
| -------- | --------- | --------- | --------- |
| opencode | `opencode/packages/core/src/tool/webfetch.ts` | `opencode/packages/core/test/tool-webfetch.test.ts` | bun:test + Effect (fake `HttpClient` layer, `TestClock`, live `Bun.serve`) |
| hermes   | `hermes-agent/tools/web_tools.py` (`web_extract_tool`) | `hermes-agent/tests/plugins/web/*` | pytest (`monkeypatch` on `tools.web_tools.httpx`) |
| pi       | — (no fetch/http tool; network only via `bash`) | — | — |

**pi** ships no fetch/http tool — its only network path is the `bash` tool
(`pi/packages/coding-agent/src/core/tools/bash.ts`); it is intentionally absent
from this table.

**opencode** (`webfetch`, the canonical port) asserts:

- **Registers** as `webfetch`; input is `{ url, format, timeout? }`, output is
  `{ url, contentType, format, output }`. Default `format` is **markdown**
  (schema-decoded default); `timeout` must be `> 0` and `<= MAX_TIMEOUT_SECONDS`
  (120) — `0` and `121` are rejected at decode time.
- **Scheme allow-list:** only `http:` / `https:`; `file:///etc/passwd` ⇒
  `Unable to fetch …` **before** any permission or transport call (asserts zero
  requests, zero permission assertions).
- **Permission on the requested URL:** asserts a `webfetch` permission for the
  raw `url` *before* fetching; the same check applies to a `localhost` URL (it
  is **not** blocked — opencode has no SSRF screen, our differentiator).
- **Redirects followed, only the requested URL approved:** a `302 → /target`
  chain returns the final body while the permission still names the original
  URL (live `Bun.serve`).
- **HTML sanitization + conversion:** `format:"text"` strips `script`, `style`,
  `noscript`, `iframe`, `object`, `embed` (via `htmlparser2`) →
  `"<h1>Hello</h1><script>bad()</script><p>world <strong>wide</strong></p>"`
  becomes `"Helloworld wide"`; `format:"markdown"` (via `turndown`, removing
  `script/style/meta/link`) becomes `"# Hello\n\nworld **wide**"`. Conversion
  only runs when `content-type` includes `text/html`.
- **Size cap** `MAX_RESPONSE_BYTES = 5 MiB`: a **declared** oversized
  `content-length` and a **streamed** oversized body both fail.
- **Content-type gating:** image (`image/png`) and non-textual file
  (`application/pdf`) content types are rejected; only textual MIME
  (`text/*`, `+json`, `+xml`, javascript) is decoded.
- **Timeout:** a stalled response fails with `Request timed out` after the
  requested `timeout` (`TestClock`-driven).
- **Cloudflare-challenge retry:** a `403` with `cf-mitigated: challenge` is
  retried once with an honest `opencode` user-agent (first request uses a
  browser UA).
- On any failure the model-visible error is the **opaque** `Unable to fetch
  <url>` (no disclosure of which stage failed).

**hermes** (`web_extract_tool`) asserts (a heavier provider-backed extractor,
not a thin fetch): extracts content from a list of URLs via a configured backend
(Firecrawl / Exa / Tavily / Parallel), `format` ∈ `{markdown, html}`, validates
and filters URLs (`safe_urls`) before calling the provider, and LLM-compresses
the result to reduce tokens. Its `httpx` client is monkeypatched in tests. It is
a search/scrape gateway (closer to spec 12), listed here as the hermes web-read
peer.

## Completeness gaps

Behaviour agent-seddon must add / guarantee to be the most complete (spec only —
do **not** implement here). Each maps to a §7 case.

- **SSRF / private-IP guard through `Policy` (the differentiator; no peer has
  it).** Resolve the host, then **deny before connect** when the destination is
  loopback (`127.0.0.0/8`, `::1`), link-local (`169.254.0.0/16`, `fe80::/10`),
  cloud-metadata (`169.254.169.254`, `fd00:ec2::254`), private RFC-1918
  (`10/8`, `172.16/12`, `192.168/16`) or unique-local (`fc00::/7`); the deny
  flows through the `Guard` as a `Decision::Deny("blocked by policy guard:
  private/link-local address …")`. Screen the **post-redirect** host on every
  hop (a public URL that 302s to `169.254.169.254` must be caught). Config
  `[web] allow_private = true` opts in for local dev; a per-host allow glob
  exempts a named host.
- **Scheme allow-list.** Only `http`/`https`; `file:`, `ftp:`, `gopher:`,
  `data:` rejected before any DNS or socket work (mirrors opencode, but denied
  by the seam not just the tool).
- **Body sanitization.** HTML → markdown / plain text with active content
  (`script`/`style`/`noscript`/`iframe`/`object`/`embed`) stripped; markdown is
  the default format.
- **Format control.** `format` ∈ `{markdown, text, html}` honored; conversion
  only when the response is HTML, raw body otherwise; content-type gated to
  textual MIME (non-text ⇒ typed unsupported error, not mojibake).
- **Caps.** Size cap (declared `content-length` **and** streamed body), request
  timeout (default 30 s, max 120 s), **and a redirect cap** (default ~5;
  opencode caps neither redirects explicitly nor screens them for SSRF —
  agent-seddon does both).
- **Model-visible preview cap.** The decoded output is `truncate`d to
  `MAX_OUTPUT` for the model with a `[output truncated]` marker (reusing
  `agent-tools`), while the full body length is reported.
- **Non-disclosure on denial.** An SSRF-denied and a scheme-denied fetch both
  return an opaque error that does not leak whether the host resolved or
  responded (matches the `Policy` "no why-oracle" convention).
- **Per-host observability.** Prometheus counters/histograms labelled by host +
  outcome, and a `web.fetch` OTel span with `host`/`format`/`status`/`bytes`
  attributes (the #44 attribute pattern).

## Table-driven test plan

Two layers, both `#[rstest]` `#[case::…]`, mirroring `edit.rs` / `policy.rs`:

1. **SSRF / scheme screen** — a pure `scan_ssrf_target(url, allow_private,
   allow_hosts)` unit table in `policy.rs` `mod tests` (no network), exactly like
   `scan_dangerous_cases` / `scan_sensitive_path_cases`.
2. **`web_fetch` tool** — a `#[tokio::test]` table over a `FakeWebBackend`
   double (canned `WebResponse` keyed by URL / a redirect script; records
   requested URLs), so the tool's format/cap/decode logic is exercised without a
   real socket, matching opencode's fake-`HttpClient` approach. A single live
   loopback server (`tiny_http` on `127.0.0.1:0`, run with `[web] allow_private
   = true`) covers the real redirect-follow + timeout paths, mirroring
   opencode's `Bun.serve` cases.

Doubles: `agent_testkit::{tempdir, FakeWebBackend}` + `captured_spans` for the
span assertion; `agent_core::ToolContext`. Case tags: `(port: opencode)` mirrors
an opencode case; `(new: agent-seddon)` is an agent-seddon-specific invariant
(the whole SSRF layer).

```rust
// --- layer 1: SSRF / scheme screen (pure, in policy.rs mod tests) ------------
// `scan_ssrf_target(url, allow_private=false, allow=&[])` -> Option<reason>.
// `true` ⇒ flagged (denied); `false` ⇒ allowed through to connect.
#[rstest]
// deny: private / loopback / link-local / metadata (the differentiator)
#[case::negative_loopback_v4("http://127.0.0.1/x", true)]            // (new: agent-seddon)
#[case::negative_localhost_name("http://localhost:8080/admin", true)] // (new; opencode ALLOWS this)
#[case::negative_loopback_v6("http://[::1]/x", true)]                // (new: agent-seddon)
#[case::negative_link_local("http://169.254.0.1/x", true)]          // (new: agent-seddon)
#[case::negative_cloud_metadata("http://169.254.169.254/latest/meta-data/iam", true)] // (new)
#[case::negative_rfc1918_10("http://10.0.0.5/x", true)]             // (new: agent-seddon)
#[case::negative_rfc1918_192("http://192.168.1.1/x", true)]        // (new: agent-seddon)
#[case::negative_ipv6_ula("http://[fc00::1]/x", true)]             // (new: agent-seddon)
// deny: non-http scheme (rejected before DNS/socket)
#[case::negative_file_scheme("file:///etc/passwd", true)]           // (port: opencode)
#[case::negative_gopher_scheme("gopher://x/1", true)]              // (new: agent-seddon)
#[case::negative_data_scheme("data:text/html,<h1>x", true)]        // (new: agent-seddon)
// allow: ordinary public hosts pass the screen
#[case::positive_public_v4("http://93.184.216.34/public", false)]  // (port: opencode)
#[case::positive_public_https("https://example.com/doc", false)]   // (port: opencode)
fn scan_ssrf_target_cases(#[case] url: &str, #[case] flagged: bool) {
    assert_eq!(scan_ssrf_target(url, false, &[]).is_some(), flagged, "url: {url}");
}

#[test]
fn ssrf_allow_private_opt_in() {
    // (new: agent-seddon) [web] allow_private = true lets loopback through …
    assert!(scan_ssrf_target("http://127.0.0.1/x", true, &[]).is_none());
}
#[test]
fn ssrf_per_host_allow_glob() {
    // … and an explicit allow glob exempts a single named private host.
    assert!(scan_ssrf_target("http://10.0.0.5/x", false, &["10.0.0.5".into()]).is_none());
}

// --- layer 2: web_fetch tool over FakeWebBackend -----------------------------
// `Ok(substr)` ⇒ ok output contains `substr`; `Err(substr)` ⇒ error contains it.
#[rstest]
#[case::positive_plaintext(                                          // (port: opencode)
    resp("http://example.com/public", "text/plain", "hello"),
    json!({"url": "http://example.com/public", "format": "text"}),
    Ok("hello"))]
#[case::positive_markdown_default(                                   // (port: opencode)
    resp("https://example.com", "text/html; charset=utf-8",
         "<h1>Hello</h1><p>world</p><script>bad()</script>"),
    json!({"url": "https://example.com"}),                            // no format ⇒ markdown
    Ok("# Hello\n\nworld"))]
#[case::corner_html_to_text_strips_active(                           // (port: opencode)
    resp("https://example.com", "text/html",
         "<h1>Hello</h1><script>bad()</script><p>world <strong>wide</strong></p><style>.x{}</style>"),
    json!({"url": "https://example.com", "format": "text"}),
    Ok("Helloworld wide"))]
#[case::corner_html_passthrough_when_format_html(                    // (port: opencode)
    resp("https://example.com", "text/html", "<h1>Hi</h1>"),
    json!({"url": "https://example.com", "format": "html"}),
    Ok("<h1>Hi</h1>"))]
#[case::negative_ssrf_metadata_denied(                               // (new: agent-seddon)
    resp("http://169.254.169.254/latest/meta-data", "text/plain", "SECRET"),
    json!({"url": "http://169.254.169.254/latest/meta-data"}),
    Err("blocked by policy guard"))]                                  // opaque; body never read
#[case::negative_ssrf_localhost_denied(                              // (new; opencode ALLOWS)
    resp("http://localhost:9000/admin", "text/plain", "internal"),
    json!({"url": "http://localhost:9000/admin"}),
    Err("blocked by policy guard"))]
#[case::negative_scheme_rejected(                                    // (port: opencode)
    resp("file:///etc/passwd", "text/plain", "root:x:0:0"),
    json!({"url": "file:///etc/passwd", "format": "text"}),
    Err("must use http"))]                                            // no request issued
#[case::negative_non_textual_mime(                                   // (port: opencode)
    resp("https://example.com/x.pdf", "application/pdf", "%PDF-1.7"),
    json!({"url": "https://example.com/x.pdf"}),
    Err("unsupported"))]
#[case::negative_image_mime(                                         // (port: opencode)
    resp("https://example.com/p.png", "image/png", "\x89PNG"),
    json!({"url": "https://example.com/p.png", "format": "html"}),
    Err("unsupported"))]
#[case::boundary_declared_oversize(                                  // (port: opencode)
    resp_len("https://example.com/big", "text/plain", "x", MAX_RESPONSE_BYTES + 1),
    json!({"url": "https://example.com/big", "format": "text"}),
    Err("too large"))]
#[case::boundary_timeout_bounds(                                     // (port: opencode)
    resp("https://example.com", "text/plain", ""),                    // 0 rejected at parse
    json!({"url": "https://example.com", "timeout": 0}),
    Err("timeout"))]                                                  // 0 <= t <= 120 violated
#[tokio::test]
async fn web_fetch_cases(
    #[case] backend: FakeWebResponse,
    #[case] args: Value,
    #[case] expected: std::result::Result<&str, &str>,
) { /* build FakeWebBackend + Guard(ssrf), run WebFetchTool, assert on obs */ }

// live loopback: real redirect-follow + redirect-cap + streamed-oversize + span
#[tokio::test]
async fn web_fetch_boundary_streamed_oversize_and_redirect_cap() {
    // (port: opencode "rejects streamed oversized bodies" + new redirect cap)
    // tiny_http on 127.0.0.1:0 with [web] allow_private=true:
    //  - /redirect -> 302 /target ; assert final body returned (port: opencode)
    //  - /loop     -> 302 /loop    ; assert Err("too many redirects")   (new)
    //  - /stream   -> body of MAX_RESPONSE_BYTES+1 ; assert Err("too large")
}

#[tokio::test]
async fn web_fetch_emits_span_with_attributes() {
    // (new: agent-seddon) captured_spans finds `web.fetch` with host/format/
    // status/bytes attributes on the success path (cf. #44 span-attribute test).
}
```

Case-prefix key: `positive_` succeeds, `negative_` rejects (SSRF / scheme /
MIME / timeout), `corner_` odd-but-valid (HTML→text/markdown/html variants),
`boundary_` at a cap (size / timeout / redirect count). `(port: opencode)` names
the peer the case came from; `(new: agent-seddon)` marks the SSRF/private-IP,
redirect-cap, and span invariants no peer has.

## Harness obligations (per the plan's per-spec contract)

- **Seam + registry:** new `WebBackend` trait in `agent-core`; `local`
  reqwest-backed impl in a sibling crate behind a `web` cargo feature; one
  factory line in `agent-runtime/src/registry.rs` `register_builtins`
  (config `[web] backend = "local" | "grpc"`). Doc in
  `docs/components/web-fetch.md`. The SSRF screen lives in the `Policy` `Guard`
  (`scan_ssrf_target`, new `ssrf_target` guard category), not the tool, so
  `bash`-borne fetches can share it later.
- **Proto + gRPC:** add `crates/agent-proto/proto/agent/v1/web.proto`
  (`WebService.Fetch(WebRequest) -> WebResponse`) + `build.rs` entry +
  server/client in `agent-grpc` + `--serve-web` + reflection; extend the gRPC
  roundtrip test; commit the `buf.image.binpb` bump via `nix run .#buf-image`;
  add the web endpoint/port to `nix/constants.nix` → `nix run .#gen-constants`.
- **Metrics + OTel:** `agent_web_fetch_total{host,outcome}`,
  `agent_web_fetch_bytes`, latency histogram, and `agent_policy_guard_total{
  category="ssrf_target"}` reused from the guard, added to `agent-metrics` with a
  metered decorator in `agent-runtime/src/metered.rs`; a `web.fetch` span
  carrying `host`/`format`/`status`/`bytes` attributes.
- **Bench:** iai-callgrind bench for the **HTML→markdown/text sanitization** hot
  path (a fixed HTML fixture in `agent-testkit::bench`, deterministic
  instruction count) with an Ir ceiling in `nix/checks/bench.nix`. The transport
  itself is I/O-bound and network-dependent — **not** benched (documented skip).
- **Leak:** dhat `tests/leak.rs` (`dhat-heap` feature) over the fetch→decode→
  sanitize→truncate path, asserting the hot path frees its buffers within an
  allocation budget.

## References

- **agent-seddon (to extend):**
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  (`Guard`, `scan_dangerous`, `scan_sensitive_path`, `is_remote_exec`, guard
  metrics — the SSRF screen plugs in here),
  [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)
  (`truncate`, `MAX_OUTPUT`), [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs)
  (`Tool` / `Observation::error` convention),
  [`crates/agent-proto/proto/agent/v1/search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto)
  (seam-over-gRPC template),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
  (`tempdir`, `bench`, `observe`/`captured_spans` — home of the new
  `FakeWebBackend`).
- **opencode:** `packages/core/src/tool/webfetch.ts` (impl —
  `MAX_RESPONSE_BYTES`, `DEFAULT/MAX_TIMEOUT_SECONDS`, `extractTextFromHTML`,
  `convertHTMLToMarkdown`, scheme + MIME gating, Cloudflare retry),
  `packages/core/test/tool-webfetch.test.ts` (tests).
- **hermes:** `tools/web_tools.py` (`web_extract_tool` — provider-backed URL
  extraction, `format` markdown/html, `safe_urls` filter), `tests/plugins/web/`.
- **pi:** no fetch/http tool; outbound network only via
  `packages/coding-agent/src/core/tools/bash.ts`.
