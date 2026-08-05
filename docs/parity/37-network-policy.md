# Parity spec 37 — network egress policy / proxy

Per-feature parity spec for a **`NetworkPolicy` seam**: a first-class, deny-by-default
control over the network destinations a *sandboxed exec* command may reach — a
host/port allow-list plus an optional egress proxy — so that `bash` running inside an
isolation backend can talk to `github.com:443` but not to `169.254.169.254`, the
internal network, or an arbitrary attacker-named host, and every allow/deny decision
is metered, traced, and dialable like every other agent-seddon seam.

> **Status: ⬜ spec written, not started.** Proposed new **`NetworkPolicy` seam**
> (`agent_core::NetworkPolicy`: `async fn decide(&self, target: &EgressTarget) ->
> EgressDecision` + `capabilities()`), with config-selected backends in a new
> `agent-netpolicy` crate: `allowlist` (an in-process
> host/port allow-list evaluated against the **resolved** IP) and `proxy` (a local
> HTTP+SOCKS5 egress proxy that enforces the same allow-list on the wire). Selected by
> `[network] backend = "allowlist" | "proxy" | "grpc"` and applied to **sandboxed**
> exec (spec 14): the sandbox executor is handed a `NetworkPolicy` handle and either
> (a) confines the child to `network: Off` + a loopback proxy, or (b) screens each
> connect. The **differentiator**: this is the first egress control that is an
> *inspectable seam* — a gRPC service with reflection (`--serve-network`), a
> `agent_network_egress_total{outcome}` counter + `network.decide` OTel span **per
> decision**, and a dhat leak gate — **and** it reuses the exact two-layer SSRF guard
> that already ships for `web_fetch` ([spec 11](11-web-fetch.md)) as its resolved-IP
> screen, so a name that resolves to a private/link-local range is refused on the same
> code path whether it arrives via `web_fetch` or via a sandboxed `curl`. Cross-refs:
> [`11-web-fetch.md`](11-web-fetch.md) (the SSRF / private-IP guard this reuses),
> [`14-sandbox.md`](14-sandbox.md) + [`34-…`](34-os-sandbox.md) (the
> isolation boundary this policy is enforced *inside* of). **Deferred:** HTTPS MITM /
> "limited" read-only mode (codex's `mode = "limited"`), credential-brokering to
> upstream hosts, per-call policy amendment / interactive approval of a blocked host,
> and OS-level enforcement wiring (seatbelt/landlock) — the allow-list + loopback
> proxy ship first; the sandbox teeth that make bypass impossible are the spec-14/34
> follow-up. **Unimplemented** — the `NetworkPolicy` trait, either backend, the proto
> service, and the sandbox wiring do not exist yet; this is the design of record.

## Feature & why it matters

agent-seddon's `bash` is the intentional **unconfined escape hatch** (parity
[04](04-shell-bash.md), [14](14-sandbox.md)): a sandboxed command today can open a
socket to anywhere the host can reach. The `web_fetch` tool is guarded — [spec
11](11-web-fetch.md) ships a two-layer SSRF screen that denies loopback / link-local /
RFC-1918 / cloud-metadata destinations *before the socket opens* and re-screens every
redirect hop — but that guard governs **exactly one tool**. The moment the model runs
`curl`, `wget`, `git fetch`, `pip install`, or a build script's `npm ci` under `bash`,
it is back on the open network with no destination control at all.

That is the gap this seam closes. A prompt-injected model that has read a hostile
README can drive a sandboxed `bash` to:

- **exfiltrate cloud IAM credentials** — `curl http://169.254.169.254/latest/meta-data/…`
  from inside a cloud VM;
- **pivot into the private network** — reach an RFC-1918 service, a loopback-bound
  admin port, a `.internal` name;
- **phone home** — POST secrets to an attacker-named public host.

`web_fetch`'s per-request SSRF check cannot see any of this, because the request never
goes through the tool. What is missing is a control on the **network egress decision
itself** — one that is asked *by the sandbox executor* on every outbound connection a
confined command attempts, that is **deny-by-default** (nothing reachable until an
allow-list names it), and that screens the **resolved IP**, not just the literal, so a
public name that resolves private is still refused. Making that decision a **seam** —
not a hard-coded `if` in the sandbox — means it is config-selectable (in-process
allow-list vs. a real egress proxy vs. a remote gRPC decider), swappable, and — the
agent-seddon signature — **inspectable**: one metric and one span per allow/deny, so an
operator can audit exactly what a session tried to reach and what was refused.

