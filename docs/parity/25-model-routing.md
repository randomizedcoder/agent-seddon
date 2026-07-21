# Parity spec 25 — model routing / fallback

Per-feature parity spec for a **`Router` seam** that fronts several
`LlmProvider`s: pick a provider per request by capability / cost / latency and
**fail over** to an alternate on a retryable error (rate-limit, 5xx, timeout).
Tracks what agent-seddon ships today, what the peer agents assert, and the
concrete behaviour + tests needed to be the most complete of the four.

> **Status: spec (design of record).** A new **`Router`** that **IS-A
> `LlmProvider`** (composes N inner `LlmProvider`s behind the same
> `agent-core` trait) and is selected by config like any other provider. It
> adds (a) a **routing policy** — capability-match / cost-min / latency /
> explicit-per-task — that picks a primary, and (b) **failover** — on a
> *classified retryable* error it advances to the next candidate with bounded
> retries + backoff, short-circuiting a **circuit-breaker** for a candidate that
> is currently unhealthy. **Differentiator:** the routing decision and the
> fallback count are **metered** (route-decision counter by target, fallback
> counter, per-target latency) and each candidate is *still* an independent
> seam — including a `= "grpc"` client — so a single `Router` can route across
> **local and remote** providers over the wire. No peer composes provider
> selection *and* remote-seam transport behind one uniform trait. This is the
> design of record; nothing here is implemented yet.

## Feature & why it matters

A coding agent that speaks to exactly one provider inherits that provider's
worst day: a 429 storm, a regional 5xx, or a stream that dies mid-turn aborts
the run. **Routing + fallover** makes the model layer resilient — a classified
transient failure on the primary transparently retries on a secondary, so the
turn completes instead of surfacing an error to the user.

Beyond resilience, a router is where **cost and latency** are optimised: send
the hard turn to the big expensive model and the easy turn (a summary, a title,
a compaction pass) to a cheap fast one; prefer the low-latency local model and
fall back to the remote frontier model only when the task needs capabilities the
local one lacks (larger context window, tool support). The selection is a policy
chosen by config, not a code edit — the same binary is cheap-by-default and
frontier-when-needed.

Concretely the router must:

- **choose** a primary from N candidates by a declared policy (capability match,
  cost-minimising, latency, or an explicit per-task pin);
- **fail over** on a *retryable* error only (rate-limit / 5xx / overload /
  timeout / dropped stream), never on a *terminal* one (auth, bad-request,
  content-policy, quota-exhausted) — with **bounded** retries and **backoff**;
- **skip** a candidate that a **circuit-breaker** has marked unhealthy after
  repeated failures, and recover it after a cooldown;
- **observe** every decision (which target, why) and every fallover as metrics
  + a span, so operators can see route mix and failure rates.

## agent-seddon today

**Absent.** Exactly **one** provider is selected by config and used directly;
there is no composition, routing, or fallover across providers.

- **Trait:** [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs)
  — `pub trait LlmProvider { fn capabilities(&self) -> ModelCapabilities; async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>; async fn stream(...) }`.
  `ModelCapabilities { supports_tools: bool, context_window: u32 }` already
  exists — the only capability axes a router could match on today (no cost or
  latency metadata).
