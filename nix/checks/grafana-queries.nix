# nix/checks/grafana-queries.nix
#
# The provisioned Grafana dashboards (nix/grafana), checked offline by
# `obs-check --check` (crates/agent-obs-check): every panel query's variables fill in
# and every query names a data source the module provisions (`prometheus` as the
# default, `clickhouse`). Whether the queries RUN is the VM test's
# (`agent-seddon-obs-check` against a live Grafana, nix/nixos/agent-stack-vm-test.nix).
#
# Check-the-check: each broken dashboard below must be refused, and a valid one built
# the same way accepted.
{ pkgs, agentSeddonDeploy }:

let
  obsCheck = agentSeddonDeploy.obsCheck;
  dashboards = agentSeddonDeploy.grafanaDashboards;
  panel = target: {
    title = "p";
    targets = [ target ];
  };
  bad =
    name: v:
    pkgs.writeTextDir "${name}/${name}.json" (builtins.toJSON ({ uid = name; } // v)) + "/${name}";
  fixtures = {
    unknownSource = bad "unknown-source" {
      panels = [ (panel { datasource.uid = "loki"; }) ];
    };
    undefinedSourceVariable = bad "undefined-source-variable" {
      panels = [ (panel { datasource.uid = "\${ghost}"; }) ];
    };
    sourceVariableOfMissingType = bad "missing-type" {
      templating.list = [
        {
          name = "logs";
          type = "datasource";
          query = "loki";
        }
      ];
    };
    targetNotAnObject = bad "target-not-an-object" {
      panels = [ (panel "up") ];
    };
  };
  # Built the same way and valid: proves a refusal above is the dashboard's, not the path's.
  good = bad "good" {
    panels = [ (panel { expr = "up"; }) ];
  };
in
pkgs.runCommand "grafana-queries" { } ''
  ${obsCheck} --check --dashboards ${dashboards}
  ${obsCheck} --check --dashboards ${good}
  ${pkgs.lib.concatStrings (
    pkgs.lib.mapAttrsToList (n: d: ''
      if ${obsCheck} --check --dashboards ${d} 2>/dev/null; then
        echo "grafana-queries: the ${n} fixture was accepted" >&2
        exit 1
      fi
    '') fixtures
  )}
  touch $out
''