## agent-seddon today

**Absent as a general seam.** There is no network-egress control for sandboxed exec.
Concretely:

- **`web_fetch` is guarded; nothing else is.** [spec 11](11-web-fetch.md) is
  implemented: a `Policy`-wired pre-flight (`scan_ssrf_target` in
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)) plus
  an authoritative resolved-IP screen in the `local` transport
  ([`crates/agent-web/src/local.rs`](../../crates/agent-web/src/local.rs)) that resolves
  every redirect hop, refuses any host resolving to a private address, follows redirects
  manually so the screen re-runs per `Location`, **pins the checked IP to the
  connection** (defeats DNS rebinding), normalises obfuscated-IP encodings, and honours
  `[web] allow_private` / `allow_hosts`. This is the machinery to reuse — but it only
  fires for the `web_fetch` tool.
- **Sandboxed `bash` reaches the network freely.** [spec 14](14-sandbox.md) shipped the
  `Sandbox` seam with `local` + `nix` backends, but the `local` backend is an
  unconfined spawn and the shipped `nix` mode is the *dev-shell* mode, not the
  network-off sandboxed-derivation mode. `ExecSpec` reserves a `network: Off|On|Loopback`
  field, but there is **no host/port allow-list, no egress proxy, and no seam** that
  owns "may this confined command reach host `h` port `p`?". Absent a future OS sandbox
  ([spec 34](34-os-sandbox.md)) that blocks the network *wholesale*, a
  sandboxed command has full egress.
- **The `ip_is_private` classifier is already shared and reusable.** The SSRF screen and
  the guard share one IP classifier (`agent_core::ip_is_private`) — the same predicate a
  `NetworkPolicy` resolved-IP screen needs. No new classifier; reuse it.
- **The seam-over-gRPC harness is the template.** `SearchBackend` / `WebBackend` /
  `Sandbox` each are an async trait in `agent-core`, an impl in a sibling crate behind a
  cargo feature, a factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs), a metered decorator
  in [`metered.rs`](../../crates/agent-runtime/src/metered.rs), a `<seam>.proto` with a
  `--serve-<seam>` server + reflection, and a roundtrip test in `agent-grpc/tests/`.

Honest gap: everything above is *reusable scaffolding*. The `NetworkPolicy` trait, the
allow-list backend, the egress-proxy backend, the sandbox wiring that actually asks the
policy on connect, the proto service, and the `--serve-network` CLI **do not exist yet**.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/network-proxy/` (crate: `policy.rs`, `connect_policy.rs`, `proxy.rs`, `socks5.rs`, `mitm.rs`, `config.rs`, `network_policy.rs`), `codex-rs/core/src/network_policy_decision.rs`, `codex-rs/exec-server/src/network_policy_decisions.rs`, `codex-rs/sandboxing/src/seatbelt_network_policy.sbpl` + `landlock.rs` + `violation.rs` | `codex-rs/network-proxy/src/policy.rs` (`mod tests`), `.../connect_policy.rs` (`mod tests`), `.../upstream_tests.rs`, `.../mitm_tests.rs`, `.../credential_broker_tests.rs`, `.../remote_config_tests.rs`, `codex-rs/core/src/network_policy_decision_tests.rs`, `codex-rs/exec-server/src/network_policy_decisions_tests.rs`, `codex-rs/exec-server-protocol/src/network_policy_tests.rs` | cargo `#[test]` / insta |
| hermes-agent | `tools/environments/docker.py` (`--network=none` air-gap toggle + air-gap verify) | `tests/tools/test_docker_environment.py` | pytest |
| opencode | — (no network-egress control; the permission gate decides whether `bash` runs at all, not which hosts it may reach — and its `webfetch` has no SSRF screen either, per [spec 11](11-web-fetch.md)) | — | — |
| pi | — (no in-repo egress policy; network controls are delegated to the external **NVIDIA OpenShell** gateway, `docs/containerization.md` — docs only, no impl or tests in the repo) | — | — |

**codex** is the deep anchor — a purpose-built local **network policy enforcement
proxy** (`codex-network-proxy`) and a decision layer on top of it, pinning exactly the
behaviours we want:

- **Deny-by-default host allow-list** (`network-proxy/README.md`, `policy.rs`
  `compile_allowlist_globset` / `compile_denylist_globset`): domains are matched against
  an allow/deny globset; *"If no domain entries are marked `allow`, the proxy blocks
  requests until an allowlist is configured."* Exact hosts or scoped wildcards
  (`*.openai.com`, `**.openai.com`) — **the global `*` wildcard is rejected**
  (`GlobalWildcard::Allow` is opt-in only for allowlist compilation, and `*` is refused
  as a footgun). Tests in `policy.rs mod tests` (`compile_denylist_globset` cases:
  trailing-dot hosts, wildcard scoping, IPv6 literals `[::1]` / `[fe80::1%25lo0]`).
- **Resolved-IP screen — the spec-11 analogue** (`policy.rs` `is_non_public_ip`,
  `connect_policy.rs` `TargetCheckedStreamConnector::connect`): before connecting, if the
  **resolved address** is non-public and `allow_local_binding = false`, the connect is
  refused with `PermissionDenied("network target rejected by policy")`. Test
  `is_non_public_ip_rejects_private_and_loopback_ranges` pins the full range set
  (loopback, RFC-1918, CGNAT `100.64/10`, `192.0.0/24`, TEST-NET, `198.18/15`,
  `240/4`, `0/8`, IPv4-mapped `::ffff:…`, `::1`, `fe80::`, `fc00::`).
- **Checked-IP pinned to the connected IP — DNS-rebinding defense** (`connect_policy.rs`
  `target_matches_non_public_addr`): *"Hostnames that resolve to local/private IPs are
  still blocked even if allowlisted."* An allowlisted hostname does **not** launder a
  private resolved address — the allow decision only stands if the resolved IP matches
  the checked host. Test `resolved_private_address_does_not_match_allowlisted_hostname`
  asserts `example.com` resolving to `127.0.0.1` is **not** matched. `mitm.rs` (~line
  381) re-resolves between CONNECT and the inner HTTPS request *"to defend against DNS
  rebinding"* — the every-hop re-screen, exactly like spec 11's manual redirect follow.
- **The proxy itself** (`proxy.rs`, `socks5.rs`, `config.rs`): an HTTP proxy
  (`127.0.0.1:3128`) + a SOCKS5 proxy (`127.0.0.1:8081`), non-loopback binds clamped to
  loopback unless `dangerously_allow_non_loopback_proxy` is set; `mode = "full"` default
  vs. `"limited"` read-only (MITM). Spawned commands are pointed at it via
  `HTTP(S)_PROXY`. Upstream/MITM behaviour tested in `upstream_tests.rs`, `mitm_tests.rs`.
- **Decision layer + typed reasons** (`core/src/network_policy_decision.rs`,
  `exec-server/src/network_policy_decisions.rs`): `NetworkPolicyDecision::{Allow, Deny,
  Ask}` with a `NetworkDecisionSource`; `denied_network_policy_message(blocked)` turns a
  `BlockedRequest` into an operator message. Tests
  (`network_policy_decision_tests.rs`, `exec-server/src/network_policy_decisions_tests.rs`,
  `exec-server-protocol/src/network_policy_tests.rs`) cover deny-message formatting,
  denylist-block explicitness, protocol mapping (HTTP/HTTPS/SOCKS), and ask-from-decider
  gating.
