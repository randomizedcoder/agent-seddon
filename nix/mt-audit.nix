# `nix run .#mt-audit` — the multi-tenancy coverage audit.
#
# Parses the source tree (it never runs it) and reconciles the gRPC service surface, the
# metric families, the span/log mechanisms and the config-ownership invariant against the
# checked-in expectation manifest (test/mt-audit/manifest.toml) — the same governance-by-
# committed-artifact shape as `constants-sync` and `buf breaking`. As the agent grows new
# services/metrics, each new element must be classified in the manifest or the audit flags
# it, so multi-tenancy coverage can't silently regress.
#
# Report mode (default) prints findings and always exits 0. `--gate` exits non-zero on any
# finding (that is the form the `mt-audit` nix check will run once the known gaps are
# fixed). `--dump-services` / `--dump-metrics` list the discovered surface (manifest-seeding
# aids). The logic lives in test/mt-audit/audit.py so it also runs straight from the dev
# shell (`python3 test/mt-audit/audit.py`).
#
# Run from the repo root (it reads crates/ + docs/); pass `--repo-root <path>` otherwise.
{
  pkgs,
}:
pkgs.writeShellApplication {
  name = "mt-audit";
  runtimeInputs = [ pkgs.python3 ];
  text = ''
    exec python3 "${../test/mt-audit}/audit.py" "$@"
  '';
}