- **Impls:** [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs),
  [`crates/agent-providers/src/openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs).
- **Wiring:** [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs)
  registers `openai-compat` (line ~303), `anthropic` (~305), and a `grpc`
  provider client (~419); `build_provider(name, cfg)` looks up **one** factory
  by the `[agent] provider` config string and returns a single `Arc<dyn LlmProvider>`.
- **gRPC:** the provider seam is already served — service `Provider` in
  [`crates/agent-proto/proto/agent/v1/provider.proto`](../../crates/agent-proto/proto/agent/v1/provider.proto)
  (`Capabilities` / `Complete` / `Stream`), served via `--serve provider`
  ([`crates/agent-cli/src/grpc_server.rs`](../../crates/agent-cli/src/grpc_server.rs),
  `Seam::Provider`), and dialable by the `grpc` provider factory. A `Router`
  therefore needs **no new proto** — each candidate is a `Complete`/`Stream`
  client already.
- **Metrics:** [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs)
  wraps the single provider — `on_provider_request(name, streaming, secs)` and
  `on_provider_error(name, op)`. There is **no** route-decision or fallover
  counter.

Honest gaps: no `Router` trait/impl, no candidate list in config, no error
**classification** (a provider error is opaque `anyhow::Error`; nothing decides
retryable vs terminal), no backoff/retry budget across providers, no
health/circuit-breaker state, and no routing metrics. The runtime assumes a
single provider end to end.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| hermes-agent | `hermes-agent/agent/error_classifier.py` (`FailoverReason`, `classify_api_error`), `hermes-agent/run_agent.py` (`fallback_model`, `switch_model`), `hermes-agent/agent/retry_utils.py` (`jittered_backoff`) | `hermes-agent/tests/agent/test_error_classifier.py`, `hermes-agent/tests/run_agent/test_auth_provider_failover.py`, `.../test_24996_fallback_exhaustion_cooldown.py`, `.../test_31273_402_not_retried.py`, `hermes-agent/tests/test_retry_utils.py` | pytest |
| pi | `pi/packages/ai/src/utils/retry.ts` (`isRetryableAssistantError`), `pi/packages/coding-agent/src/core/model-resolver.ts` (`buildFallbackModel`, `resolveCliModel`, capability/provider match) | `pi/packages/ai/test/retry.test.ts`, `pi/packages/coding-agent/test/model-resolver.test.ts` | vitest |
| opencode | `opencode/packages/opencode/src/provider/provider.ts` (gateway/transform routing incl. Cloudflare AI Gateway), `opencode/packages/opencode/src/provider/transform.ts`, `opencode/specs/v2/provider-policy.md` (`provider.use` allow/deny) | `opencode/packages/core/test/plugin/provider-cloudflare-ai-gateway.test.ts`, `opencode/packages/core/test/plugin/provider-llmgateway.test.ts`, `opencode/packages/opencode/test/provider/transform.test.ts` | bun test + Effect |

**hermes-agent** — the richest fallover model, and the closest analogue to the
proposed seam. `error_classifier.py` defines a `FailoverReason` enum whose
variants *are* the routing decision: `rate_limit` / `upstream_rate_limit`
(429 → backoff then rotate, or fall back to a **different model** when the key
is healthy), `overloaded` (503/529 → backoff), `server_error` (500/502 →
retry), `timeout` (rebuild client + retry) — all `retryable=True`; versus
`auth_permanent`, `billing`, `model_not_found`, `content_policy_blocked`,
`format_error` — `retryable=False`, abort or fall over to another model, never
blind-retry the same call. `classify_api_error(...)` maps a raw provider
error → a `ClassifiedError` with those hints; `run_agent.py` consumes them:
`fallback_model` is invoked when the primary can't rotate (single-credential
pool that just 429'd, issues #11314/#13636) and `switch_model` swaps provider
mid-run. Tests assert the whole matrix:

- **Classification is exhaustive and correct** — `test_error_classifier.py`
  drives raw errors → expected `FailoverReason` + `retryable` for each variant.
- **Retryable → fail over; terminal → abort** — `test_31273_402_not_retried.py`
  asserts a `402` billing error is **not** retried (terminal), while
  `test_auth_provider_failover.py` asserts an auth failure rotates provider.
- **Fallback exhaustion + cooldown** — `test_24996_fallback_exhaustion_cooldown.py`
  asserts the chain stops after exhausting candidates and enters a cooldown
  (the circuit-breaker analogue) instead of hot-looping.
- **Bounded jittered backoff** — `tests/test_retry_utils.py` asserts
  `jittered_backoff` is exponential, respects a max delay, adds jitter, and is
  thread-safe (so retries don't stampede).

**pi** — retryability classification + model/provider resolution, split across
two layers. `retry.ts` `isRetryableAssistantError(...)` matches a provider
error string against a **retryable** pattern set (`overloaded`, `rate.?limit`,
`429`, `500/502/503/504/524`, `timeout`, `fetch failed`, `stream ended before
message_stop`, …) *minus* an explicit **non-retryable** set (`insufficient_quota`,
`quota exceeded`, `billing`, per-tier `GoUsageLimitError`/`FreeUsageLimitError`).
`model-resolver.ts` resolves a pattern → a concrete `Model` across providers,
rejects **ambiguous** bare-id matches across providers, and `buildFallbackModel`
picks a provider default when a specific id is unavailable. Tests assert:

- **Retryable vs. non-retryable classification** — `retry.test.ts` asserts
  provider-limit wording stays **non-retryable** while transient socket-drop /
  premature-stream wording is **retryable** ("keeps provider limit errors
  non-retryable", "matches Bun fetch socket drop wording").
- **Provider/model resolution + ambiguity** — `model-resolver.test.ts` asserts a
  `provider/id` reference resolves uniquely, a bare id ambiguous across
  providers is rejected, and `resolveCliModel` honours `--provider`/`--model`.

**opencode** — routing lives at the **provider/gateway** layer (transform + a
Cloudflare / LLM gateway that fronts many upstreams) plus a `provider.use`
**policy** that allows/denies a provider by wildcard (`provider-policy.md`,
last-match-wins). It has no in-process capability/cost *router* trait like the
proposed seam; the gateway is the routing point and the policy gates which
providers are eligible. Tests assert the gateway wiring (`aiGatewayHeaders`,
account/token plumbing, `cf-aig-token`) and transform behaviour, i.e. that a
request is correctly routed *through* the selected gateway/provider.

## Completeness gaps

Behaviour agent-seddon must add/guarantee to be the most complete (spec only —
do **not** implement here):

- **`Router` seam that IS-A `LlmProvider`.** A new impl composing an ordered set
  of inner `Arc<dyn LlmProvider>` candidates behind the *unchanged* trait, so it
  drops into the existing `build_provider` slot and the loop is oblivious. Its
  `capabilities()` is the union/least-upper-bound of its candidates.
- **Routing policies (pluggable, config-selected).** `capability` (first
  candidate whose `ModelCapabilities` satisfy the request's needs — e.g.
  `supports_tools`, `context_window ≥ N`), `cost-min` (cheapest candidate that
  qualifies — needs a per-candidate cost weight in config; ties to spec 23),
  `latency` (fastest by observed EWMA latency), and `explicit` (a per-task pin,
  e.g. an `aux`/`small` model for compaction). Selection is deterministic given
  the same inputs and health state.
- **Failover with bounded retries + backoff.** On a **classified retryable**
  error advance to the next candidate; cap total attempts (retry budget) and
  apply exponential **jittered** backoff between attempts (port hermes'
  `jittered_backoff`: exponential, max-delay clamp, jitter). A **terminal** error
  (auth, bad-request, content-policy, quota-exhausted) short-circuits — no
  fallover, surface it. Requires an **error-classifier** (port hermes
  `FailoverReason` / pi `isRetryableAssistantError`) since today's error is
  opaque.
- **Health / circuit-breaking.** Track per-candidate consecutive failures; open
  a breaker after a threshold so the candidate is **skipped** while unhealthy;
  half-open probe / cooldown re-admits it (port hermes' exhaustion→cooldown).
- **All-candidates-fail surfaces one error.** When every candidate errs (or is
  breaker-open), return a single aggregated error naming the last cause — no
  hot-loop, no silent hang.
- **Metrics + span.** A **route-decision counter** labelled by chosen target
  (and policy), a **fallover counter** labelled by from→to + reason, and
  **per-target latency**; a `router.route` span carrying `target`,
  `policy`, `attempt`, and `fallover_reason` attributes (matching the #44
  span-attribute pattern). Extends `metered.rs` beyond the single-provider
  counters.
- **Route across local + remote seams.** Because a candidate can be the existing
  `grpc` provider client, one `Router` may list a local `anthropic` and a remote
  `= "grpc"` provider and fail over across the wire — no peer does this behind a
  single provider trait.

## Table-driven test plan

**Target test file:** a new `#[cfg(test)] mod tests` in the router impl (proposed
`crates/agent-providers/src/router.rs`) plus a classifier table beside it. The
`Router` is a pure `LlmProvider` composed of test doubles — no network, no
`tempdir()`.

