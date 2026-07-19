# nix/checks/constants-sync.nix
#
# Fails if the committed `crates/agent-grpc/src/constants.rs` differs from what
# `nix/constants.nix` renders — i.e. someone edited the Nix source of truth without
# regenerating. The fix is a one-liner: `nix run .#gen-constants`.
{
  pkgs,
  constantsRs,
  src,
}:
pkgs.runCommand "constants-sync-check" { } ''
  committed="${src}/crates/agent-grpc/src/constants.rs"
  if [ ! -f "$committed" ]; then
    echo "missing crates/agent-grpc/src/constants.rs — run: nix run .#gen-constants" >&2
    exit 1
  fi
  if ! diff -u "$committed" ${constantsRs}; then
    echo "" >&2
    echo "crates/agent-grpc/src/constants.rs is stale vs nix/constants.nix." >&2
    echo "Regenerate it: nix run .#gen-constants" >&2
    exit 1
  fi
  touch $out
''
