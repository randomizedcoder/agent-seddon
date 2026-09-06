# Review-fleet Phase 1 — implementation progress log

Crash-resilient running journal for the Foundation phase (R2 + R1). Finer-grained than
`STATUS.md` (which tracks increments); append-only decisions, updated after every meaningful
action so history survives a power loss. Plan of record:
`~/.claude/.../plans/ok-we-have-more-immutable-cookie.md`.

Three PRs, each based off `main`, never stacked, each gated by `nix flake check`:
**PR1 R2** identity-at-source → **PR2 R1a** per-session cwd → **PR3 R1b** creds mechanism.

---

## Now

- **Current PR:** inc **3b** — **C3** gRPC control plane (`ReviewFleetService`)
- **Branch:** `feat/review-fleet-inc3b-control-plane` (off main 91c40ab; inc 3a C2 #278 merged)
- **Current step:** code + tests complete; **`nix flake check` GREEN** ("all checks passed!")
- **Last action:** `review_fleet.proto` + `ReviewFleetService` (server + `GrpcFleet` client) +
  convert.rs core↔pb (token_ref verbatim, numbers clamped on decode) + FLEET constants
  (50086/9636/fleet.sock) + `GrpcCfg.fleet` + `Seam::Fleet` wiring in grpc_server.rs +
  `resolve_fleet_registry` factory + builder wiring + runtime `fleet`/`fleet-sqlite` features;
  table-driven tests: convert round-trip (agent-proto) + real-wire CRUD/token-never-returned/
  traversal-rejected roundtrip (agent-grpc, TCP+UDS); docs control-plane section
- **Next action:** confirm gate GREEN, then commit (awaiting go-ahead); then 3c (server + FSM) off
  updated main.

Inc **3a (C2) DONE + MERGED (#278, main 91c40ab).**

Inc 2 (C9) DONE + MERGED (#277, main ea52e2c). Now the **fleet build proper**, sliced into
**three gated PRs** off main (never stacked): **3a roster (C2)** → **3b control plane (C3)** →
**3c server + FSM skeleton (C1 + C8)**. Plan file
`~/.claude/.../plans/ok-we-have-more-immutable-cookie.md`.

---

## Increment 3b (C3) — gRPC control plane  🟢 (gate green; ready to commit)

Second of the three fleet-core PRs. Live CRUD over the 3a roster: `ReviewFleetService` +
its client, so a roster is editable at runtime and one process can serve it. **Additive**
throughout (a new proto file/service, new endpoint, new default-empty config field) — buf stays
additive (no `buf.image.binpb` bump); nothing existing changes shape.

- [x] `review_fleet.proto` (`agent-proto`, package `agent.v1`): `FleetSession` (13 fields ==
  the core row), `FleetListRequest`/`FleetSessionList`/`FleetSessionRef`/`FleetDeleteReply`/
  `FleetSetEnabledRequest`; `service ReviewFleetService { List/Get/Put/Delete/SetEnabled }`;
  security contract in the file header (`token_ref` reference-not-secret, untrusted ids). Added to
  `build.rs`. Additive → `buf breaking` passes, **no baseline bump**
- [x] `convert.rs` — `From<core::FleetSession> for pb::FleetSession` and back; `token_ref` copied
  **verbatim** (never resolved); `pb→core` runs `sanitize()` (poll clamp, timestamps floored)
- [x] Server `agent-grpc/src/server/review_fleet.rs` (`ReviewFleetSvc`, mirrors
  `provider_registry.rs`): per-RPC `span` + trait call + `status_from_error` (`not found`→NotFound,
  validation→InvalidArgument); `Put` decodes wire→core (clamps) then the store validates fail-closed;
  `Get`/`List` return `token_ref` only. Registered in `server/mod.rs`
- [x] Client `agent-grpc/src/client/review_fleet.rs` (`GrpcFleet`, `unary!` + retry): fails hard
  (`Err`, no silent no-op); `status_to_err` preserves the `not found` contract across a chained
  `grpc→grpc` hop. Registered in `client/mod.rs`
- [x] Constants: `nix/constants.nix` `fleet` block (**50086/9636/fleet.sock** — next free after
  `config`; NOT the design's 50081/9631 which belong to `digest`) + `gen-constants.nix` `seamConst`
  render; `agent-grpc/src/constants.rs` regenerated (constants-sync enforces)
- [x] `GrpcCfg.fleet: GrpcSeamCfg` (`#[serde(default)]`, endpoint/listen) — `config.rs`
- [x] `grpc_server.rs`: `Seam::Fleet` variant + `ALL_SEAMS` entry + `SEAMS` row (`--serve-fleet`,
  `agent.v1.ReviewFleetService`, `constants::FLEET`) + `configured_listen` arm + `build_router` arm
  (served only when `agent.fleet_registry()` is `Some`)
- [x] `resolve_fleet_registry(cfg)` factory (`builder.rs`, next to `resolve_provider_registry`):
  string-match `store` (`""`→None / `file` / `sqlite` double-cfg-gated + `bail!` / `grpc` client /
  `bail!` unknown) + builder wiring `with_fleet_registry`; Agent `fleet_registry` field + accessor
  (added with the field so accessors are always present, only the builder wiring is feature-gated)
- [x] Runtime `Cargo.toml`: `fleet = ["dep:agent-review-fleet"]` (added to default features — inert
  when `store=""`), `fleet-sqlite = ["fleet", "agent-review-fleet/fleet-sqlite"]`; `agent-grpc`
  dev-dep on `agent-review-fleet` for the wire test
- [x] Tests (table-driven, desc+expect): convert round-trip identity + hostile-poll/timestamp clamp
  (`agent-proto`); real-wire CRUD roundtrip (Put clamps poll → Get/List → SetEnabled → Delete +
  benign second-delete), `adversarial_` token-never-returned + bad-row-rejected (traversal/separator/
  raw-token) + unknown-id→NotFound, all over **TCP + UDS** (`agent-grpc/tests/roundtrip.rs`)
- [x] `docs/components/review-fleet.md` — control-plane section (service, `--serve-fleet`,
  `store="grpc"`, the two cross-wire invariants); note deferred to 3c: `metered::fleet_registry`
  gauge, and the dedicated `Mode::ServeFleet` orchestrator process (distinct from the seam-harness
  `--serve-fleet` that hosts only the roster CRUD)
- [x] `nix flake check` — **GREEN** ("all checks passed!"): buf additive (no `buf.image.binpb`
  bump — new file/service/RPCs only), constants-sync, clippy -D warnings, tests, fmt all clean.
  Gate surfaced + fixed: new files git-tracked (flake requirement), rustfmt on the wire test,
  `agent-review-fleet` dev-dep for the roundtrip; repo helper uses the `acme__web` safe-segment
  convention (a `/` in a repo fails `safe_segment`, by design)

---

## Increment 3a (C2) — persisted roster  ✅ (PR #278, main 91c40ab)

First of the three fleet-core PRs. The durable list of "who reviews what" + its validation core —
no wire, no server (those are 3b/3c). Additive: a new crate + a new seam; nothing existing changes
shape.

- [x] `agent_core::FleetSession` — the full C2 row (id/user/repo/backend/base_url/token_ref/skill/
  slack trigger+progress channels/poll_secs/enabled/created_at/updated_at) so later increments only
  *read* the extra fields; `derive(Serialize, Deserialize, PartialEq, Eq)`; `sanitize()` (poll clamp
  to `[MIN,MAX]`, `0⇒DEFAULT`, timestamps floored ≥0) + `validate()` (fail-closed) — `agent-core/src/lib.rs`
- [x] `agent_core::FleetRegistry` trait (list/get/put/delete/set_enabled) — seam contract mirrors
  `ProviderRegistry`: unknown `get`/`set_enabled` → `Err` starting `not found`; `delete` unknown ⇒
  `Ok(false)`. New `Error::Fleet(String)` variant (`not found` → NotFound, rest → InvalidArgument)
- [x] Decision: `token_ref` **reuses the audited `ApiKeyRef::parse`** (`env:`/`file:`/none, never a
  raw token, never echoes the value) — a forge token is a secret; no near-duplicate parser
- [x] Fleet caps in agent-core: `MAX_FLEET_ROWS=512`, poll bounds (`MIN=30`/`MAX=86400`/`DEFAULT=300`),
  `MAX_FLEET_NAME_LEN=256`; id/user/repo via existing `safe_segment` (MAX_SEGMENT_LEN=128)
- [x] New crate `crates/agent-review-fleet` (workspace member + `[workspace.dependencies]` entry):
  private `mod ops` (the single sanitize→validate→cap path all backends funnel through, + `revalidate`
  for out-of-band-edit defense) + `MemoryFleet` / `FileFleet` (atomic JSON bundle, temp+rename) /
  `SqliteFleet` (feature `fleet-sqlite`, row-as-JSON in a BLOB, bound-param ids)
- [x] `ReviewFleetCfg` gains roster-store fields (`store`/`file`/`path`/`max_total`/`max_per_user`;
  explicit `Default` matching the serde defaults) — kept but **unconsumed in 3a**; the
  `resolve_fleet_registry` factory + runtime `fleet` features land in **3b** (consumed by
  `build_router` — adding them in 3a would be dead code under `-D warnings`)
- [x] Tests (table-driven, `desc`+`expect` per row, `#[cfg(test)] mod` at file end): unified
  `crud_contract` table (all op classes, positive/negative/boundary/corner + adversarial-traversal),
  poll-clamp boundary table, raw-token-not-echoed, over-cap, per-backend equivalence (memory=file=
  sqlite), file absent/oversized/out-of-band-tamper, sqlite reopen/tamper/bound-params — 40/40 green
- [x] `nix/checks/fleet-sqlite.nix` (feature-scoped, mirrors `prompt-sqlite.nix`) registered in
  `nix/checks/default.nix`; default-feature tests run via `test.nix` automatically
- [x] `docs/components/review-fleet.md` — the FleetRegistry seam doc (trait, row, token-ref rule,
  backends, testing)
- [x] `nix flake check` — **GREEN** ("all checks passed!", exit 0): clippy -D warnings, tests, buf
  additive (no baseline bump — no proto change), bench + leak, fmt, constants-sync, coverage all clean.
  Gate surfaced + fixed: new files git-tracked (flake requirement), unused `serde` dep (cargo-machete),
  stale Cargo.lock (`--locked`), the new `Error::Fleet` non-exhaustive in `convert.rs::status_from_error`
  (mapped like `Registry`: `not found`→NotFound else InvalidArgument — no proto/wire change), missing
  `default_fleet_{file,path}` fns + explicit `impl Default for ReviewFleetCfg`

---

## Increment 2 (C9) — PR fetch + checkout  🟡 (code+tests done; gate running)

The gap: the git seam could mirror + worktree an *already-resolvable* revision but could not
fetch a **PR head ref** (`refs/pull/<N>/head` / `refs/merge-requests/<N>/head` — not in a default
`git fetch`), and a `PullRequest` carries only a branch name, no head SHA. So `agent --review <PR#>`
was **broken on a fresh clone** (`resolve` → branch names → `rev-parse --verify` fails, always for
fork PRs). Additive, low-risk, single gated PR off `main`.

- [x] `RepoBackend::fetch_pr(number) -> Revision` — **default body** (`Err`, matching the
  optional-op idiom so gRPC/remote backends compile unchanged) — `agent-core/src/lib.rs`
- [x] `pub fn pr_local_ref(number)` — single source of truth for the namespaced local ref
  (`refs/fleet/pr/<n>`), shared by the impl (writes) + orchestrator present-check (reads) — `agent-core`
- [x] `CliBackend::fetch_pr` + `with_pr_ref_template` builder + `pr_ref_template` field — empty
  template ⇒ `Err(Config)`; `ensure_mirror`; `{n}`-substituted refspec via the R3c sandbox funnel;
  head resolved from `base()` (the DB the ref lands in), not `self.root` — `agent-git/src/cli.rs`
- [x] Decision: PR ref convention = **operator config** (`[git] pr_ref_template`), forge-agnostic —
  keeps `RepoBackend`/`Forge` separated, covers self-hosted layouts
- [x] `GitCfg.pr_ref_template` (`#[serde(default)]`) + `build_repo` wiring — `agent-runtime/{config,git}.rs`
- [x] `MeteredRepo::fetch_pr` delegating override (a defaulted trait method the decorator MUST
  forward or it silently shadows the real impl) — `agent-runtime/src/metered.rs`
- [x] Orchestrator Pr arm: fetch-if-missing, head resolved **fork-correctly** via the namespaced PR
  ref (never `source_branch`, which can collide with a local branch) — `agent-review/src/orchestrator.rs`
- [x] `config/agent.toml` — `pr_ref_template = "refs/pull/{n}/head"` active + GitLab alt commented
- [x] Tests: agent-git unit (recording-sandbox: refspec build, GitLab template, empty→err+no-spawn,
  safe-segment) + real-repo integration (resolve head oid, worktree files, idempotent refetch,
  unknown-PR soft error, `u64::MAX` boundary, advisory read-only) + agent-review orchestrator
  (fetch-when-missing, skip-when-present) — ALL PASS
- [x] `nix flake check` — **GREEN** ("all checks passed!", exit 0): clippy -D warnings, tests, buf
  additive (no baseline bump), bench + leak, fmt, constants-sync, coverage all clean

---

## PR1 — R2: identity at the source  🟡 (code+tests done; gate running)

- [x] `MemoryEvent` gains `user: String` (`#[serde(default)]`, telemetry-local) — `agent-core/src/lib.rs`
- [x] Wire decision: `user` NOT wire-carried (telemetry sink runs in-process before the memory composite); `convert.rs` wire→core defaults it. No proto/buf change.
- [x] Stamp `user`/`session` from `current_identity()` — extracted `stamp_identity()`, called in `Agent::append_event`
- [x] Verified funnel is on the scoped loop task; all ~8 `MemoryEvent` literals updated (`user: String::new()`)
- [x] Added `user` to all 7 `Row` structs + builders — `agent-telemetry/src/rows.rs`; `LogRow::new` gained a `user` param
- [x] `LogRow` identity from `current_identity()` (fallback to process handle when no scope) — `layer.rs`
- [~] Per-span `session`/`user` attribute — DEFERRED to fast-follow (needs OTEL SpanProcessor via Context, not the task-local); documented in `otel.rs`. Rows+logs carry `user` (the RLS-relevant surface).
- [x] `schema.sql` — added `user String` to 7 CREATE TABLEs + header note w/ manual `ALTER TABLE` recipe
- [x] Digest cross-tenant read fix: `DigestQuery.user_id`; WHERE in BOTH backends (`clickhouse.rs` + `sqlite.rs` bind param); `sanitize_query` validates uid; seeds fixed (`distiller.rs latest_summary`, `instant.rs`, gRPC `server/digest.rs`)
- [x] Metrics dump: documented as known Tier-0 exposure (scoping deferred to MT-02) — `agent-tools/src/metrics.rs`
- [x] Tests: stamp (scope/no-scope), all-7-rows-carry-user, sqlite scoped-read isolation + unscoped, sanitize_query hostile uid — ALL PASS
- [x] `nix flake check` — GREEN ("all checks passed!"); buf additive (no baseline bump); digest_query bench Ir held

## PR2 — R1a: per-session working directory  ✅ (PR #271, gate green)

- [x] `resolve_cwd(key, fleet_root, fallback, opts)` — 3 branches; `CwdOpts.inherited_workspace` reserved for inc 8; returns `Result` (fail-closed) — `agent.rs`
- [x] Rewired `session_with` cwd source; `fleet_root=None` ⇒ `settings.cwd` (zero change when unset); infallible constructor keeps its logged fail-safe fallback (keys are pre-validated)
- [x] `Settings.fleet_root: Option<PathBuf>` + `[review_fleet] root` config (`config.rs` + wired in `builder.rs`; documented in `config/agent.toml`)
- [x] `confine` on the `working_dir` override; `path_under` (safe_segment) for the derived root; dir created `0700` (`ensure_dir_0700`)
- [~] `OpenRequest.working_dir` (secondary) — DEFERRED to a follow-up: `CwdOpts.working_dir` seam is wired + tested; the wire/proto threading through `SessionRegistry::open` is not needed for the fleet's primary path (branch 3). Noted below.
- [x] Tests: positive (derives/disjoint), boundary (working_dir within root), corner (fallback/reuse), adversarial (traversal ×4, symlink escape, working_dir escape) — 11 pass
- [x] Regression: exec_roundtrip / pty_e2e / loop_e2e green (via `nix flake check`)
- [x] `nix flake check` — GREEN ("all checks passed!"); clippy clean, fmt, constants-sync ok

## PR3 — R1b: per-session credentials mechanism  🟡 (code+tests done; gate running)

- [x] `Secret(String)` newtype (redacting Debug/Display, transparent serde, `expose()`) — `agent-core/src/security.rs`
- [x] Adopted `Secret` through the forge: `ForgeHttp.token`, `GitHubForge::new`/`GitLabForge::new` (all callers + http_e2e updated)
- [x] `resolve_token(inline, env, file) -> Result<Secret>` (inline>env>file; env-miss absent; file-miss hard error) — `registry.rs`; `token_file` added to `ForgeCfg` + wired into both forge factories; documented in `config/agent.toml`
- [~] Per-session `Forge` builder (vs global `shared_forge`) — DEFERRED to inc 3: the per-session token SOURCE is the roster; building a per-identity Forge now would be dead code. The resolution mechanism (`resolve_token`) it will call is in place.
- [~] Git token injection at the `git_bytes` funnel — DEFERRED to inc 3 (see decision below): safe injection is host-scoped, and the host+token are exactly the roster's per-repo fields.
- [x] Tests: Secret (redaction/expose/deserialize/empty), resolve_token (inline/file/file-miss/absent/no-leak) — all pass; http_e2e still green
- [x] `nix flake check` — GREEN ("all checks passed!", exit 0); clippy -D warnings clean, fmt, buf additive, bench + leak held

---

# Phase 2 — R3 (exec chokepoint, C24) + R4 (org tier, C25)

Four PRs, each off `main`, gated: **R3a** seam foundation → **R3b** route rg + guard + Tier-0
policy + pty-scrub → **R3c** git funnel → **R4** org-tier convention. R3a is additive (nothing
routes yet); R3c isolates the widest blast radius (git); R4 is light + independent.

## PR4 — R3a: Sandbox argv exec + bytes output + env-scrub  🟡 (code+tests done; gate running)

- [x] `ExecSpec.argv: Vec<String>` + `ExecSpec::argv(...)` ctor — argv mode runs the program
  directly (no shell); empty ⇒ existing `bash -c` shell mode (`agent-core/src/lib.rs`)
- [x] `ExecOutput.stdout_bytes: Vec<u8>` (+ `Default` derive); `stdout` stays the lossy view — so
  the git funnel can read binary objects through the seam (`agent-core/src/lib.rs`)
- [x] `run_argv` honors `EnvPolicy::Scrub` (`env_clear` + minimal `PATH`) + captures exact bytes;
  `NetworkPolicy` still unenforced (documented — arrives with C23 backends) (`agent-sandbox/src/lib.rs`)
- [x] `LocalSandbox`/`NixSandbox` dispatch argv-vs-shell (`local.rs`, `nix.rs`)
- [x] Proto additive: `ExecRequest.argv=6`, `ExecResult.stdout_bytes=5`; both convert dirs
  (older-server fallback = derive bytes from string) — buf additive, no baseline bump
- [x] Updated all `ExecOutput`/`ExecSpec` literals (go.rs, structural_search, metered, exec_roundtrip)
- [x] Tests: argv-no-shell, shell-unchanged, env-scrub-removes-HOME, scrub-keeps-PATH,
  stdout_bytes-binary-exact, timeout; gRPC argv+bytes roundtrip (tcp+uds) — all pass
- [x] `nix flake check` — GREEN ("all checks passed!", exit 0); clippy -D warnings clean, fmt, buf additive (no baseline bump), bench + leak held

## PR5 — R3b: route `rg` + no-raw-`Command` guard + Tier-0 policy + pty env-scrub  🟢 (gate green; ready to push)

- [x] `GrepTool` holds `Arc<dyn Sandbox>`; `rg` fast path builds `ExecSpec::argv([...])` (no shell)
  + reads `stdout_bytes`; Err ⇒ fall back to in-process walk (unchanged). Moved from a registry
  factory to builder-wiring from `shared_sandbox` (like `bash`); added to `is_builder_registered_tool`
  + allowlist; `find`/`ls` stay factories (no spawn) (`search.rs`, `registry.rs`, `builder.rs`)
- [x] **Tier-0 `DenyTools` policy** (fail-closed overlay: deny globs → base) + `[policy] deny_tools`
  config + builder wraps base_policy when non-empty; documented in `config/agent.toml`. A fleet session
  sets `deny_tools = ["bash","pty"]` (`policy.rs`, `config.rs`, `builder.rs`)
- [x] **PTY env-scrub**: `PtySpec.env: EnvPolicy` (default Inherit) + `PtyOpenRequest.env=6` proto
  (additive) + convert; LocalPty spawn `env_clear` + minimal PATH + TERM on Scrub (`agent-core`,
  `exec.proto`, `convert.rs`, `agent-pty/src/lib.rs`). PtySpec literals use `..Default::default()`
- [x] **No-raw-`Command` guard** — `agent-tools/tests/no_raw_spawn.rs`: scans production src (pre
  `#[cfg(test)]`) of chokepointed crates for `Command::new`; R3b scope = agent-tools (grows to
  agent-git/agent-search in R3c); agent-sandbox seam + agent-pty streaming spawn = standing exceptions
- [x] Tests: grep suite (72) green through LocalSandbox; DenyTools (deny/cannot-widen/star); pty
  scrub-hides-HOME; guard passes — all green
- [x] `nix flake check` — **GREEN** (exit 0, "all checks passed!", all 30 checks incl. leak/bench/buf).
  First run surfaced a missed `pb::PtyOpenRequest` literal in `agent-grpc/tests/exec_roundtrip.rs`
  (adversarial_absurd_dimensions_are_clamped) — needed the new `env` field; fixed
  (`env: ExecEnvPolicy::Inherit as i32`), exec_roundtrip 25/25 green; re-run clean

Deferred to C23 (documented): full PTY-under-a-real-sandbox backend (streaming exec variant);
manifest/consensus git routing → R3c.

## PR6 — R3c: route the `git` funnel through the seam (binary fidelity + capped timeout)  🟢 (gate green; ready to push)

- [x] `CliBackend` gains `sandbox: Arc<dyn Sandbox>` (default `LocalSandbox`) + `with_sandbox`
  builder; `git_bytes` now builds `ExecSpec::argv(["git","-C",cwd,...args], cwd).timeout(600)` and
  calls `self.sandbox.exec`, reading `stdout_bytes`. Non-zero exit / `timed_out` → `Err` (preserves
  today's semantics incl. `grep`'s swallowed exit-1). argv mode = untrusted ref/path never shelled
  (`agent-git/src/cli.rs`; `agent-git` gains `agent-sandbox` dep, features `sandbox-local`)
- [x] `crate::git::build_repo` gains a `sandbox` param; `builder.rs` threads `shared_sandbox` (falls
  back to `LocalSandbox` when tool-core off) into the `cli`/`hybrid` construction (`git.rs`, `builder.rs`)
- [x] **No-raw-`Command` guard extended**: `SCANNED` += `agent-git/src` (fully routed, zero allowlist)
  + `agent-search/src`; `ALLOWED` += `agent-search/src/manifest.rs` (documented: sync, fixed-arg,
  read-only index probe — no model input, no async seam) (`agent-tools/tests/no_raw_spawn.rs`)
- [x] Consensus `git_diff_evidence` (`builder.rs`) left a raw spawn **by decision** (sync
  `EvidenceSource` closure over the agent's OWN working tree, fixed args) — documented in-code; not
  in guard scope (guard scans tool/git/search, not runtime)
- [x] Docs: `agent-git/src/cli.rs` module header + `docs/components/sandbox.md` (chokepoint wiring
  now names rg + git; futures list updated)
- [x] Tests: real-repo binary blob byte-exact via `stdout_bytes` (objects_fixture, 14); recording-
  sandbox unit tests — argv-mode/no-shell + 600s cap, timeout→Err, nonzero→Err (cli.rs, 31 lib);
  guard passes (agent-git routed, manifest allowlisted); all existing objects_fixture now run through
  `LocalSandbox` (free regression)
- [x] `nix flake check` — **GREEN** (exit 0, "all checks passed!", all 30 checks incl. git-heavy
  review/e2e suites, leak/bench/buf-additive)

## PR7 — R4: org tier convention + session-id encoding + semantics docs  🟢 (gate green; ready to push)

- [x] `SessionKey` doc: org-tier convention (`user=<org>`, `session=<repo>+<pr>`; hierarchy
  `host ⊃ org ⊃ repo+pr ⊃ child`; single-level, `org→team→user` a non-goal) — `agent-core/src/identity.rs`
- [x] `encode_review_session_id(repo, pr) -> SessionId` — sanitize repo to charset + `-pr<n>`;
  valid-by-construction (non-empty, no leading `-`/`.`, ≤MAX_SEGMENT_LEN). NO `safe_segment` widening
  (raw `repo@pr` stays rejected). Auto-exported via `pub use identity::*`
- [x] Semantics re-meanings documented at their sites (no behaviour change): per-user session cap →
  **per-org** (`session_manager.rs` per_user filter comment); metrics `user` label → **org**, still
  session-coarse so cardinality budget holds (`agent-metrics/src/lib.rs`)
- [x] Design: multi-tenancy `STATUS.md` (plane 01 → 🟡: C24 ✅, C25 foundation 🟡, C23 ⬜) +
  `00-components.md` C25 "Landed (R4)" note (deferred: org-value mint-site inc 3, hard DB/netns, 3rd tier)
- [x] Tests (identity.rs mod, file end): user-as-org disjoint `path_under`; encoder → safe + parse-safe
  + carries `-pr<n>`; raw `repo@pr` rejected; pathological repo names (all-sep fallback, over-long cap) — 4 pass
- [x] `nix flake check` — **GREEN** (exit 0, "all checks passed!"). 1st run hit the known flaky pty
  `positive_cursor_resumes_without_replaying` (coverage check, `agent-pty` — untouched by R4; 3/3 in
  isolation); clean on retry

R4 is deliberately **light** (convention + encoding + semantics). The org *value* injection at the
fleet mint-site is deferred to fleet core inc 3; row/digest flow under `user=org` is mechanically
identical to R2's tested user-scoping (org is just the user value).

---

## Decisions log (append-only)

- **2026-09-05** — Phase 1 sliced as 3 PRs (R2 → R1a → R1b); creds mechanism built now, roster-wired
  in inc 3 (user choice).
- **2026-09-05** — Telemetry identity column named **`user`** (mirrors `SessionMetrics (session,user)`
  labels; `tenant == user == SessionKey.user` at this tier). `session_id` kept as-is.
- **2026-09-05** — Stamp identity once at the `append_event` funnel, not at ~8 `MemoryEvent`
  constructors (fields `#[serde(default)]`).
- **2026-09-05** — Defer ClickHouse ORDER-BY/sort-key change (tenant-leading) to MT-02; Phase 1 only
  ADDs the column (additive, name-mapped rows).
- **2026-09-05 (PR2/R1a)** — `session_with` kept INFALLIBLE (it's called in ~20 places incl. the
  synchronous `SessionManager::open`); `resolve_cwd` returns `Result` so adversarial tests assert
  rejection directly, and `session_with` logs + falls back to the shared cwd on error (unreachable for
  the pre-validated keys that actually reach it — belt-and-suspenders, no worse than today's shared cwd).
- **2026-09-05 (PR2/R1a)** — `OpenRequest.working_dir` wire threading DEFERRED. The `CwdOpts.working_dir`
  branch is built + tested (confine within root); the proto/`SessionRegistry::open` plumbing isn't needed
  for the fleet's primary mechanism (branch 3 = `fleet_root` + `path_under`). Follow-up if a caller needs
  to pin a subdir on open.
- **2026-09-05 (PR3/R1b)** — Git-env token injection DEFERRED to inc 3 (roster). Rationale: safe git auth
  is HOST-SCOPED (`http.<host>.extraHeader`), and the host+token are precisely the roster's per-repo
  fields; injecting the global token into all git ops now would be fail-open (header sent to any remote)
  or would require a separate `[git]` credential+host that duplicates what the roster carries. The single
  injection point (`git_bytes`, `agent-git/src/cli.rs`) is identified; inc 3 wires it per-session via
  `GIT_CONFIG_*` env (not argv → not ps-visible). R1b ships the credential-HANDLING core (Secret +
  resolve_token + forge adoption), which is live and tested now.
- **2026-09-05 (PR3/R1b)** — Config-struct `Debug` redaction (`ForgeCfg.token`) left as `String` to avoid
  dragging `schemars`/`Serialize` for `Secret` across the crate boundary (config-schema feature). The
  RESOLVED, in-memory token (the real leak surface — `ForgeHttp`, logs, spans) is a `Secret`; the inline
  config token is usually empty (env/file preferred). Minor; revisit if config Debug is ever logged.

## Gate status

- **2026-09-05** — PR1 (R2): `nix flake check` GREEN ("all checks passed!", exit 0). clippy -D
  warnings clean, fmt applied, buf additive, bench + leak passed (digest_query Ir held).
- **2026-09-05** — PR2 (R1a): `nix flake check` GREEN ("all checks passed!", exit 0). clippy clean,
  fmt, constants-sync ok; exec/pty/loop regression green (fleet_root=None path unchanged).
- **2026-09-05** — PR3 (R1b): `nix flake check` GREEN ("all checks passed!", exit 0). clippy -D
  warnings clean, fmt applied, buf additive (no baseline bump), bench + leak held. Secret redaction +
  resolve_token + forge adoption + http_e2e all green.

## l2 verification

- ClickHouse `ALTER TABLE ... ADD COLUMN user String` (7 tables): not yet applied.
- Scoped-session smoke + digest-scoping live check: not yet done.

## Open questions / blockers

- None.
