# Increment 1 — per-session workspace + credentials isolation

Components: **C4** (workspace) · **C5** (credentials). This is the foundation; nothing else
in the fleet is safe to run in one process until a session's files and token are its own.
It also lands the long-deferred **multi-session 04d** (per-session cwd) and **05b**
(`OpenRequest.working_dir`).

## Problem

Two shared singletons make "N repos in one process" unsafe today:

1. **cwd is shared.** `Agent::session_with(key)` sets
   `tool_ctx: ToolContext { cwd: self.settings.cwd.clone() }` (`agent-runtime/src/agent.rs:905`),
   and every exec/pty inherits it (cloned per call at `agent.rs:1325`). Two sessions would
   clone, checkout, and run tools in the **same** directory — repo A stomps repo B.
2. **The forge is a single global.** `shared_forge` is built once from `cfg.forge.{token,
   token_env}` (`agent-runtime/src/builder.rs:431-441`) and reused everywhere (review path
   at `:935`). One token for all repos — no per-repo credential isolation.

## C4 — per-session workspace

### Change
Make the session's cwd the output of **one resolution function**, not a hardcoded
`path_under` — so the deferred child-session inheritance (increment 8, C21) plugs into the
middle branch later without reopening this increment:

```
resolve_cwd(key, opts) -> PathBuf:
    if opts.working_dir is Some:      confine(base / working_dir)          // explicit override
    else if opts.inherited_workspace: opts.inherited_workspace            // child (inc 8 fills this)
    else:                             key.path_under(fleet_root)?          // top-level default
```

- `Agent::session_with(key)` calls `resolve_cwd`. This increment wires branch 1 (explicit)
  and branch 3 (`key.path_under(fleet_root)` → `root/<user>/<session>`,
  `agent-core/src/identity.rs:166`, both segments `safe_segment`-validated), replacing
  `self.settings.cwd.clone()` (`agent-runtime/src/agent.rs:905`). Branch 2 is a documented
  hole that increment 8 supplies (`opts.inherited_workspace = None` for now).
- The directory is created (mode `0o700`) if absent; `ToolContext.cwd`, and thus every
  downstream `ExecSpec.cwd`/`PtySpec.cwd` (cloned per call at `agent.rs:1325`), is this path.
- Clone mirror + worktrees (C9) live under `<cwd>/` (`.mirror/`, `reviews/`, worktrees).

### Interface
- New `Agent` construction knob `fleet_root: Option<PathBuf>` (config `[review_fleet] root`,
  default under the state dir). When unset, `resolve_cwd` falls back to `settings.cwd` so
  non-fleet callers are untouched.
- `OpenRequest` gains `working_dir: Option<String>` (the field 05b deferred) → `opts.working_dir`.
  A caller may pin a subdir **within** the confined root; anything resolving outside is
  rejected.
- `resolve_cwd`'s `opts` type reserves `inherited_workspace` now (always `None` until inc 8),
  so the function signature is stable across the child-session build.

### Confinement (the load-bearing security boundary)
- Every `SessionKey` segment already passes `safe_segment` (rejects `..`, separators,
  leading `-`; ≤128 chars). Applied to `user` and `session` before `path_under`.
- The resolved cwd (and any `working_dir` override) goes through `confine()`
  (`agent-tools/src/lib.rs`) — canonicalize, then reject if it escapes `fleet_root` — never
  a lexical join alone. This blocks a symlink planted by attacker code from redirecting a
  later tool write outside the session.
- `bash` remains the intentional unconfined escape hatch, so it is **disabled or
  policy-restricted per fleet session** (multi-session 07b) — see increment 5/security.

## C5 — per-session credentials

### Change
Build a **session-scoped** `Forge` from the roster row's `token_ref`, replacing the reliance
on the single `shared_forge` for fleet sessions:

- `resolve_token(token_ref) -> Result<Secret>`: scheme `env:NAME` or `file:/path`, mirroring
  `resolve_key_opt` semantics — **env-miss = absent** (session runs read-only / disabled),
  **file-miss = hard error** (fail closed, session stays disabled with a reason).
- The resolved secret is injected into (a) the session's `Forge` backend and (b) the git
  child-process env for that session's clones/fetches (via `EnvPolicy`), and **nowhere
  else**.

### Interface
- Roster row (C2) carries `token_ref` + `base_url`; the fleet builds one `Forge` per session
  at admission, cached on the session handle.
- Non-fleet paths keep using `shared_forge` — this is additive.

### Non-leak guarantees
- The `Secret` newtype's `Debug`/`Display` redact; the token is never logged, never included
  in a `SessionEvent`, never written into a draft (C13 runs a redaction pass anyway).
- A token resolved for session A is never visible to session B (separate `Forge` instances,
  no shared mutable state).

## Test matrix (table-driven, per CLAUDE.md)

C4 confinement — `agent-runtime` / `agent-tools`:
- `positive_cwd_derives_from_session_key` — cwd == `root/<user>/<session>`.
- `positive_two_sessions_get_disjoint_cwds`.
- `boundary_working_dir_within_root_is_accepted`.
- `negative_missing_fleet_root_falls_back_to_settings_cwd`.
- `corner_repeated_open_same_key_reuses_dir`.
- `adversarial_traversal_session_id_rejected` (`..`, `a/b`, leading `-`).
- `adversarial_symlink_escape_blocked` — plant a symlink in the session dir → confine
  rejects a write through it.
- `adversarial_working_dir_escaping_root_rejected`.

C5 credentials — `agent-runtime`:
- `positive_token_from_file_ref_resolves`.
- `positive_token_from_env_ref_resolves`.
- `negative_file_ref_missing_is_hard_error` (fail closed).
- `corner_env_ref_missing_is_absent_not_error`.
- `adversarial_token_never_appears_in_debug_or_logs` — format the session/forge, assert the
  secret substring is absent.
- `adversarial_session_a_token_not_visible_to_session_b`.

## Done when

`nix flake check` green; two concurrent fleet sessions clone into disjoint confined dirs;
each uses its own token; a traversal/symlink attempt is rejected; a missing `file:` token
fails the session closed without touching others. No change to non-fleet behavior.
