# nix/checks/doc-orphans.nix
#
# The docs-discoverability GATE (the §9.1 companion to the §9.2 `doc-links` broken-link gate):
# runs the `doc-links` binary in `--orphans` mode over the real repo tree and fails the build on
# any first-party doc that is not reachable from `README.md` by following in-repo Markdown links —
# a design doc that no index ever links rots out of reach. See docs/gap-analysis/README.md §9.1.
#
# The constants-sync / buf duality: the SAME binary (crates/agent-doc-links) backs both
# `nix run .#doc-orphans` (report) and this gate, so report and gate can never disagree. Docs that
# are DELIBERATELY standalone are exempted in the committed allowlist (test/doc-links/orphans.allow),
# which the binary reads from the tree; a stale exemption (now reachable, or file gone) also fails,
# so the allowlist cannot drift.
#
# Read-only: the checker only reads the docs and probes whether link targets exist, so it points
# `--repo-root` straight at the source store path (no `cp`). The source is the whole flake tree
# (`../..`), git-tracked only — so untracked strays never count as orphans, and `docs/parity/`
# peer-clone citations escape the root and are terminal (never followed, never an orphan).
#
# The crane source filter drops `.md` files (they are not cargo sources), so this check must use
# the RAW flake tree (`../..`), not `commonArgs.src`. The checker's OWN correctness (BFS reachability
# + allowlist + check-the-checks) is gated by agent-doc-links' rstest tables, run in the default
# `test` check.
{
  pkgs,
  doc-links-bin,
}:
pkgs.runCommand "doc-orphans-gate"
  {
    src = ../..;
  }
  ''
    echo "doc-links: checking every first-party doc is reachable from README.md ..."
    ${doc-links-bin}/bin/doc-links --orphans --gate --repo-root "$src"
    touch "$out"
  ''
