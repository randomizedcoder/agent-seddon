# Router

Provider routing and failover. Parity spec [25](../parity/25-model-routing.md).

A `Router` **is-a** `LlmProvider`, so nothing downstream knows it exists — the
loop, the context strategy, and the metered decorators all see one provider. What
it adds is resilience: an agent that speaks to exactly one provider inherits that
provider's worst day, and a classified *transient* failure on the primary should
continue on a secondary rather than aborting the run.

Each candidate is still an independent seam, **including a `grpc` client**, so one
router can span local and remote providers.

## Configuration

```toml
[agent]
provider = "router"

[router]
providers         = ["anthropic", "openai-compat"]   # preference order
policy            = "in-order"                       # in-order | round-robin
failure_threshold = 3                                # failures before the breaker opens
cooldown_secs     = 30                               # how long it stays open
```

Candidate names are other **registered provider names**, built back through the
registry — so a candidate can be `grpc`, or anything an out-of-tree binary
registered.

## Three rules that make failover safe

### 1. Only retryable failures fail over

A terminal failure — auth, billing, bad request, content policy, unknown model —
fails the same way on every candidate. Trying them all burns the chain, and real
money, to arrive at the identical answer. Classification lives in `agent-retry`
(`classify`) and is shared rather than re-implemented:

| Class | Examples |
|---|---|
| `Retryable` | 429, 5xx, 529 overloaded, timeout, connection refused/reset |
| `Terminal` | 401/403 auth, 402 billing, 400 bad request, 404 model, content policy |

**Unknown failures classify as `Terminal`.** That is the conservative choice: an
unrecognised error is more likely a deterministic bug (a malformed request, an
unsupported parameter) than a transient blip, and retrying it across every
candidate is expensive and pointless.

> Classification reads the error *message*, because that is the contract the
> provider seam actually has — `Error::Provider(String)` carries no status code,
> and the in-tree adapters format failures as `"http {code}: {body}"`. This is
> the honest weak point: a custom provider that formats errors differently gets
> `Terminal` (i.e. no failover) rather than a wrong retry. Making the status
> structured on `Error` would remove the guesswork and is the natural follow-up.

### 2. An unhealthy candidate is skipped

Consecutive failures open a per-candidate circuit breaker; it closes again after
`cooldown_secs`. Without this a dead provider costs a timeout on *every* turn
forever. Unhealthy candidates are ordered **last** rather than dropped, so a total
outage still attempts something instead of failing with "no candidates".

### 3. Incapable candidates are not tried

A candidate that structurally cannot serve the request — no tool support when the
request carries tools, no vision when it carries images — is skipped. Failing over
to it would just produce a different error.

## Capabilities

The router reports the **union** of its candidates' capabilities, with the
**minimum** context window:

- Union, so the loop doesn't disable tools just because the *first* candidate
  lacks them.
- Minimum window, because a request has to fit whichever candidate ends up
  serving it.

## Streaming

Failover covers failures raised while *establishing* the stream. Once bytes are
flowing the turn is committed — restarting mid-stream would duplicate content the
caller has already seen.

## Observability

| Metric | Labels |
|---|---|
| `agent_route_decisions_total` | `target`, `decision` = `routed` \| `fellover` \| `skipped_unhealthy` \| `exhausted` |

Each candidate is also individually metered (it is wrapped in the standard
provider decorator before being handed to the router), so per-target latency and
error counts appear under the usual provider metrics with the candidate's name.

`agent-providers` does not depend on `agent-metrics`, so the router emits typed
`RouteEvent`s through a callback and the runtime turns them into metrics — keeping
the dependency direction intact rather than inverting it for observability.

## A note on the registry

The router factory is a **composing** factory: it builds its candidates by calling
back into the registry, which is why `FactoryCtx` carries a registry handle. The
borrow is immutable and re-entrant (`build_*` takes `&self`), so a factory the
registry invoked may call back into it.

A router listing itself would recurse until the stack blows, so that is rejected
at build time with a clear message.

## Deferred

- **Cost- and latency-based policies.** `in-order` and `round-robin` are
  implemented; cost-minimising routing needs per-candidate price metadata, which
  lives in `agent-tokenizer`'s `PriceTable` and is not yet plumbed to candidates.
- **Structured provider errors.** Classification is message-based (see above);
  a `status` on `Error::Provider` would make it exact.
- **Mid-stream failover**, which requires replay semantics the seam does not have.
