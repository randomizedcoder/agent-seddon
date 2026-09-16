# `nix run .#fleet-measure` — a deterministic review-fleet performance report.
#
# Read-only: it runs a fixed set of SELECTs against the agent's ClickHouse
# telemetry and prints a per-phase timing picture (model loop / grounding
# collectors / end-to-end / draft outcomes) for the reviews the fleet has
# produced. This turns "measure the performance" + "did the run actually work"
# into one repeatable command instead of ad-hoc `curl --data-binary` queries.
#
# Needs a reachable ClickHouse (the same one the fleet writes to) — its only
# runtime dependency, like `fleet-e2e` needs a model. Nothing is mutated and no
# model runs. The harness logic lives in test/fleet-measure/report.py so it also
# runs straight from the dev shell (`python3 test/fleet-measure/report.py`).
#
# Flags / env (all optional): --ch-url/CH_URL, --db/CH_DB, --since/CH_SINCE,
# --like/CH_LIKE, --iter-cap/CH_ITER_CAP, --json. See the script header.
{
  pkgs,
}:
pkgs.writeShellApplication {
  name = "fleet-measure";
  runtimeInputs = [ pkgs.python3 ];
  text = ''
    exec python3 "${../test/fleet-measure}/report.py" "$@"
  '';
}