- **OS-level enforcement, so the proxy can't be bypassed** (`sandboxing/`):
  `seatbelt_network_policy.sbpl` (macOS seatbelt — network denied except the injected
  proxy allow rules) + `landlock.rs` (Linux) + `violation.rs`
  `record_network_sandbox_violation`. The README is explicit that clients which bypass
  proxies for loopback (Go's `net/http`) *"remain blocked by the operating-system
  sandbox when local binding is disabled"* — the proxy is the policy, the OS sandbox is
  the teeth.

**hermes-agent** offers only a coarse air-gap: the docker backend
([`tools/environments/docker.py`](../../../hermes-agent/tools/environments/docker.py))
takes a `network: bool` and appends `--network=none` (~line 643) for a wholesale
disconnect, with an air-gap *verification* step (~line 904-921) that re-checks the
container's network mode and errors on mismatch. Test
([`test_docker_environment.py`](../../../hermes-agent/tests/tools/test_docker_environment.py))
threads the `network` flag. This is **on/off**, not a host/port allow-list or a proxy —
a second, much shallower data point.

**opencode** and **pi** have no in-repo egress policy — marked "—". opencode's
permission gate decides whether `bash`/`edit` run at all, not which hosts they reach;
pi delegates "network controls" to the external NVIDIA OpenShell gateway
(`docs/containerization.md`), documentation with no impl or tests in the tree. This is a
feature where codex is the deep anchor, hermes a coarse second point, and agent-seddon
can leapfrog both on **distribution** (gRPC + reflection decider) and **observability**
(a metric + span per egress decision), while **reusing** the spec-11 SSRF guard so the
resolved-IP screen is not reinvented.

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`NetworkPolicy` seam.** New async trait in `agent-core`:
  `decide(target: &EgressTarget) -> EgressDecision` (where `EgressTarget` carries the
  requested host, port, protocol, and the **resolved IP** once known) returning
  `Allow` / `Deny{reason}`; plus `capabilities()` (does this backend enforce on the wire,
  or only advise?). Impls in a new sibling `agent-netpolicy` crate behind cargo features
  (`netpolicy-allowlist`, `netpolicy-proxy`); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected via
  `[network] backend`. (Port codex `NetworkPolicyDecision`.)
- **Deny-by-default host/port allow-list.** With no `allow` entries, **everything is
  denied**; a request is permitted only if its host matches an allow glob (exact host or
  scoped wildcard `*.github.com`) **and** its port is in the allowed set, and does not
  match a deny glob. The **global `*` wildcard is rejected** at config-compile time (a
  footgun, exactly as codex refuses it). (Port codex `compile_allowlist_globset` /
  README deny-by-default.)
- **Resolved-IP screen — reuse the spec-11 guard.** The allow decision is evaluated
  against the **resolved** IP via the *same* `agent_core::ip_is_private` classifier and
  the same normalisation the `web_fetch` transport uses: a host resolving to loopback /
  link-local (`169.254/16`, `fe80::/10`) / cloud-metadata (`169.254.169.254`,
  `fd00:ec2::254`) / RFC-1918 / ULA is **denied even if the host string is
  allowlisted**, unless `[network] allow_private = true`. This is the differentiator —
  one SSRF code path shared between `web_fetch` and sandboxed egress. (Reuse spec 11;
  port codex `is_non_public_ip` + `target_matches_non_public_addr`.)
- **Checked-IP pinned to the connected IP (DNS rebinding).** The IP the policy screened
  must be the IP the socket connects to — resolve once, screen, then connect to *that*
  address, so a name that passes the screen and then re-resolves to a private address
  between check and connect cannot slip through. For the proxy backend, re-resolve and
  re-screen on every CONNECT / redirect hop. (Port codex `connect_policy` pinning +
  `mitm.rs` re-resolve; mirror spec 11's per-hop re-screen.)
- **Egress-proxy backend.** A local HTTP + SOCKS5 proxy bound to loopback that enforces
  the allow-list on the wire; sandboxed commands are pointed at it via `HTTP(S)_PROXY` /
  `ALL_PROXY`. Non-loopback binds refused unless explicitly opted in. (Port codex
  `proxy.rs` / `socks5.rs`; the `allowlist` backend is the in-process advisor, the
  `proxy` backend is the on-the-wire enforcer.)
- **Sandbox integration ([spec 14](14-sandbox.md) / [34](34-os-sandbox.md)).**
  The `Sandbox` executor is handed the `NetworkPolicy` and either (a) sets the child to
  `network: Off` and injects the loopback proxy env, or (b) consults `decide()` per
  connect. Absent OS teeth the proxy is advisory (bypassable by a proxy-ignoring client)
  — honestly documented, exactly as codex leans on seatbelt/landlock so a Go client
  can't bypass it. (New — no peer offers a *swappable, served* egress policy inside its
  sandbox seam.)
- **Non-disclosure on denial.** A denied egress returns an opaque
  `blocked by network policy` reason that does not reveal whether the host resolved or
  what it resolved to (matches the `Policy` "no why-oracle" convention and spec 11's
  opaque `blocked by policy guard`). (Port codex `denied_network_policy_message` opacity;
  reuse spec 08/11.)
- **Metered decision + per-decision span (differentiator).** An
  `agent_network_egress_total{outcome=allow|deny_host|deny_port|deny_private|deny_scheme}`
  counter and a `network.decide` OTel span (attrs `host`, `port`, `protocol`, `resolved`
  (private-bit only, never the raw IP as a metric label — cardinality/PII), `outcome`)
  reusing [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) +
  [`agent-telemetry`](../../crates/agent-telemetry/). Metrics are **not** labelled by
  host (untrusted → cardinality DoS); host is a span attribute only. (New — no peer
  emits a metric/span per egress decision.)
- **gRPC service.** `network.proto` with a `Decide(EgressTarget) -> EgressDecision`
  unary RPC (+ a `Capabilities` RPC), reflection, `--serve-network`; a remote decider is
  dialable like any other seam, so a central policy service can rule on many agents'
  egress. (New — codex's decider is in-process; ours is remotable.)

## Table-driven test plan

Two layers, both `#[rstest]` `#[case::…]`, mirroring `policy.rs` / spec 11's SSRF
table:

1. **Allow-list + resolved-IP screen** — a pure `decide(target, config) -> EgressDecision`
   unit table in `agent-netpolicy` (no real socket): the destination host/port/protocol
   and a **stubbed resolver** (host → canned IP) drive the decision, so DNS-rebinding and
   resolves-private cases are deterministic. This is the leak-free, network-free layer.
2. **Proxy enforcement + sandbox wiring** — a `#[tokio::test]` over a loopback proxy
   (`tiny_http`-class) with `[network] allow_private = true`, asserting an allowed host
   tunnels and a denied host is refused on the wire, plus a sandbox-integration case that
   a confined `curl` to a denied host fails *inside* the boundary.

Destinations are **untrusted** (a prompt-injected model picks them), so **`adversarial_`
cases are mandatory** and reuse the spec-11 framing — DNS rebinding, obfuscated-IP
encodings, a redirect/CONNECT to a private range, and a name that resolves private all
**must be refused**, with the checked IP pinned against the connected IP. Doubles from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs): a `StubResolver` (host → IP,
so `resolves-private` is deterministic) and `captured_spans` for the span assertion.
Prefixes: `positive_` allowed, `negative_` denied, `corner_` odd-but-valid,
`boundary_` at a limit; every case tagged `(port: codex)` or `(new: agent-seddon)`.

```rust
// ---- layer 1: allow-list + resolved-IP screen (pure, StubResolver) ----------
// `decide(host, port, proto, resolver, cfg) -> EgressDecision`.
// Ok ⇒ Allow; Err(substr) ⇒ Deny whose opaque reason contains `substr`.
#[rstest]
// --- deny-by-default: nothing allowed until the allow-list names it ----------
#[case::negative_empty_allowlist_denies_all(                                     // (port: codex "blocks requests until an allowlist is configured")
    "github.com", 443, cfg(/*allow=*/&[]),               stub("github.com","140.82.121.4"), Err("blocked by network policy"))]
#[case::positive_allowlisted_host_port(                                          // (port: codex allow glob)
    "github.com", 443, cfg(&["github.com:443"]),         stub("github.com","140.82.121.4"), Ok(()))]
#[case::positive_scoped_wildcard(                                                // (port: codex "*.openai.com")
    "api.openai.com", 443, cfg(&["*.openai.com:443"]),   stub("api.openai.com","1.2.3.4"),  Ok(()))]
#[case::negative_port_not_allowed(                                              // (new: agent-seddon) host ok, port denied
    "github.com", 22, cfg(&["github.com:443"]),          stub("github.com","140.82.121.4"), Err("blocked by network policy"))]
#[case::negative_host_not_in_allowlist(                                         // (port: codex denylist/allowlist miss)
    "evil.example", 443, cfg(&["github.com:443"]),       stub("evil.example","5.6.7.8"),    Err("blocked by network policy"))]
#[case::negative_global_wildcard_rejected_at_compile(                           // (port: codex "global * wildcard is rejected")
    "anything", 443, cfg(&["*:443"]),                    stub("anything","9.9.9.9"),        Err("invalid allowlist"))]
// --- resolved-IP screen (reuse spec-11 guard) --------------------------------
#[case::negative_cloud_metadata(                                                // (new: agent-seddon; reuse spec 11)
    "169.254.169.254", 80, cfg(&["169.254.169.254:80"]), stub("169.254.169.254","169.254.169.254"), Err("blocked by network policy"))]
#[case::negative_loopback_literal(                                              // (port: codex is_non_public_ip)
    "127.0.0.1", 8080, cfg(&["127.0.0.1:8080"]),         stub("127.0.0.1","127.0.0.1"),     Err("blocked by network policy"))]
#[case::negative_rfc1918(                                                       // (port: codex is_non_public_ip)
    "10.0.0.5", 443, cfg(&["10.0.0.5:443"]),             stub("10.0.0.5","10.0.0.5"),       Err("blocked by network policy"))]
#[case::corner_allow_private_opt_in(                                            // (new: agent-seddon) [network] allow_private=true
    "127.0.0.1", 8080, cfg_private(&["127.0.0.1:8080"]), stub("127.0.0.1","127.0.0.1"),     Ok(()))]
// --- scheme/protocol gating --------------------------------------------------
#[case::negative_disallowed_protocol(                                           // (port: codex protocol mapping)
    "github.com", 25, cfg_proto(&["github.com:25"], Smtp), stub("github.com","140.82.121.4"), Err("blocked by network policy"))]
fn decide_cases(
    #[case] host: &str, #[case] port: u16, #[case] cfg: NetCfg,
    #[case] resolver: StubResolver, #[case] expect: Result<(), &str>,
) { /* decide(host,port,proto,resolver,cfg) → assert Allow / opaque-Deny(substr) */ }

// ---- adversarial: destinations are attacker-controlled (MANDATORY) ----------
// Reuses spec-11 framing: rebinding, obfuscated IPs, redirect-to-private, name→private.
#[rstest]
#[case::adversarial_dns_rebinding_checked_ip_pinned(                            // (new: agent-seddon; port codex connect_policy pinning)
    // resolver returns PUBLIC on the screening lookup then PRIVATE on reconnect:
    // the checked IP is PINNED to the connected IP, so the swap is refused.
    "rebind.evil", 443, cfg(&["rebind.evil:443"]),
    rebind_stub("rebind.evil", /*checked=*/"8.8.8.8", /*reconnect=*/"127.0.0.1"),
    Err("blocked by network policy"))]
#[case::adversarial_name_resolves_private(                                      // (port: codex "hostnames that resolve to private IPs are still blocked even if allowlisted")
    "internal.evil", 443, cfg(&["internal.evil:443"]),   stub("internal.evil","10.1.2.3"),  Err("blocked by network policy"))]
#[case::adversarial_obfuscated_decimal_ip(                                      // (new: agent-seddon; reuse spec-11 obfuscated-IP normalisation)
    "2130706433", 80, cfg(&["2130706433:80"]),           stub("2130706433","127.0.0.1"),    Err("blocked by network policy"))]  // 2130706433 == 127.0.0.1
#[case::adversarial_ipv4_mapped_v6(                                             // (port: codex ::ffff:127.0.0.1)
    "[::ffff:127.0.0.1]", 80, cfg(&["[::ffff:127.0.0.1]:80"]), stub("[::ffff:127.0.0.1]","::ffff:127.0.0.1"), Err("blocked by network policy"))]
#[case::adversarial_userinfo_host_confusion(                                    // (new: agent-seddon; reuse spec-11 userinfo normalisation)
    "169.254.169.254", 80, cfg(&["trusted.example:80"]), stub_userinfo("trusted.example@169.254.169.254","169.254.169.254"), Err("blocked by network policy"))]
fn adversarial_egress_cases(
    #[case] host: &str, #[case] port: u16, #[case] cfg: NetCfg,
    #[case] resolver: StubResolver, #[case] expect: Result<(), &str>,
) {
    // EVERY case denies. The checked IP is pinned to the connected IP; a name that
    // resolves private, an obfuscated encoding, a redirect/reconnect to a private
    // range, and userinfo host-confusion all refuse. Opaque reason — no why-oracle.
}

// ---- redirect / CONNECT to a private range is re-screened every hop ---------
#[tokio::test]
async fn adversarial_redirect_to_private_re_screened() {                        // (port: codex mitm.rs re-resolve; reuse spec-11 per-hop)
    // proxy backend: allowlisted public host issues a 302 / CONNECT to 169.254.169.254.
    // the post-redirect target is re-resolved and re-screened → refused mid-chain.
}

// ---- layer 2: proxy enforcement + sandbox wiring ----------------------------
#[tokio::test]
async fn positive_proxy_tunnels_allowlisted_host() {                            // (port: codex proxy.rs)
    // loopback proxy, [network] allow_private=true, allow=["127.0.0.1:<p>"]:
    // a request to the allowed loopback server tunnels; egress_total{outcome="allow"} += 1.
}
#[tokio::test]
async fn negative_sandboxed_curl_to_denied_host_fails_inside_boundary() {       // (new: agent-seddon; cf. spec 14)  requires: nix|bwrap
    // spec-14 sandbox with network:Off + injected loopback proxy; a confined
    // `curl https://evil.example` fails INSIDE the boundary (not a string check),
    // egress_total{outcome="deny_host"} += 1. Guarded by an availability probe.
}
#[tokio::test]
async fn network_decide_emits_span_with_attributes() {                          // (new: agent-seddon)
    // captured_spans finds `network.decide` with host/port/protocol/outcome attrs;
    // the resolved IP is NOT a metric label (cardinality/PII), only a span attr.
}
```

gRPC roundtrip (extend [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)):
`Decide` an allowed and a denied `EgressTarget` over the wire (TCP + UDS), asserting the
served decider returns the same `Allow` / opaque-`Deny` as the in-process seam — the
pattern every other seam's roundtrip test uses (the point is the decision survives the
seam identically in-process vs. served).

Prefix legend (repo convention): `positive_` expected allow, `negative_` expected deny,
`corner_` odd-but-valid, `boundary_` at a limit. `adversarial_` cases (mandatory for
untrusted destinations) assert the **rejection** of rebinding / obfuscated-IP /
redirect-to-private / name-resolves-private and that the checked IP is pinned to the
connected IP. `(port: codex)` names the peer a case was mined from; `(new:
agent-seddon)` marks the port-granularity, allow-private opt-in, per-decision
metric/span, served-decider, and sandbox-integration invariants that have no peer
analogue.

## Harness obligations

The implementing PR(s) must satisfy all (follows #21–45, matching specs 11/14):

- **Seam + registry:** `NetworkPolicy` trait in `agent-core`; `allowlist` + `proxy`
  impls in a new `agent-netpolicy` crate behind cargo features (`netpolicy-allowlist`,
  `netpolicy-proxy`); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) (config `[network]
  backend = "allowlist" | "proxy" | "grpc"`); a `MeteredNetworkPolicy` in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs). **Reuse** the spec-11 SSRF
  machinery — `agent_core::ip_is_private` + the obfuscated-IP normalisation — as the
  resolved-IP screen rather than reimplementing it. The `Sandbox` executor
  ([spec 14](14-sandbox.md)) gains a `NetworkPolicy` handle on its exec path. Doc in
  `docs/components/network-policy.md`.
- **Proto + gRPC:** `crates/agent-proto/proto/agent/v1/network.proto`
  (`NetworkService.Decide(EgressTarget) -> EgressDecision` + a `Capabilities` RPC) +
  `build.rs` entry + server/client in `agent-grpc` + `--serve-network` + reflection;
  extend the gRPC roundtrip test; commit the `buf.image.binpb` bump (`nix run
  .#buf-image`); add the endpoint/port to `nix/constants.nix` (`nix run
  .#gen-constants`).
