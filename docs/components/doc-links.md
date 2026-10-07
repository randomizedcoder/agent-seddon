# doc-links — the first-party documentation link + discoverability gates

Two small gates over the docs we own, backed by one binary, that fail
`nix flake check` when the docs rot:

- **doc-links** (§9.2) — a **relative** link points at a path that no longer exists
  (the rot a rename leaves behind: `server.rs` → `server/mod.rs`,
  `sqlite.rs` → `store.rs`).
- **doc-orphans** (§9.1) — a first-party doc is not **reachable** from `README.md`
  by following in-repo Markdown links, so no reader walking out from the front door
  will ever find it.

Together they close the broken-link and discoverability halves of
[gap-analysis §9](../gap-analysis/README.md).

## What doc-links checks (§9.2)

It walks the Markdown we own — `docs/` plus the root `README.md` / `DESIGN.md` /
`CLAUDE.md` (`SCAN_ROOTS`) — and, for every relative link, resolves the target
against the linking file's directory and checks that it exists. Deliberate scope,
so the gate neither misses a real break nor fails on something it does not own:

- **Only relative links are checked.** `http(s)` / `mailto` / `tel` / `#anchor` /
  protocol-relative `//` targets are skipped — live-web reachability is not a build
  concern.
- **Links that escape the repo root are external, not broken.** The
  [`docs/parity/`](../parity) specs deliberately cite sibling peer-clone checkouts
  (`../../../codex`, `../../../pi`); those live outside the repo and outside the
  hermetic nix sandbox, so they are classified `external` and never fail.
- **Anchors are not resolved** — `file.md#section` is checked as `file.md` only.
- **Fenced code blocks are skipped**, so an illustrative link inside a fence is
  never mistaken for a live one.

## What doc-orphans checks (§9.1)

It walks the **same** `SCAN_ROOTS` universe, but asks reachability instead of
existence: starting at `README.md`, it follows every in-repo Markdown link
breadth-first and collects the docs it can reach. Anything tracked but unreached is
an **orphan** — present in the repo, invisible to a reader.

- **Only `.md` docs are followed.** A link to code, a directory, an `external`
  peer clone or an out-of-scope tree is terminal — it is never an orphan and is
  never walked through.
- **The walk is over the git-tracked tree**, so untracked strays never count as
  orphans (the flake sandbox only sees tracked files; §9.2's scoping applies here
  too).
- **Deliberate standalones are allowlisted.** A doc that genuinely should not be
  linked from the front door goes in the committed allowlist
  ([`test/doc-links/orphans.allow`](../../test/doc-links/orphans.allow)), one
  repo-relative path per line, `#` for comments/reasons — the
  `test/mt-audit/manifest.toml` governance shape.
- **The allowlist cannot drift.** An entry that is no longer an orphan (now
  reachable, or the file is gone) is reported as a **stale** entry and also fails
  the gate, so an exemption outlives its reason by exactly zero commits.

## Where it lives

The logic is a leaf Rust crate, `agent-doc-links`
([`src/lib.rs`](../../crates/agent-doc-links/src/lib.rs)), with the usual
four-class + `adversarial_` rstest tables (the link text a doc author writes is
untrusted, so traversal / overlong / weird targets are asserted to resolve to
`external` or be skipped, never to read out of tree or panic; the orphan walk and a
hostile allowlist are asserted not to crash or read out of tree either). Both halves
have check-the-checks cases — the pipeline must *reject* a broken / orphaned fixture,
not merely accept a clean one. Those tables run in the default `test` check — there
is no separate self-test check.

The [`doc-links`](../../crates/agent-doc-links/src/main.rs) binary is the single
entrypoint for both gates, the constants-sync / buf duality: the SAME binary backs
report and gate, so they can never disagree. Default mode checks links; `--orphans`
checks discoverability.

- **Report** — `nix run .#doc-links` prints every broken in-repo link;
  `nix run .#doc-orphans` prints every orphan and stale allowlist entry. Both exit 0.
- **Gates** — the `doc-links` check
  ([`nix/checks/doc-links.nix`](../../nix/checks/doc-links.nix)) and the `doc-orphans`
  check ([`nix/checks/doc-orphans.nix`](../../nix/checks/doc-orphans.nix)) run the
  binary with `--gate` against the whole flake tree and exit non-zero on any finding.
  Because the crane source filter drops `.md` files, the gates point `--repo-root` at
  the raw flake tree (git-tracked only), not the cargo-filtered source. All wiring is
  in [`nix/default.nix`](../../nix/default.nix).

## Fixing a failure

- **Broken link** — run `nix run .#doc-links` (or `cargo run -p agent-doc-links`) to
  list the breaks, then fix each link's target or remove the link. A citation that
  genuinely points outside the repo is fine — make it escape the repo root and it is
  treated as `external`.
- **Orphan** — run `nix run .#doc-orphans` to list them, then **link the doc** from
  an indexed page (its track `README.md`, or the `docs/README.md` index) — that is
  the real discoverability fix. Only if the doc is deliberately standalone, add it to
  [`test/doc-links/orphans.allow`](../../test/doc-links/orphans.allow) with a reason.
  A **stale** entry listed by the gate means an allowlisted doc became reachable or
  was deleted — remove its line.
