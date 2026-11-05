# Status — observability dimensionality (tenant + repo)

Legend: ⬜ designed, not built · 🟡 in progress / open PR · ✅ built + merged.

**Track state: ✅ complete.** Design-of-record written 2026-09-10; all phases merged by 2026-09-12. Each
phase was a gated PR off `main`, never stacked (all phases touch `agent-metrics/src/lib.rs` and/or the
shared gRPC span helper, so they would conflict if branched together), with a merge pause between phases.
Every metric family now matches its census decision, both ClickHouse sinks (`agent_logs` + OTLP traces)
carry tenant/repo/pr, and every span created under a scope is tenant-filterable.

| Phase | Scope | State | PR |
|---|---|---|---|
| **0** | audit + census + doctrine reconciliation (docs-only): this dir + revisions to review-fleet 07, multi-session 06, `docs/tracing.md`, `docs/observability.md` | ✅ | #315 |
| **1** | shared plumbing: `tenant` on the `grpc.server` span; new `agent_fleet_*` families registered; repo-label LRU cap helper; bench ceilings bumped | ✅ | #316 |
| **2** | review-fleet + C18 progress + approver — the C19 build (per-tenant + per-repo metrics; `fleet.*` spans with tenant/repo/pr) | ✅ | #317 |
| **3** | message transport (slack/matrix) — post/recv/ratelimit/soft-fail metrics + `transport.*` spans | ✅ | #318 |
| **4** | config-plane: config-store backends (all registry/scheduler/prompt persistence), auth verify + authz allow/deny, one generic per-RPC server metric (per-tenant) | ✅ | #320 |
| **5.5** | ClickHouse sinks — `agent_logs` inherits tenant/repo/pr from the span scope (+`repo`/`pr` columns); OTLP `EnrichSpanProcessor` stamps tenant/session on every scoped span; `tenant` on the `agent.turn` root | ✅ | #319 |
| **5** | sweep the pre-existing families + existing spans per the census (§E resolved: policy_authorize/policy_guard/hook_dispatches/session_ops → +tenant; scheduled/session_gc/registry kept health with rationale; span side already covered by 5.5's processor) | ✅ | #321 |

## Later fixes

- **Triggers counted, HyperDX deployed, portal links follow the page** (after the l2
  checks on 2026-11-04): Grafana's trigger panel was empty because
  `FleetMetrics::on_trigger` was never called. `FleetTrigger` now carries a
  `TriggerSource` (poll/webhook/slack/review_now) that `handle` counts. HyperDX had never
  moved from hand-started podman into `nixosModules.agent-stack`; `hyperdx.enable` adds it
  (see `docs/deployment-nixos.md`), with `[telemetry] otlp_headers_file` keeping the
  ingestion key out of config. The portal's Launch links swap a loopback host for the
  page's host. Tables: `trigger_counted_cases`, `telemetry_otlp_headers_cases`,
  `obs_link_test.dart`, Launch rows `follows_page_host`/`empty_url_hidden`, and the
  `hyperdx*` rows of `nixos-agent-stack-eval`.
- **GenAI span attributes** (l2, 2026-11-04, after the HyperDX deploy): HyperDX's
  LLM dashboard found no LLM calls because it matches the OTel GenAI attribute names, and
  no span carried them. `crates/agent-runtime/src/genai.rs` adds them to the model-call
  and `tool.execute` spans (operation, provider, model, conversation, finish reason,
  token usage, cost, error status), bounded and with hostile costs dropped
  ([tracing.md](../../tracing.md)). Table: `genai::tests`. Also from the #701 review:
  `FLEET_TRIGGER_SOURCES` is derived from `TriggerSource::ALL`; the HyperDX firewall
  warning fires whenever the firewall is off (row
  `hyperdxWarnsWithoutFirewallOnLoopback`), and its text and the deployment docs now say
  the ingestion key is checked only when the team turns collector authentication on;
  `obsLink` treats `0.0.0.0` as loopback, matches `127.x` only as an IPv4 address, and
  has IPv6 rows.

- **Dashboards for every family** (2026-11-04): the single hand-written `agent-seddon.json`
  becomes seven dashboards generated from `nix/grafana/dashboards.nix` (overview, LLM &
  router, review fleet, tools, sessions/context/memory, gRPC/stores/search, traces), 114
  metric families plus the ClickHouse review tables and `otel_traces`. Each picks its data
  sources by variable. `services.agentSeddon.grafana.enable` provisions them into the
  host's Grafana with a ClickHouse source as `agent_viewer` (password via `LoadCredential`).
  The `grafana-dashboards` check fails on an unknown metric or table, with check-the-checks
  on a broken fixture; the `grafana*` rows of `nixos-agent-stack-eval` cover the module.
  Carried from #702's review: the legacy `tool` field on `tool.execute` is bounded like
  `gen_ai.tool.name`; an agent-loop test pins that a real turn's `provider.complete` /
  `provider.stream` and `tool.execute` spans carry the GenAI attributes; the span census
  points at `genai.rs`; `docs/tracing.md` says `gen_ai.request.model` under the task router
  is the configured default; the portal's obs links treat `[::]` like `0.0.0.0`.
  Review fixes (#703 draft): the overview's tool error share uses the job filter on both
  sides (the error counter has no tenant label); the check also fails a generated uid that
  would overwrite a hand-written `dashboards/<uid>.json`; the Tenant variable lists `user`
  from every agent family, not only review-fleet ones.

## Cross-references updated by Phase 0

- [`../review-fleet/07-observability.md`](../review-fleet/07-observability.md) — the "no repo/PR in
  labels" rule revised: **repo permitted** as a bounded operator-roster label (LRU-capped); **PR still
  forbidden** (span attribute only).
- [`../multi-session/06-observability.md`](../multi-session/06-observability.md) — the curated
  attributable subset **expanded** to the config-plane/fleet families; the repo-LRU lifecycle rule added.
- `docs/tracing.md`, `docs/observability.md` — net-new per-tenant/per-repo **span-attribute** guidance.

## Notes

- **No wire/proto change** anywhere in this track (instrumentation only) → no `buf.image.binpb` bump.
- Bench ceilings in `crates/agent-metrics/benches/metrics.rs` step up as families land (Phases 1–4);
  each bump is recorded in the bench comment.
- Known gate flake: the `leak` check trips under `--max-jobs 8` memory pressure on untouched crates —
  verify isolated via `nix build .#checks.x86_64-linux.leak` (LEAK_EXIT=0), then treat as pass.