**Doubles (from [`agent-testkit`](../../crates/agent-testkit/src/lib.rs)):**
`ScriptedProvider` (already the canonical `LlmProvider` fake — scripts a
response) is the healthy candidate; a **new** double `ErrProvider { err, caps }`
(returns a scripted classified error a fixed number of times, then optionally
succeeds) models the failing/erroring candidate and the recovering one. Add
`ErrProvider` beside `ScriptedProvider` in `agent-testkit`. Candidates carry
distinct `ModelCapabilities` (and a test-only cost weight) so the routing-policy
cases can discriminate. A `MetricsProbe` (agent-testkit `observe`) asserts the
route/fallover counters.

**Naming prefixes** (match the `edit.rs` convention): `positive_` (routes /
recovers as intended), `negative_` (surfaces an error), `corner_`
(boundary/empty/all-fail), `boundary_` (breaker threshold edge).

```rust
// ---- error classification: retryable vs terminal (the fallover gate) ----
#[rstest]
#[case::positive_rate_limit("429 too many requests",        true)]  // (port: pi|hermes)
#[case::positive_overloaded("provider overloaded",          true)]  // (port: pi|hermes)
#[case::positive_server_5xx("500 internal server error",    true)]  // (port: pi)
#[case::positive_timeout("connection timed out",            true)]  // (port: hermes)
#[case::positive_stream_drop("stream ended before message_stop", true)] // (port: pi)
#[case::negative_auth("401 invalid api key",                false)] // (port: hermes)
#[case::negative_billing("insufficient_quota",              false)] // (port: pi|hermes)
#[case::negative_bad_request("400 bad request",             false)] // (port: hermes)
#[case::negative_content_policy("content blocked by safety filter", false)] // (port: hermes)
fn classifies_retryable(#[case] err: &str, #[case] retryable: bool) {
    assert_eq!(is_retryable(&anyhow::anyhow!("{err}")), retryable);
}

// ---- routing policy: pick the right primary from N candidates ----
// candidates: cheap(no-tools, 8k ctx, cost=1), big(tools, 200k ctx, cost=10)
#[rstest]
#[case::positive_capability_needs_tools(Policy::Capability, needs_tools(), "big")]        // (port: pi)
#[case::positive_capability_needs_ctx(Policy::Capability, needs_ctx(100_000), "big")]     // (new: agent-seddon)
#[case::positive_cost_min_easy(Policy::CostMin, easy_req(), "cheap")]                      // (new: agent-seddon)
#[case::positive_explicit_pin(Policy::Explicit("cheap"), easy_req(), "cheap")]            // (port: hermes aux-model)
#[tokio::test]
async fn routes_to_expected_target(
    #[case] policy: Policy, #[case] req: CompletionRequest, #[case] target: &str,
) {
    // Router::new(policy, [cheap, big]); assert the served response came from `target`
    // (ScriptedProviders tag their response content with their name) and that the
    // route-decision counter for `target` incremented exactly once.
}

// ---- failover: primary errs (retryable) -> secondary serves ----
#[rstest]
#[case::positive_failover_on_rate_limit("429", 1, Ok("from-secondary"))]      // (port: hermes|pi)
#[case::positive_failover_on_5xx("503", 1, Ok("from-secondary"))]             // (port: hermes)
#[case::negative_no_failover_on_terminal("401", 1, Err("invalid api key"))]   // terminal: don't advance // (port: hermes)
#[case::corner_all_candidates_fail("429", 2, Err("all providers failed"))]    // both err -> aggregated // (port: hermes)
#[tokio::test]
async fn fails_over(#[case] err: &str, #[case] n_failing: usize, #[case] expect: Result<&str,&str>) {
    // primary = ErrProvider(err); secondary = ScriptedProvider("from-secondary").
    // n_failing=2 makes BOTH candidates ErrProvider so the all-fail path is hit.
    // Assert: served response / aggregated error, and the fallover counter (from=primary,
    // to=secondary, reason) incremented on the retryable cases, NOT on the terminal one.
}

// ---- circuit breaker: unhealthy candidate is skipped, recovers after cooldown ----
#[rstest]
#[case::boundary_opens_after_threshold(3, /*breaker opens; next route skips primary*/ "secondary")] // (port: hermes cooldown)
#[case::positive_half_open_recovers(/*after cooldown, primary probed & re-admitted*/ "primary")]      // (port: hermes)
#[tokio::test]
async fn circuit_breaker_skips_unhealthy(#[case] /* ... */) {
    // Drive primary to `threshold` consecutive retryable failures; assert subsequent
    // routes SKIP it (route-decision target == secondary) with no wasted attempt on
    // primary; after the cooldown a half-open probe re-selects primary once healthy.
}

// ---- bounded jittered backoff (ported hermes retry_utils) ----
#[rstest]
#[case::positive_exponential(1, 2)]          // delay(n+1) > delay(n)                 // (port: hermes)
#[case::boundary_clamped_to_max(100, /*==max_delay*/ )]                                // (port: hermes)
fn backoff_is_bounded(#[case] attempt: u32, #[case] _expected: u32) {
    // assert jittered_backoff exponential, clamped to max, jitter within bounds.
}
```

