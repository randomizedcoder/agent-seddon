# Review-fleet Phase 1 — implementation progress log

Crash-resilient running journal for the Foundation phase (R2 + R1). Finer-grained than
`STATUS.md` (which tracks increments); append-only decisions, updated after every meaningful
action so history survives a power loss. Plan of record:
`~/.claude/.../plans/ok-we-have-more-immutable-cookie.md`.

Three PRs, each based off `main`, never stacked, each gated by `nix flake check`:
**PR1 R2** identity-at-source → **PR2 R1a** per-session cwd → **PR3 R1b** creds mechanism.

---

## Now

- **Current PR:** PR2 — R1a per-session working directory
- **Branch:** `feat/review-fleet-r1a-cwd` (off main; #270/R2 merged as 4be29aa)
- **Current step:** PR2 · gate GREEN ("all checks passed!", exit 0); committing + opening PR
- **Last action:** `nix flake check` passed (exec/pty/loop regression green — fleet_root=None unchanged)
- **Next action:** commit + push + open PR2; on merge, start PR3 (R1b creds) on a fresh branch off main

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

## PR2 — R1a: per-session working directory  🟡 (code+tests done; gate running)

- [x] `resolve_cwd(key, fleet_root, fallback, opts)` — 3 branches; `CwdOpts.inherited_workspace` reserved for inc 8; returns `Result` (fail-closed) — `agent.rs`
- [x] Rewired `session_with` cwd source; `fleet_root=None` ⇒ `settings.cwd` (zero change when unset); infallible constructor keeps its logged fail-safe fallback (keys are pre-validated)
- [x] `Settings.fleet_root: Option<PathBuf>` + `[review_fleet] root` config (`config.rs` + wired in `builder.rs`; documented in `config/agent.toml`)
- [x] `confine` on the `working_dir` override; `path_under` (safe_segment) for the derived root; dir created `0700` (`ensure_dir_0700`)
- [~] `OpenRequest.working_dir` (secondary) — DEFERRED to a follow-up: `CwdOpts.working_dir` seam is wired + tested; the wire/proto threading through `SessionRegistry::open` is not needed for the fleet's primary path (branch 3). Noted below.
- [x] Tests: positive (derives/disjoint), boundary (working_dir within root), corner (fallback/reuse), adversarial (traversal ×4, symlink escape, working_dir escape) — 11 pass
- [x] Regression: exec_roundtrip / pty_e2e / loop_e2e green (via `nix flake check`)
- [x] `nix flake check` — GREEN ("all checks passed!"); clippy clean, fmt, constants-sync ok

## PR3 — R1b: per-session credentials mechanism  ☐

- [ ] `Secret(String)` newtype (redacting Debug/Display); adopt for forge token
- [ ] `resolve_token(inline, env, file)` (resolve_key_opt semantics: file-miss errors)
- [ ] Per-session `Forge` builder (vs global `shared_forge`)
- [ ] Git token injection at the `git_bytes` funnel (`agent-git/src/cli.rs:152`)
- [ ] Tests: positive/negative/corner + adversarial (no-leak, cross-session isolation)
- [ ] `nix flake check` green

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

## Gate status

- **2026-09-05** — PR1 (R2): `nix flake check` GREEN ("all checks passed!", exit 0). clippy -D
  warnings clean, fmt applied, buf additive, bench + leak passed (digest_query Ir held).
- **2026-09-05** — PR2 (R1a): `nix flake check` GREEN ("all checks passed!", exit 0). clippy clean,
  fmt, constants-sync ok; exec/pty/loop regression green (fleet_root=None path unchanged).

## l2 verification

- ClickHouse `ALTER TABLE ... ADD COLUMN user String` (7 tables): not yet applied.
- Scoped-session smoke + digest-scoping live check: not yet done.

## Open questions / blockers

- None.
