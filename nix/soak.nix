# nix/soak.nix
#
# `nix run .#soak` — long-running load soak. The load harnesses run a fixed batch
# then exit; this wraps each in a wall-clock loop that re-runs the batch until
# `SOAK_DURATION` seconds elapse, so each harness is exercised for ~1 hour by
# default (surfacing leaks, fd exhaustion, latency drift a short run misses).
#
# Model-free by design — it loops the CPU-only load tiers (loadtest, loadtest-loop,
# loadtest-wire), NOT the one-shot breadth probe (serve-smoke) or the model tiers.
# Each iteration returns the shared 0/1/2 contract; the worst across all iterations
# is the exit code.
#
# Env:
#   SOAK_DURATION   seconds per harness (default 3600 = 1h; set e.g. 60 for a smoke)
#   SOAK_HARNESSES  which to loop (default "loadtest loadtest-loop loadtest-wire")
#
# With the default 3 harnesses at 1h each this runs ~3h; scope it with SOAK_HARNESSES
# or shorten with SOAK_DURATION.
{
  pkgs,
  lib,
  harness,
  loadtest,
  loadtest-loop,
  loadtest-wire,
}:
pkgs.writeShellApplication {
  name = "soak";
  runtimeInputs = [
    pkgs.coreutils
    loadtest
    loadtest-loop
    loadtest-wire
  ];
  text = ''
    set -uo pipefail

    DURATION="''${SOAK_DURATION:-3600}"
    HARNESSES="''${SOAK_HARNESSES:-loadtest loadtest-loop loadtest-wire}"
  ''
  + harness.contract
  + ''

    # Loop one harness's batch until DURATION elapses; fold each iteration's 0/1/2
    # exit into `worst`.
    soak_one() {
      local label="$1"
      shift
      local start end iter=0
      start="$(date +%s)"
      end=$((start + DURATION))
      echo ""
      echo "######## soak: $label for ~''${DURATION}s ########"
      while [ "$(date +%s)" -lt "$end" ]; do
        iter=$((iter + 1))
        echo ""
        echo "--- soak[$label] iteration $iter (t=$(( $(date +%s) - start ))s / ''${DURATION}s) ---"
        "$@"
        note_fail "$?"
      done
      echo "soak[$label]: $iter iteration(s) over ~''${DURATION}s"
    }

    for h in $HARNESSES; do
      case "$h" in
        loadtest)
          soak_one "loadtest" loadtest --scenario overload --cap 4 --concurrency 64 --requests 5000 ;;
        loadtest-loop)
          soak_one "loadtest-loop" loadtest-loop --concurrency 16 --runs 512 ;;
        loadtest-wire)
          soak_one "loadtest-wire" loadtest-wire ;;
        *)
          echo "soak: unknown harness '$h' (choose from: loadtest loadtest-loop loadtest-wire)" >&2
          note_fail 1 ;;
      esac
    done

    echo ""
    contract_exit "SOAK complete: [$HARNESSES] each looped for ~''${DURATION}s."
  '';
}
