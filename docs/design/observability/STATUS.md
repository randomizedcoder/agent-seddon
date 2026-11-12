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

- **HyperDX without a login** (2026-11-07): `services.agentSeddon.hyperdx.auth = false`
  runs HyperDX's local app mode for a trusted lab network: no sign-up or login, one
  built-in team seeded with the ClickHouse connection and sources at start, and no
  collector authentication, so the key timer, key unit and restart-on-key path are not
  installed and the agents export without `otlp_headers_file`. The default stays
  `true`. `nixos-agent-stack-eval` rows: default not local, the exact guard string, no
  key units, agents still export, the firewall-off warning names the open endpoints.
  Review fix: the image's `entry.sh` overwrites `IS_LOCAL_APP_MODE` with
  `REQUIRED_AUTH`, so the app starts from a copy without that line and refuses to start
  if the line is missing (rows: the start command strips it, the default keeps the
  image's entrypoint); a LAN + `openFirewall` warning; the runbook's "not possible"
  note corrected. Live check (UI with no login, collector up at start) after deploy.

- **HyperDX dashboards** (2026-11-07): `services.agentSeddon.hyperdx.dashboards`
  (default on when `auth = false`) provisions four dashboards (overview, model calls,
  tools, review fleet) and four saved searches from `nix/hyperdx/dashboards.nix`
  through the app's API with `hyperdx-provision` (new crate `agent-hyperdx`: source
  placeholders by kind, create/update/leave-alone plan keyed on name + the
  `agent-seddon` tag, fail-closed parsing of the API's ids before they reach a URL,
  bounded replies, agent-retry until the API and sources are up). Gate:
  `hyperdx-dashboards` (`--check` over the template + five check-the-check fixtures)
  and `nixos-agent-stack-eval` rows. #703 carry-overs: a Grafana `requires` on the creds
  unit, an assertion when the host's Grafana is off, URL ports bounded to 1-65535, and
  the dev container's read-only dashboards no longer claim `allowUiUpdates`. Still open:
  a VM subtest for `grafana.enable`, and confirming the data-source variables
  auto-select on the live Grafana.
- **Every panel query, checked** (2026-11-12, after a live pass over l2's Prometheus,
  Grafana dashboards and HyperDX): all 175 Grafana panel queries ran without an error
  against l2's data (140 with data; the 35 empty ones name metrics the code registers
  for events l2 hasn't had: auth, approvals, posts, priced calls, search). PR 🟡 (O1):
  - **`obs-check`** (new crate `agent-obs-check`): fills a dashboard's variables as the
    browser does, sends each query to Grafana's `/api/ds/query` (macros left to Grafana
    and the plugins), and fails closed on the reply (a missing result is a failure).
    The module installs it as `agent-seddon-obs-check`. Tables cover interpolation (one
    pass, longest name first, macros untouched), variable values, data-source resolution
    and refusals, the nesting and query caps, the request, and reply verdicts.
  - **Gate:** `grafana-queries` (`obs-check --check`: 175 queries resolve; four broken
    fixtures refused, a valid one accepted). **VM test:** Prometheus behind the door
    (loopback, `/prometheus/`, scraping the three agents) and a subtest running
    `agent-seddon-obs-check`: no query fails, some return data, and a wrong password is
    refused. This closes the open "VM subtest for `grafana.enable`".
  - **Found and fixed:**
    - `grafana.prometheusUrl` refused a path, so Grafana couldn't reach Prometheus
      behind the door. It now takes `/segments` (no `..`, query, fragment or empty
      segment; eval rows).
    - HyperDX reads ClickHouse as `agent_viewer`, which couldn't read
      `system.data_skipping_indices`, so its searches skipped the indexes (a 403 per
      search). Granted; the table lists only tables the user can already see. RLS
      harness rows: the viewer may, the tenant reader may not.
    - HyperDX table tiles applied the tile's `ms` format to their count columns
      ("19 ms" runs). The three mixed tables drop it; the `p95 ms` alias carries the
      unit.
    - Every poll opens a `fleet.review` span (about 170 an hour on l2, around 12 ms
      each), so HyperDX's "Reviews by repo" counted polls. The span now records
      `outcome` (`drafted`, `failed`, `uptodate`, `at_capacity`), and the review tiles
      and saved search count `drafted`/`failed` only.
  - **Found by the new VM subtest** (all 175 queries failed "Plugin not registered"):
    - On a fresh host, systemd-tmpfiles made Grafana's plugins directory as root
      inside grafana's own directory. It then refused its own ClickHouse plugin link
      ("unsafe path transition"). l2 works only because Grafana had made the directory
      first. The module now makes the directory grafana's (eval row).
    - Grafana 13.3+ no longer bundles Prometheus. Preinstall downloads it from
      grafana.com, which an offline host can't reach. On 13.3+ the module links
      nixpkgs' `grafanaPlugins.prometheus` with `L`, which keeps a copy preinstall
      already fetched. Eval rows: linked on 13.3 and 14, not on 13.1.
    - The 13 trace panels read `default.otel_traces`, which only HyperDX's collector
      creates, so on a Grafana-only host they can never run. `obs-check --skip-table
      db.table` counts a query whose SQL names that table (as a whole, case-insensitive
      identifier) as skipped, not failed. Without `hyperdx.enable` the module's wrapper
      skips the collector's tables (eval row both ways). Tables: `valid_table_cases`,
      `names_table_cases` (a longer or prefixed name and a non-text field don't match;
      a quoted name isn't matched, so that query runs and fails rather than being
      hidden).
  - **Still open:**
    - O2: `obs-smoke`, a live read-only pass (Prometheus targets, Grafana panels,
      HyperDX tiles through its own connection, and a Playwright contact sheet that
      waits for tiles to fill).
    - A HyperDX log source for `agent.agent_logs`: the agents' logs are in ClickHouse,
      but HyperDX's only log source is `otel_logs` (Envoy's access log).
    - On l2 itself: the `node` and `mq-cake` scrape targets are down (no node exporter,
      nothing on 2112).

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
