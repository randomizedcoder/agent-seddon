# 05 — Message-transport registry (C37)

Generalize messaging beyond Slack: a bidirectional `MessageTransport` seam with Slack as one impl and
matrix/teams/IRC/signal as future cards. Channel/token config lifts out of `FleetSession` into a
transport card.

> **Status: 🟡 built (D2).** The seam is generalized and the card registry shipped: `agent_core`
> now owns the bidirectional `MessageTransport` seam (`recv` + the **new outbound `post`**) with
> neutral `InboundMessage`/`OutboundMessage`/`Channel`, plus `TransportCard`/`ChannelBinding`/
> `ChannelPurpose`/`TransportRegistry` and the pure `RateLimiter` + soft-fail `announce` primitives.
> `agent-slack` became an impl (`SlackMessageTransport` posts via `chat.postMessage`, rate-limited +
> bot-token-gated; `SlackSocketMode` still carries inbound) behind `agent_slack::build_transport_from_card`
> (unknown kind fails closed at build time; endpoint SSRF-screened), with an in-crate `StoreTransports`
> and the `TransportRegistryService` seam (`--serve-transport-registry`, port 50089), RBAC-gated on
> `(write|delete, transport_registry)`.
>
> **D2b (under way — the transport twin of D1b, a 3-PR split).** PR1 landed the **matrix** host impl
> (`agent-slack/src/matrix.rs`, opt-in `transport-matrix` feature, off in the default build): a second
> `MessageTransport` proving the C37 recipe — *add a host = a new impl + a `kind.rs` factory line, no core
> allow-list edit* — against a genuinely divergent protocol (`PUT` with a client transaction id, the room
> id percent-encoded into the URL **path**, a single access token in `bot_token_ref`, `errcode` errors).
> PR2 then landed **card-by-id + Socket-Mode inbound unification**: a `FleetSession` may set an additive
> `transport_id` (proto field 15) referencing a persisted `TransportCard`; the fleet's Slack watch now
> resolves each row through `agent_slack::slack_trigger_binding` and runs **one Socket-Mode connection per
> resolved app token** — a card row takes its `app_token_ref` + `trigger`-purpose channels from the card,
> a legacy row (empty `transport_id`) keeps the inline `slack_*` + the `[review_fleet.slack]` default token
> (unchanged). A missing/disabled/non-Slack card contributes no trigger (fail-closed). **Still to come in
> D2b:** the review-fleet C18 progress feed wired as a live `announce()` caller (the `progress`-purpose
> channels). teams/irc/signal host impls are further-deferred. See [`09-increments.md`](09-increments.md) §D2.

## Where we are today

Messaging is **inbound-only and Slack-named** (`crates/agent-slack/src/lib.rs`):
- `trait SlackTransport { async fn recv(&mut self) -> Option<InboundMessage>; }` (`lib.rs:43`) — a real
  seam (the gate drives a fake; `SlackSocketMode` in `socket_mode.rs` is the only network code), and
  `InboundMessage { channel, text }` (`lib.rs:33`) is already transport-neutral in shape.
- **No outbound abstraction** — outbound `chat.postMessage` (the review-fleet C18 progress feed) would
  live in `serve_fleet` wiring, not behind the seam.
- `SlackWatch` fan-out (`lib.rs:59`) is keyed on Slack channel strings + Slack PR-link parsing
  (`parse.rs`).
- Config is Slack-specific and split across two places: `FleetSlackCfg { app_token_ref, bot_token_ref }`
  (`crates/agent-runtime/src/config.rs:176`) and the `slack_trigger_channel` / `slack_progress_channel`
  fields on `FleetSession` itself (`crates/agent-core/src/lib.rs:2872`).
- No matrix/teams/IRC/signal code exists.

The good news: everything **downstream** of a message is already transport-agnostic — the neutral
`FleetTrigger` / `TriggerSink` pipeline (review-fleet C7/C8) doesn't know or care that a trigger came
from Slack. The abstraction boundary exists; it's just Slack-named and one-directional.

## The `MessageTransport` seam

Rename/generalize to a **bidirectional** seam in `agent-core`:

