# Exit-code contract shared by the opt-in wire harnesses, matching e2e-live:
#   0 — ok.
#   1 — HARNESS failure (a server never came up, a tool errored). Our bug.
#   2 — CONTRACT violation (the thing the tier exists to catch).
# `worst` accumulates the max severity across sub-steps; `note_fail N` raises it;
# `contract_exit MSG` prints the verdict for `worst` and exits with it.
#
# This snippet is concatenated into a `writeShellApplication` after `set -uo
# pipefail`, so it is shellchecked in-context (no runtime `source`).

worst=0

note_fail() { [ "$1" -gt "$worst" ] && worst="$1" || true; }

contract_exit() {
  case "$worst" in
    0) printf '%s\n' "$1" ;;
    2) echo "FAIL(contract): see CONTRACT lines above." >&2 ;;
    *) echo "FAIL(harness): see FAIL lines above." >&2 ;;
  esac
  exit "$worst"
}
