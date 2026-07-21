# web_fetch — the `WebBackend` seam

The agent's one legitimate outbound-network primitive: HTTP(S) `GET` a URL → the
body decoded and reduced to model-friendly text (markdown by default, or
`text`/`html`), under size / timeout / redirect caps. Because a prompt-injected
model chooses the URL, the destination is **SSRF-screened by the `Policy` guard
before the socket opens** — that screen is the differentiator over the peers
(opencode/hermes fetch tools have none). See parity spec
[`11-web-fetch.md`](../parity/11-web-fetch.md).

- **Trait:** `agent_core::WebBackend` ([`agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)) —
  `async fn fetch(&WebRequest) -> WebResponse`. Transport only: it enforces the
  request's size / timeout / redirect caps and returns the **raw** decoded body.
  MIME-gating and the HTML→`format` conversion are the tool's job (so they are
  unit-testable over a `FakeWebBackend` without a socket).
- **Impl crate:** [`agent-web`](../../crates/agent-web).
- **Shipped backend:** `local` (`web-local`) — a `reqwest` (rustls) client with a
  bounded redirect policy, a per-request timeout, and a body cap enforced on both
  the declared `content-length` **and** the streamed bytes (a lying/absent length
  can't smuggle an oversized body past the cap). A `grpc` fetch worker
  (`WebService.Fetch`) is a documented follow-up, staged like the tokenizer seam.
- **Tool:** `web_fetch` (`agent-tools`, `tool-web`) over the seam. Owns argument
  validation (scheme, `format ∈ {markdown,text,html}`, `timeout` clamped to
  `(0, max]`), MIME-gating (only textual bodies decode — an image/PDF is a typed
  `unsupported` error, not mojibake), the **dependency-free HTML sanitizer**
  (single-pass tokenizer → markdown/text; `script`/`style`/`noscript`/`iframe`/
  `object`/`embed` content is dropped so the model never sees active markup), and
  the `MAX_OUTPUT` preview cap.
- **Runtime feature:** `web` (default) — builds the `local` backend, meters it,
  and registers the `web_fetch` tool.
- **Config:** `[web] backend = "local"`, `max_bytes` (default 5 MiB),
  `timeout_secs` (30) / `max_timeout_secs` (120), `max_redirects` (5), plus the
  SSRF fields below.

## The SSRF / private-IP guard (the differentiator)

The destination screen lives in the `Policy` `Guard`
([`agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)) as a
new `ssrf_target` category, alongside the dangerous-command and sensitive-path
screens. A `web_fetch` call is denied — through the same
`Decision::Deny("blocked by policy guard: …")` path, with an **opaque** reason
(no host-resolved / responded oracle) — when its URL:

- uses a scheme other than `http`/`https` (`file:`/`gopher:`/`data:` rejected
  before any DNS or socket work); or
- resolves *by literal* to a private / loopback / link-local / cloud-metadata
  target: `127.0.0.0/8`, `::1`, `169.254.0.0/16` (incl. `169.254.169.254`),
  RFC-1918 (`10/8`, `172.16/12`, `192.168/16`), CGNAT `100.64/10`, IPv6 ULA
  `fc00::/7`, link-local `fe80::/10`, unspecified/broadcast/multicast; the
  `localhost` name and `metadata.google.internal` are denied by name.

`Url::parse` follows the WHATWG host parser for the special schemes, so the
classic SSRF-bypass encodings — decimal (`2130706433`), hex (`0x7f.0.0.1`), short
(`127.1`), IPv4-mapped IPv6 (`::ffff:127.0.0.1`), and `user@host` userinfo — are
normalised to their real address *before* classification and can't slip a
loopback past the screen.

Config (`[web]`): `allow_private = true` opts private/loopback targets back in for
local dev; `allow_hosts = ["*.internal", "127.0.0.1"]` are host globs that bypass
the screen entirely (explicit operator opt-in). **DNS-name → IP resolution** (a
public name that resolves to a private address, and re-screening each redirect
hop's resolved host) is a documented follow-up; the literal screen + the `local`
backend's bounded redirect policy are the first line today.

## Observability

- **Metrics** (`agent-metrics`, recorded by the `MeteredWeb` decorator):
  `agent_web_fetch_total{outcome}` (ok/error), `agent_web_fetch_seconds`,
  `agent_web_fetch_bytes`. Deliberately **not** labelled by host — the model is
  untrusted and chooses the URL, so a host label is an unbounded-cardinality
  Prometheus DoS vector; the host lands on the span instead.
  `agent_policy_guard_total{category="ssrf_target"}` counts SSRF denials.
- **Tracing:** a `web.fetch` span carrying `host` / `format` / `status` / `bytes`
  attributes (the span-attribute pattern from the telemetry work).

## Tests, bench, leak

- **SSRF screen:** a pure `scan_ssrf_target(url, allow_private, allow_hosts)`
  adversarial table in `policy.rs` (loopback/private/link-local/metadata + the
  obfuscated encodings + scheme denials + allow_private / allow_hosts opt-ins).
- **Tool:** a table over a one-shot backend double (format/MIME/cap/timeout/
  decode), plus html→markdown/text conversion tables, and an end-to-end
  guard+tool test (`metered.rs`) proving an SSRF target is denied and **never
  fetched** while a public one is fetched + converted.
- **Transport:** live `tiny_http` loopback tests for `LocalWebBackend` (body,
  redirect-follow, redirect-cap, oversize, scheme).
- **Span:** `captured_span_fields` asserts the `web.fetch` attributes.
- **Bench:** `benches/web.rs` — the HTML sanitizer over a fixed
  `agent_testkit::bench::html_document` fixture (deterministic Ir ceiling in
  `nix/checks/bench.nix`). The transport is I/O-bound → not benched.
- **Leak:** `tests/leak.rs` runs the fetch→sanitize→truncate path under dhat and
  asserts flat live blocks across iterations.