- **Metrics + OTel:** `agent_network_egress_total{outcome}` counter (outcomes
  `allow`/`deny_host`/`deny_port`/`deny_private`/`deny_scheme`) in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) — **not** labelled by host
  (untrusted → cardinality DoS); a `network.decide` span carrying
  `host`/`port`/`protocol`/`resolved`(private-bit)/`outcome` attributes reusing
  [`agent-telemetry`](../../crates/agent-telemetry/) — the per-decision differentiator.
- **Bench:** the allow-list + resolved-IP `decide()` path is a **pure, deterministic**
  CPU hot path (globset match + IP classification, no I/O) — a candidate for an
  iai-callgrind bench over a fixed allow-list + target fixture in
  `agent-testkit::bench`, with an Ir ceiling in `nix/checks/bench.nix`. The proxy
  transport itself is I/O-bound — **not** benched (documented skip, as `bash`/`web`
  transport did).
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over the compile-allowlist →
  resolve → screen → decide path, asserting the decision path frees its buffers within
  an allocation budget under a burst of decisions (the globset + per-decision allocation
  must not leak).

## References

- **agent-seddon (to extend / reuse):**
  [`crates/agent-runtime/src/policy.rs`](../../crates/agent-runtime/src/policy.rs)
  (`scan_ssrf_target`, `ssrf_target` category — the spec-11 pre-flight the egress screen
  parallels),
  [`crates/agent-web/src/local.rs`](../../crates/agent-web/src/local.rs)
  (the authoritative resolved-IP screen: resolve-every-hop, pin checked IP, obfuscated-IP
  normalisation — the machinery to share),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  (`ip_is_private` classifier, `Tool`/`ToolContext`),
  [`crates/agent-tools/src/core.rs`](../../crates/agent-tools/src/core.rs) (`BashTool` —
  the unconfined egress this seam constrains under a sandbox),
  [`crates/agent-proto/proto/agent/v1/search.proto`](../../crates/agent-proto/proto/agent/v1/search.proto)
  (seam-over-gRPC template),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)
  (metered-seam pattern),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (counters to
  extend), [`crates/agent-telemetry/`](../../crates/agent-telemetry/) (per-decision span),
  [`crates/agent-grpc/tests/roundtrip.rs`](../../crates/agent-grpc/tests/roundtrip.rs)
  (roundtrip pattern),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`,
  home of the new `StubResolver`);
  dependencies: [`11-web-fetch.md`](11-web-fetch.md) (the SSRF / private-IP guard this
  reuses), [`14-sandbox.md`](14-sandbox.md) + [`34-…`](34-os-sandbox.md)
  (the isolation boundary this policy is enforced inside),
  [`08-permissions-policy.md`](08-permissions-policy.md) (the no-why-oracle deny
  convention), [`04-shell-bash.md`](04-shell-bash.md) (bash unconfined by design).
- **codex (anchor):** `codex-rs/network-proxy/` — `policy.rs` (`is_non_public_ip`,
  `compile_allowlist_globset` / `compile_denylist_globset`, global-`*` rejection),
  `connect_policy.rs` (`TargetCheckedStreamConnector::connect`,
  `target_matches_non_public_addr` — checked-IP pin / DNS-rebinding defense),
  `proxy.rs` + `socks5.rs` (HTTP `:3128` + SOCKS5 `:8081` enforcing proxy),
  `mitm.rs` (re-resolve between CONNECT and inner HTTPS), `config.rs`
  (`allow_local_binding`, `mode = full|limited`, domains allow/deny map), `README.md`
  (deny-by-default semantics); `codex-rs/core/src/network_policy_decision.rs` +
  `codex-rs/exec-server/src/network_policy_decisions.rs` (decision layer,
  `denied_network_policy_message`); `codex-rs/sandboxing/src/seatbelt_network_policy.sbpl`
  + `landlock.rs` + `violation.rs` (OS-level teeth);
  tests: `network-proxy/src/policy.rs` `mod tests`
  (`is_non_public_ip_rejects_private_and_loopback_ranges`, `compile_denylist_globset`
  cases), `connect_policy.rs` `resolved_private_address_does_not_match_allowlisted_hostname`,
  `upstream_tests.rs`, `mitm_tests.rs`, `credential_broker_tests.rs`,
  `remote_config_tests.rs`, `core/src/network_policy_decision_tests.rs`,
  `exec-server/src/network_policy_decisions_tests.rs`,
  `exec-server-protocol/src/network_policy_tests.rs`.
- **hermes-agent:** `tools/environments/docker.py` (`--network=none` air-gap toggle +
  air-gap verify, ~line 643 / 904-921); test `tests/tools/test_docker_environment.py`
  (threads the `network` flag). Coarse on/off, not a host/port allow-list or proxy.
- **opencode:** — (no network-egress control; the permission gate decides whether `bash`
  runs, not which hosts it reaches; `webfetch` has no SSRF screen either).
- **pi:** — (no in-repo egress policy; delegated to the external NVIDIA OpenShell
  gateway, `packages/coding-agent/docs/containerization.md` — docs only, no impl/tests).
