# 09 — Sourcing peer prompts from nixpkgs (and one locked input)

> **Round 3.** Where the imported personalities' text comes from, reproducibly, and how
> it refreshes when we bump nixpkgs. The extractor that *reads* these trees is
> [`11-refresh-job.md`](11-refresh-job.md); provenance recording is
> [`08`](08-versioning-and-provenance.md).

## Goal

Import each peer's system prompt from a **locked, content-addressed source** so that:

1. the imported personality is **reproducible** (same lock ⇒ same bytes), and
2. it **refreshes automatically** when we bump the lock (`nix flake update`) — the
   refresh job ([`11`](11-refresh-job.md)) detects the change and re-imports.

## The `.src` rule: read source, never build output

Three of the four peers are packaged in nixpkgs and build **from a `fetchFromGitHub`
`src`**, so their raw prompt files are reachable as `<pkg>.src` — a store path that
changes exactly when nixpkgs bumps the pinned version. Reading must be from **`.src`**,
**never `$out`**:

- **codex** `include_str!`-embeds its `.md` prompts into the binary → `$out` has no
  prompt files.
- **opencode**/**pi** bundle/transpile their prompts into JS → `$out` has no clean files.

Only `.src` has the parseable originals. (Full paths in
[`../../reference/peer-harnesses.md`](../../reference/peer-harnesses.md).)

| Peer | nixpkgs attr | Source path within `.src` | Format |
|---|---|---|---|
| opencode | `opencode` | `packages/opencode/src/session/prompt/*.txt` | plain `.txt` (clean) |
| codex | `codex` | `codex-rs/models-manager/prompt.md`, `codex-rs/core/*_prompt.md` | `.md` |
| pi | `pi-coding-agent` | `packages/coding-agent/src/core/system-prompt.ts` | TS literal (needs extraction) |
| hermes | — (not packaged) | `agent/prompt_builder.py` (`DEFAULT_AGENT_IDENTITY`, `*_GUIDANCE`) | Python constants |

## hermes: a dedicated locked input

hermes is not in nixpkgs, so it is pinned as its own **`flake=false`** input — the
pattern `flake.nix` already uses for `advisory-db` and the `xtcp2-*` eval corpus:

```nix
# flake.nix :: inputs
hermes-agent-src = {
  url = "github:nousresearch/hermes-agent/<commit-sha>";   # pinned by commit
  flake = false;                                            # + nar hash in flake.lock
};
```

`flake.lock` records the commit **and** the nar hash, so it is as reproducible as any
nixpkgs `.src`; `nix flake update` bumps it the same way.

## The prompt-sources derivation

A small nix derivation gathers the four raw prompt trees into one predictable place for
the extractor to read — it copies (does not build) the relevant sub-trees:

```nix
# nix/prompt-sources.nix  (sketch)
pkgs.runCommand "agent-prompt-sources" { } ''
  mkdir -p $out/{opencode,codex,pi,hermes}
  cp -r ${pkgs.opencode.src}/packages/opencode/src/session/prompt        $out/opencode/
  cp -r ${pkgs.codex.src}/codex-rs/models-manager/prompt.md              $out/codex/
  cp -r ${pkgs.codex.src}/codex-rs/core                                  $out/codex/core
  cp    ${pkgs.pi-coding-agent.src}/packages/coding-agent/src/core/system-prompt.ts  $out/pi/
  cp    ${hermes-agent-src}/agent/prompt_builder.py                      $out/hermes/
''
```

- Exposed as a flake output and to the refresh job via an env var (mirroring
  `AGENT_NIXPKGS_FLAKEREF` baked from `flake.lock` in
  [`tool_provider.rs`](../../../crates/agent-runtime/src/tool_provider.rs)). The job
  reads files from this tree; it never runs peer code.
- Because every input is locked, the derivation's output hash is a **stable fingerprint
  of the current peer-prompt corpus** — the refresh job compares it against the last
  imported fingerprint to decide whether anything changed ([`11`](11-refresh-job.md)).
- A `nix run .#prompts-refresh` app wires the derivation + the extractor + the store.

## `flake.nix` / `nix/` change surface (Phase 4)

| File | Change |
|---|---|
| `flake.nix` | add `hermes-agent-src` (`flake=false`, commit-pinned) to `inputs` + `outputs` args; opencode/codex/pi `.src` come from the existing `nixpkgs` pin |
| `nix/prompt-sources.nix` (new) | the gather derivation above |
| `nix/packages.nix` / `nix/apps` | a `prompts-refresh` app (the [`11`](11-refresh-job.md) job) reading `prompt-sources` |
| `nix/versions.nix` | record the hermes pin + any extractor deps |

Path/version drift between the pinned nixpkgs and a peer's `HEAD` is expected and
**handled by the extractor**, not here: a moved/renamed file is a fail-soft skip with a
recorded reason ([`11`](11-refresh-job.md), `negative_absent_or_moved_file`).

## Licensing

Each peer's prompt carries its upstream license (recorded in `source_ref`,
[`08`](08-versioning-and-provenance.md)). The sourcing step captures the upstream
`LICENSE` alongside the prompt so attribution travels with the text. A personality whose
license does not permit redistribution is imported as a **reference** (source path +
version, empty verbatim content) rather than a copied prompt — the store row still lets a
user *point at* it, without agent-seddon redistributing text it may not. `cargo-deny`'s
license gate covers our own dependency graph; the peer-prompt license check is a step in
the refresh job, not a crate-license concern.

## No wire change

Phase 4 is nix + a job; it introduces no proto/wire change. The store fields it writes
(`source_ref`, `version`) are the additive ones from [`08`](08-versioning-and-provenance.md).
Tests + `adversarial_` cases (a symlinked/oversized/renamed source tree) are in
[`status/round3-04-nixpkgs-sourcing.md`](status/round3-04-nixpkgs-sourcing.md).
