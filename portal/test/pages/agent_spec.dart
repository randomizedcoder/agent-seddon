import '../testkit/spec.dart';

/// Agent view spec (design 02) — the mechanically hardest page: a server-
/// streaming `Subscribe` transcript (loading / stream-down / loaded), a
/// `Registry.Open` → `Session.Send` drive with an in-flight `_sending` guard, a
/// `Reconnect` re-subscribe, and two periodic pollers (GPU pool @3 s, gRPC
/// latency @5 s). Every key stem in `agent_view_page.dart` — `agent.goal`,
/// `agent.send`, `agent.reconnect`, `agent.status.pool`, `agent.status.grpc`,
/// `agent.transcript` — has a `positive_` row, so the completeness critic passes.
const agentSpec = PageSpec('agent', [
  // ── the streamed transcript: loading / loaded / down ──────────────────────
  SpecRow(
    elementId: 'agent.transcript',
    caseClass: CaseClass.positive,
    name: 'stream_renders_transcript',
    description: 'Subscribe events render as transcript lines',
    expectedRpc: 'agent.v1.AgentSessionService/Subscribe',
  ),
  SpecRow(
    elementId: 'agent.transcript',
    caseClass: CaseClass.corner,
    name: 'loading_shows_empty_transcript',
    description: 'before any event, the transcript is empty (not the down panel)',
    expectedRpc: 'agent.v1.AgentSessionService/Subscribe',
  ),
  SpecRow(
    elementId: 'agent.transcript',
    caseClass: CaseClass.boundary,
    name: 'token_deltas_continue_line',
    description: 'token deltas append inline to the current assistant line',
    expectedRpc: 'agent.v1.AgentSessionService/Subscribe',
  ),
  // ── stream-down + reconnect ────────────────────────────────────────────────
  SpecRow(
    elementId: 'agent.reconnect',
    caseClass: CaseClass.negative,
    name: 'stream_error_shows_reconnect',
    description: 'a failed Subscribe greys the view into the Reconnect panel',
    expectedRpc: 'agent.v1.AgentSessionService/Subscribe',
  ),
  SpecRow(
    elementId: 'agent.reconnect',
    caseClass: CaseClass.positive,
    name: 'reconnect_resubscribes',
    description: 'tapping Reconnect re-dials Subscribe and clears the panel',
    expectedRpc: 'agent.v1.AgentSessionService/Subscribe',
  ),
  // ── the goal input + drive ─────────────────────────────────────────────────
  SpecRow(
    elementId: 'agent.goal',
    caseClass: CaseClass.positive,
    name: 'goal_updates_field',
    description: 'typing updates the goal field (local, no RPC)',
  ),
  SpecRow(
    elementId: 'agent.send',
    caseClass: CaseClass.positive,
    name: 'send_opens_then_sends',
    description: 'Send mints a session (Open) then drives Send {goal}',
    expectedRpc: 'agent.v1.AgentSessionService/Send',
  ),
  SpecRow(
    elementId: 'agent.send',
    caseClass: CaseClass.boundary,
    name: 'slow_send_no_double_submit',
    description: 'while a Send is in flight the button is disabled — one Open',
    expectedRpc: 'agent.v1.AgentSessionService/Send',
  ),
  SpecRow(
    elementId: 'agent.send',
    caseClass: CaseClass.negative,
    name: 'open_error_greys_stream',
    description: 'a failed Open greys the transcript and re-enables the button',
    expectedRpc: 'agent.v1.SessionRegistryService/Open',
  ),
  SpecRow(
    elementId: 'agent.send',
    caseClass: CaseClass.adversarial,
    name: 'huge_goal_sent_verbatim',
    description: 'a huge goal payload is sent without truncation or crash',
    expectedRpc: 'agent.v1.AgentSessionService/Send',
  ),
  // ── status cells: the two pollers ──────────────────────────────────────────
  SpecRow(
    elementId: 'agent.status.pool',
    caseClass: CaseClass.positive,
    name: 'pool_poll_renders_and_repolls',
    description: 'Health fills the GPU-pool cell and re-polls every 3 s',
    expectedRpc: 'agent.v1.LlmPoolService/Health',
  ),
  SpecRow(
    elementId: 'agent.status.pool',
    caseClass: CaseClass.negative,
    name: 'pool_error_shows_na',
    description: 'a failed Health leaves the GPU-pool cell at n/a, no crash',
    expectedRpc: 'agent.v1.LlmPoolService/Health',
  ),
  SpecRow(
    elementId: 'agent.status.grpc',
    caseClass: CaseClass.positive,
    name: 'grpc_poll_renders_and_repolls',
    description: 'Query fills the gRPC-latency cell and re-polls every 5 s',
    expectedRpc: 'agent.v1.MetricsProxyService/Query',
  ),
  SpecRow(
    elementId: 'agent.status.grpc',
    caseClass: CaseClass.boundary,
    name: 'grpc_empty_series_shows_na',
    description: 'an empty PromResult leaves the gRPC cell at n/a, no crash',
    expectedRpc: 'agent.v1.MetricsProxyService/Query',
  ),
]);
