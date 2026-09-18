# 11 — The MI50 prompt-refresh job (LAST PHASE)

> **Round 3, last phase.** Keep the imported personalities fresh: when the locked peer
> sources change (a nixpkgs bump or the hermes input bump), re-scan each source tree,
> re-extract the system prompt, and upsert a **new version** into the store. Sources:
> [`09`](09-nixpkgs-sourcing.md); versioning it writes: [`08`](08-versioning-and-provenance.md).

## Why a job, and why the MI50

The peer prompts live in **four different formats** — opencode `.txt`, codex `.md`, pi
TS literal, hermes Python constant ([`09`](09-nixpkgs-sourcing.md)) — and the files
**move and rename between versions**. A brittle per-format scraper would break on every
upstream refactor. An LLM extractor normalizes across the heterogeneity and survives
drift: *"here is a source tree; return the agent's main system-prompt text."*

The **local MI50** does this — it is cheap, offline, and already the target for
non-critical LLM work. Routing reuses the shipped `RouteRole::Review`
([`agent-core/src/lib.rs:152`](../../../crates/agent-core/src/lib.rs)) via
`RouteHint { role: Some(Review) }`, exactly the pattern the review track's Stage-3
`digest_summary` uses to push cheap work to the MI50
([`agent-review/src/digest.rs`](../../../crates/agent-review/src/digest.rs),
[`agent-providers/src/route.rs`](../../../crates/agent-providers/src/route.rs)). No
remote Kimi tokens are spent keeping prompts fresh.

## Trigger: a fingerprint change

The `prompt-sources` derivation's output hash is a stable fingerprint of the current
peer-prompt corpus ([`09`](09-nixpkgs-sourcing.md)). The job:

1. computes/reads the current `prompt-sources` fingerprint;
2. compares it to the last-imported fingerprint (stored alongside the personalities);
3. if unchanged → **no-op** (idempotent; the common case);
4. if changed → re-extract and upsert.

Invoked via `nix run .#prompts-refresh` (manual, or from CI / a scheduler after a
`nix flake update`). It is **not** an always-on daemon — it runs on demand / on lock
change, so there is no per-turn cost.

## Pipeline (per personality)

```
prompt-sources/<peer>/…            (locked source tree, 09)
   │  read files (never execute peer code)
   ▼
MI50 extract+normalize             (RouteRole::Review) → candidate prompt text
   │  validate: size-cap, UTF-8, safe derived id, injection-screen
   ▼
compare to stored (kind=System, id=<peer>)
   │  identical content+source_ref → NO-OP (08)
   │  changed → put: version += 1, record source_ref (08)
   ▼
store (sqlite/grpc)                versioned, provenance-stamped, history-appended
```

- **The extractor is the untrusted-parser surface** — peer prompt text is external input
  a compromised/poisoned upstream could shape. Everything it emits is treated as data,
  fail-closed:
  - output **size-capped** (the shipped `MAX_CONTENT_BYTES`);
  - the derived store `id` is the **closed-set** personality name (`07`), never raw text;
  - content is **injection-screened at use** (it becomes a system message; the loop's
    `scan_for_injection` guards it — `08`);
  - a personality is **quarantined, not silently trusted**: an extraction that fails
    validation is flagged (recorded reason), not written as a live base.
- **Fail-soft, per peer:** a moved/renamed/absent file skips *that* peer with a recorded
  reason and does not abort the others (`negative_absent_or_moved_file`).
- **Provenance required:** an imported entry with no `source_ref` is rejected
  (`08`, `negative_missing_source_ref_on_imported`).
- **License-gated:** a peer whose license forbids redistribution is imported as a
  reference (source path + version, empty verbatim content) — [`09`](09-nixpkgs-sourcing.md#licensing).

## Telemetry (measurable, like every other lever)

An `agent_prompt_refresh_*` metric family in
[`agent-metrics`](../../../crates/agent-metrics/src/lib.rs) (mirror `on_iteration`):
`agent_prompt_refresh_runs_total`, `_updates_total{peer}`, `_skips_total{peer,reason}`,
`_quarantined_total{peer}`. So a refresh is auditable: what changed, what was skipped and
why, what was quarantined.

## Fuzzing the extractor (the repo's first real fuzz harness)

The extractor is a parser over external, drifting, untrusted input — the right place to
add genuine fuzzing (the repo has none today; its "fuzzing" is the mandatory
`adversarial_` table cases). Introduced with the same discipline as the first DB
dependency ([`05-storage.md`](05-storage.md)): feature-gated, pinned in
`nix/versions.nix`, `cargo-audit`ed, its own gated PR.

- **`proptest` property tests in the gate** (deterministic, bounded, hermetic — matches
  the repo's deterministic-gate ethos): over randomly-generated hostile byte inputs the
  extractor+normalizer **never panics**, **always size-caps** output, and **never emits a
  traversal/ref-special id**. A `nix/checks/prompt-fuzz.nix` executes them like
  `prompt-sqlite.nix`.
- **Optional `cargo-fuzz` (libfuzzer) target** for local deep fuzzing
  (`nix run .#fuzz-prompt-extractor`), kept out of the always-on gate (time-boxed /
  non-deterministic) or gated with a tiny fixed time-budget + committed seed corpus. The
  property-test gate is the load-bearing part; the libfuzzer target is for deep local
  runs.

## Change surface (Phase 5)

| File | Change |
|---|---|
| new module/crate | the extractor/normalizer (untrusted parser), fingerprint detector, upsert job |
| `crates/agent-providers/src/route.rs` (reuse) | `RouteRole::Review` routing to the MI50 |
| `crates/agent-cli` | the `--refresh-prompts` path behind `nix run .#prompts-refresh` |
| `crates/agent-metrics/src/lib.rs` | `agent_prompt_refresh_*` counters |
| `nix/checks/prompt-fuzz.nix` (new) | the `proptest` gate check |
| `nix/apps` / `nix/packages.nix` | `prompts-refresh` + optional `fuzz-prompt-extractor` apps |
| `nix/versions.nix` | pin `proptest` (+ `cargo-fuzz` if used) |

No wire change (the store fields are the additive ones from [`08`](08-versioning-and-provenance.md)).
Full test table in [`status/round3-05-refresh-job.md`](status/round3-05-refresh-job.md).
