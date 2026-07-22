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

The screen is **two layers**, sharing one IP classifier
(`agent_core::ip_is_private` — the single source of truth for "private / loopback
/ link-local / metadata / non-routable", covering `127.0.0.0/8`, `::1`,
`169.254.0.0/16` incl. `169.254.169.254`, RFC-1918 `10/8`·`172.16/12`·`192.168/16`,
CGNAT `100.64/10`, IPv6 ULA `fc00::/7`, link-local `fe80::/10`,
unspecified/broadcast/multicast, and IPv4-mapped IPv6):

1. **Pre-flight literal screen** — in the `Policy` `Guard`
   ([`agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs),
   new `ssrf_target` category). A fast, synchronous check of the *literal* URL:
   denies non-`http(s)` schemes and IP-literal private/loopback/metadata targets
   before any work, through the opaque
   `Decision::Deny("blocked by policy guard: …")` path (no resolved/responded
   oracle). `Url::parse` normalises the classic bypass encodings — decimal
   (`2130706433`), hex (`0x7f.0.0.1`), short (`127.1`), IPv4-mapped
   (`::ffff:127.0.0.1`), `user@host` userinfo — to their real address before
   classification. `localhost`, `metadata.google.internal`, `metadata.azure.com`
   are denied by name.
2. **Authoritative resolved-IP screen** — in the `local` transport
   ([`agent-web/src/local.rs`](../../crates/agent-web/src/local.rs)). Only the
   transport sees the *resolved* IP and every *redirect hop*, so this is where
   the real enforcement lives:
   - every hop's host is resolved (`getaddrinfo`) and refused if **any** resolved
     address is private — catching a **public DNS name that resolves to a private
     address**, which the literal screen cannot;
   - redirects are followed **manually** (reqwest auto-follow disabled) so the
     screen re-runs on every `Location` — a public URL that `302`s to
     `169.254.169.254` is refused **before** the next request is issued;
   - the checked IP is **pinned** to the connection (`Client::resolve`), so the
     IP screened is the IP connected to — defeating **DNS rebinding** (a TOCTOU
     re-resolve between check and connect).

Config (`[web]`): `allow_private = true` opts private/loopback targets back in for
local dev (both layers); `allow_hosts = ["*.internal", "127.0.0.1"]` are host
globs that bypass the screen (explicit operator opt-in). Residual note: the pin
uses the first screened address; hosts that resolve to a *set* mixing public and
private addresses are refused outright (any-private → deny), so there is no
partial-set bypass.

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
  redirect-follow, redirect-cap, oversize, scheme) + the resolved-IP screen:
  hermetic IP-literal unit tests, a loopback-refused-at-connect test, and the
  **redirect-to-metadata bypass regression** (a `302` to `169.254.169.254` is
  refused before the hop is issued).
- **Span:** `captured_span_fields` asserts the `web.fetch` attributes.
- **Bench:** `benches/web.rs` — the HTML sanitizer over a fixed
  `agent_testkit::bench::html_document` fixture (deterministic Ir ceiling in
  `nix/checks/bench.nix`). The transport is I/O-bound → not benched.
- **Leak:** `tests/leak.rs` runs the fetch→sanitize→truncate path under dhat and
  asserts flat live blocks across iterations.

## Over gRPC — a shared egress host

`[web] backend = "grpc"` routes fetches through a remote `WebService`
(`agent --serve-web`, default `127.0.0.1:50064`). Every outbound request then
leaves from **one** process, which makes the SSRF screen a property of *network
position* rather than of each agent's config — an agent with no egress of its own
can still fetch, through a host with exactly the reach it should have.

```toml
[web]
backend = "grpc"
[grpc.web]
endpoint = "http://egress:50064"
```

### Hosting egress remotely does not make its output trustworthy

The bytes still originate on the open internet, and the gateway itself may be the
thing misbehaving. So the client screens what comes back:

| Hostile input | Handling | Why |
|---|---|---|
| Body over the caller's `max_bytes` | Rejected locally | The cap protects *this* process's memory and context window; it cannot be delegated to the peer it is protecting against |
| HTTP status past `u16` | Saturates to `u16::MAX` | A wrapped `65736` reads as **`200`** — the model would be told a failed fetch succeeded |

**Failure semantic: hard.** An empty body on failure is indistinguishable from a
page that is genuinely empty, and the model would reason over the absence as if
it were evidence.

### One backend, not three

The web backend is now built once and shared between the `web_fetch` tool, the
`@url` reference route, and `--serve-web`. It used to be constructed per
consumer, so the SSRF screen was configured (identically) two or three times.

