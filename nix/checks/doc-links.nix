# nix/checks/doc-links.nix
#
# The first-party documentation link GATE: runs the `doc-links` binary (`--gate`) over the
# real repo tree and fails the build on any relative link whose target is missing in-repo —
# the rot a rename leaves behind (`server.rs` → `server/mod.rs`, `sqlite.rs` → `store.rs`),
# see docs/gap-analysis/README.md §9.2. The constants-sync / buf duality: the SAME binary
# (crates/agent-doc-links) backs both `nix run .#doc-links` (report) and this gate, so report
# and gate can never disagree.
#
# Read-only: the checker only tests whether link TARGETS exist, so it points `--repo-root`
# straight at the source store path (no `cp`). The source is the whole flake tree (`../..`),
# git-tracked only — so untracked strays never enter, and the `docs/parity/` peer-clone
# citations (`../../../codex`, `../../../pi`) escape the root and are classified `external`,
# never failing the hermetic sandbox.
#
# The crane source filter drops `.md` files (they are not cargo sources), so this check must
# use the RAW flake tree (`../..`), not `commonArgs.src` — the latter would have no docs to
# scan. The checker's OWN correctness (extractor + classifier + check-the-checks) is gated
# by agent-doc-links' rstest tables, run in the default `test` check.
{
  pkgs,
  doc-links-bin,
}:
pkgs.runCommand "doc-links-gate"
  {
    src = ../..;
  }
  ''
    echo "doc-links: resolving every first-party relative link against the tree ..."
    ${doc-links-bin}/bin/doc-links --gate --repo-root "$src"
    touch "$out"
  ''
