# Review-fleet Phase 1 — implementation progress log

Crash-resilient running journal for the Foundation phase (R2 + R1). Finer-grained than
`STATUS.md` (which tracks increments); append-only decisions, updated after every meaningful
action so history survives a power loss. Plan of record:
`~/.claude/.../plans/ok-we-have-more-immutable-cookie.md`.

Three PRs, each based off `main`, never stacked, each gated by `nix flake check`:
**PR1 R2** identity-at-source → **PR2 R1a** per-session cwd → **PR3 R1b** creds mechanism.

---

## Now

- **Current PR:** PR1 — R2 identity at the source
- **Branch:** `feat/review-fleet-r2-identity`
- **Current step:** PR1 · gate GREEN ("all checks passed!", exit 0); committing + opening PR
- **Last action:** `nix flake check` passed (digest_query bench Ir held, no ceiling bump)
- **Next action:** commit + push + open PR1; on merge, start PR2 (R1a per-session cwd) on a fresh branch off main

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

## PR2 — R1a: per-session working directory  ☐

- [ ] `resolve_cwd(key, opts)` (3 branches; branch 2 reserved for inc 8)
- [ ] Rewire `agent.rs:904` cwd source; `fleet_root=None` fallback to `settings.cwd`
- [ ] `fleet_root: Option<PathBuf>` knob + `[review_fleet] root` config
- [ ] `confine` on resolved path + working_dir override
- [ ] `OpenRequest.working_dir` (secondary) — proto + `SessionRegistry::open`
- [ ] Tests: positive/boundary/corner + adversarial (traversal, symlink escape, working_dir escape)
- [ ] Regression: exec_roundtrip / pty_e2e / loop_e2e green
- [ ] `nix flake check` green

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

## Gate status

- **2026-09-05** — PR1 (R2): `nix flake check` GREEN ("all checks passed!", exit 0). clippy -D
  warnings clean, fmt applied, buf additive, bench + leak passed (digest_query Ir held).

## l2 verification

- ClickHouse `ALTER TABLE ... ADD COLUMN user String` (7 tables): not yet applied.
- Scoped-session smoke + digest-scoping live check: not yet done.

## Open questions / blockers

- None.