```rust
#[async_trait]
pub trait MessageTransport: Send + Sync {
    fn kind(&self) -> &str;                                  // "slack" | "matrix" | ...
    async fn recv(&mut self) -> Option<InboundMessage>;      // as today
    async fn post(&self, to: &Channel, msg: &OutboundMessage) -> Result<()>;  // NEW outbound half
}
```

- `InboundMessage`/`OutboundMessage`/`Channel` are transport-neutral (Slack channel, Matrix room,
  Teams channel, IRC channel, Signal thread all map to `Channel`).
- The existing `agent-slack` crate becomes one impl (`SlackTransport: MessageTransport`); matrix/teams/
  irc/signal are future impls behind their own cargo features, registered by `kind` in a transport
  factory (exactly like forges, C36).

## The transport card

A card per the C32 pattern:

```proto
message TransportCard {
  string id = 1;                 // safe_segment
  string kind = 2;               // slack|matrix|teams|irc|signal
  bool   enabled = 3;
  string endpoint = 4;           // workspace/homeserver/host; empty ⇒ kind default
  string app_token_ref = 5;      // e.g. Slack xapp- (Socket-Mode); env:/file: ref
  string bot_token_ref = 6;      // e.g. Slack xoxb- (outbound); env:/file: ref
  repeated ChannelBinding channels = 7;   // which channels this transport watches/posts to
  uint32 rate_limit_per_min = 8; // clamped on ingest
}
message ChannelBinding { string channel = 1; string purpose = 2; }  // purpose: trigger|progress
```

- **Channel/token config lifts out of `FleetSession`.** A `FleetSession` references a transport card +
  purpose bindings (trigger/progress) by id, instead of carrying `slack_*` fields and the fleet reading
  `FleetSlackCfg`. This decouples "which repo to review" from "how to reach its humans".
- Tokens are `*_ref` references; the card store is the shared C41 backend, `PerTenant`-wrapped (C35) —
  each org configures its own transports (org A on Slack, org B on Matrix).

## Announce-only, and the review-fleet C18 relationship

The review-fleet progress feed (C18, still unbuilt) is **announce-only** — it posts lifecycle events
(found PR → reviewing → draft ready → approved → posted) via `MessageTransport::post`; approval itself
stays the `Approve` RPC. C37 is the seam C18 posts through. (If C18 lands before C37 it does so directly
against Slack; C37 then generalizes it — the seam is designed so C18 is a thin caller either way.)

## Security

- **Inbound text is data, never instructions** — exactly as today only a `u64` PR number is ever
  extracted; prose/@mentions/"ignore your rules" content is inert and never forwarded to the model.
  Link-parsing (host + owner/repo match) moves behind the transport (per-kind), strict `url::Url`-based.
- **Tokens are `*_ref`**, never logged; outbound posts reuse the review redaction pass (no secret/token
  in a message); per-transport **rate-limit** + **soft-fail** (a failed post never blocks a review).
- A bot must **not react to itself** (Slack `bot_id`/`subtype` acked-but-inert today) — the neutral seam
  keeps that rule per-impl.
- Unknown `kind` → fail-closed reject.

## C37 test matrix

| Class | Case | Expect |
|---|---|---|
| positive | `positive_slack_recv_and_post_roundtrip` | inbound parsed to `InboundMessage`; `post` reaches the fake |
| positive | `positive_fleet_references_transport_card` | a `FleetSession` binds trigger/progress channels by card id |
| negative | `negative_unknown_transport_kind_rejected` | `kind:carrierpigeon` → reject, error lists known kinds |
| negative | `negative_post_without_bot_token_errors` | outbound with no `bot_token_ref` → clear error |
| boundary | `boundary_rate_limit_enforced` | posts beyond `rate_limit_per_min` → throttled, not dropped silently |
| corner | `corner_post_failure_is_soft` | transport `post` error → soft (review continues), counted |
| corner | `corner_bot_message_does_not_trigger` | a bot/self message → acked, no trigger (no self-loop) |
| adversarial | `adversarial_inbound_text_is_not_executed` | prose/@mention/injection in a message → inert; only PR# extracted |
| adversarial | `adversarial_lookalike_host_link_rejected` | `github.com.evil.com` link → not matched (strict `url::Url`) |
| adversarial | `adversarial_token_ref_never_logged` | token ref masked in logs; raw token in `*_token_ref` rejected |