Case-prefix key: `positive_` routes/recovers as designed, `negative_` surfaces
an error (terminal, no fallover), `corner_` all-candidates-fail, `boundary_`
breaker-threshold / max-delay edges. `(port: …)` names the peer the case came
from (`hermes` for the fallover/classifier/cooldown matrix, `pi` for the
retryable-pattern + capability-resolution cases); `(new: agent-seddon)` marks
cases with no peer origin (cost-min, context-window capability match).

**Prereq work the plan implies:** (a) an `is_retryable` / error-classifier over
provider errors (a small enum mirroring hermes `FailoverReason`); (b) a per-
candidate cost/latency descriptor in config + `Router` config schema (ordered
candidate list + policy); (c) the `Router` impl + a `router` factory line in
`register_builtins` (guarded by a `router` feature); (d) `ErrProvider` in
`agent-testkit`; (e) route/fallover metric families in `agent-metrics` + a
`metered.rs` decorator; (f) doc `docs/components/router.md`.

**Harness obligations** (per the spec contract; makes the implementing PR
unambiguous):

- **Seam + registry:** new `Router` trait-impl of `LlmProvider` in
  `agent-providers` behind a `router` cargo feature; one `r.provider("router", …)`
  factory line in `register_builtins` (config-selected via `[agent] provider =
  "router"` + a `[router]` candidate list + policy). Doc in
  `docs/components/router.md`.
