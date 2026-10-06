# doc-links — the first-party documentation link gate

A small gate that fails `nix flake check` when a **relative** link in the
first-party docs points at a path that no longer exists — the rot a rename leaves
behind (`server.rs` → `server/mod.rs`, `sqlite.rs` → `store.rs`). It closes the
broken-intra-repo-link half of [gap-analysis §9.2](../gap-analysis/README.md).

## What it checks

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

## Where it lives

The logic is a leaf Rust crate, `agent-doc-links`
([`src/lib.rs`](../../crates/agent-doc-links/src/lib.rs)), with the usual
four-class + `adversarial_` rstest tables (the link text a doc author writes is
untrusted, so traversal / overlong / weird targets are asserted to resolve to
`external` or be skipped, never to read out of tree or panic). Those tables run in
the default `test` check — there is no separate self-test check.

The [`doc-links`](../../crates/agent-doc-links/src/main.rs) binary is the single
entrypoint, the constants-sync / buf duality: the SAME binary backs both the report
and the gate, so they can never disagree.

- **Report** — `nix run .#doc-links` prints every broken in-repo link and exits 0.
- **Gate** — the `doc-links` check ([`nix/checks/doc-links.nix`](../../nix/checks/doc-links.nix))
  runs the binary with `--gate` against the whole flake tree and exits non-zero on
  any finding. Because the crane source filter drops `.md` files, the gate points
  `--repo-root` at the raw flake tree (git-tracked only), not the cargo-filtered
  source. Both are wired in [`nix/default.nix`](../../nix/default.nix).

## Fixing a failure

Run `nix run .#doc-links` (or `cargo run -p agent-doc-links`) to list the breaks,
then fix each link's target or remove the link. A citation that genuinely points
outside the repo is fine — make it escape the repo root and it is treated as
`external`.