- **Proto + gRPC:** **reuses `ProviderService`** — a `Router` *is* an
  `LlmProvider`, so it serves under the existing `Provider` service
  (`--serve provider`) with no new `.proto` and no `buf.image.binpb` bump; each
  candidate may itself be a `grpc` provider client (route local + remote). Note
  in the doc that no proto change is required.
- **Metrics + OTel:** route-decision counter (label: target, policy), fallover
  counter (labels: from, to, reason), per-target latency; a `router.route` span
  with `target` / `policy` / `attempt` / `fallover_reason` attributes.
- **Bench:** an iai-callgrind bench over the **route-decision / candidate-
  selection** logic — a deterministic CPU hot path (score N candidates against a
  request under each policy, no I/O) with an Ir ceiling in
  `nix/checks/bench.nix`. The network fallover path itself is I/O-bound and
  documents the skip.
- **Leak:** a dhat `tests/leak.rs` (`dhat-heap` feature) over the routing +
  failover path (iterate route→fail→fallover with `ErrProvider` doubles) to
  assert the retry/breaker state frees everything it allocates.

## References

- **agent-seddon:** trait + `ModelCapabilities` — [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs); impls — [`crates/agent-providers/src/anthropic.rs`](../../crates/agent-providers/src/anthropic.rs), [`crates/agent-providers/src/openai_compat.rs`](../../crates/agent-providers/src/openai_compat.rs); registry (`provider` factories, `build_provider`) — [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs); provider gRPC seam — [`crates/agent-proto/proto/agent/v1/provider.proto`](../../crates/agent-proto/proto/agent/v1/provider.proto), served via `Seam::Provider` in [`crates/agent-cli/src/grpc_server.rs`](../../crates/agent-cli/src/grpc_server.rs); provider metrics — [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs); test doubles (`ScriptedProvider`) — [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs).
- **hermes-agent:** `hermes-agent/agent/error_classifier.py` (`FailoverReason`, `classify_api_error`), `hermes-agent/run_agent.py` (`fallback_model`, `switch_model`), `hermes-agent/agent/retry_utils.py` (`jittered_backoff`); tests `hermes-agent/tests/agent/test_error_classifier.py`, `hermes-agent/tests/run_agent/test_auth_provider_failover.py`, `.../test_24996_fallback_exhaustion_cooldown.py`, `.../test_31273_402_not_retried.py`, `hermes-agent/tests/test_retry_utils.py`.
- **pi:** `pi/packages/ai/src/utils/retry.ts` (`isRetryableAssistantError`), `pi/packages/coding-agent/src/core/model-resolver.ts` (`buildFallbackModel`, `resolveCliModel`); tests `pi/packages/ai/test/retry.test.ts`, `pi/packages/coding-agent/test/model-resolver.test.ts`.
- **opencode:** `opencode/packages/opencode/src/provider/provider.ts`, `opencode/packages/opencode/src/provider/transform.ts`, `opencode/specs/v2/provider-policy.md`; tests `opencode/packages/core/test/plugin/provider-cloudflare-ai-gateway.test.ts`, `opencode/packages/core/test/plugin/provider-llmgateway.test.ts`, `opencode/packages/opencode/test/provider/transform.test.ts`.
