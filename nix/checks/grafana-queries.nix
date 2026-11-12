# nix/checks/grafana-queries.nix
#
# The provisioned Grafana dashboards (nix/grafana) and HyperDX tiles (nix/hyperdx),
# checked offline by `obs-check --check` (crates/agent-obs-check): every panel query's
# variables fill in and every query names a data source the module provisions
# (`prometheus` as the default, `clickhouse`), and every tile and saved search renders
# to SQL. Whether they RUN is the VM test's (`agent-seddon-obs-check` against a live
# Grafana and ClickHouse, nix/nixos/agent-stack-vm-test.nix).
#
# Check-the-check: each broken dashboard or template below must be refused (an empty
# directory too: a run that checks nothing is not a pass), and a valid one built the
# same way accepted.
{ pkgs, agentSeddonDeploy }:

let
  obsCheck = agentSeddonDeploy.obsCheck;
  dashboards = agentSeddonDeploy.grafanaDashboards;
  template = agentSeddonDeploy.hyperdx.dashboardsTemplate;
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
    noQueries = bad "no-queries" {
      panels = [ { title = "text"; } ];
    };
    emptyDirectory = pkgs.runCommand "empty-dashboards" { } "mkdir $out";
  };
  tile = cfg: {
    dashboards = [
      {
        name = "D";
        tiles = [
          {
            id = "d-0";
            config = {
              name = "t";
              source = "$trace";
              displayType = "table";
              whereLanguage = "sql";
              select = [ { aggFn = "count"; } ];
            }
            // cfg;
          }
        ];
      }
    ];
  };
  badTemplate = name: v: pkgs.writeText "${name}.json" (builtins.toJSON v);
  templateFixtures = {
    secondStatement = badTemplate "second-statement" (tile {
      where = "1; DROP TABLE agent.agent_events";
    });
    logSource = badTemplate "log-source" (tile {
      source = "$log";
    });
    unknownAggregate = badTemplate "unknown-aggregate" (tile {
      select = [ { aggFn = "last_value"; } ];
    });
    empty = badTemplate "empty" { dashboards = [ ]; };
  };
  goodTemplate = badTemplate "good" (tile { });
  # Built the same way and valid: proves a refusal above is the dashboard's, not the path's.
  good = bad "good" {
    panels = [ (panel { expr = "up"; }) ];
  };
in
pkgs.runCommand "grafana-queries" { } ''
  ${obsCheck} --check --dashboards ${dashboards} --hyperdx-template ${template}
  ${obsCheck} --check --dashboards ${good}
  ${obsCheck} --check --hyperdx-template ${goodTemplate}
  ${pkgs.lib.concatStrings (
    pkgs.lib.mapAttrsToList (n: d: ''
      if ${obsCheck} --check --dashboards ${d} 2>/dev/null; then
        echo "grafana-queries: the ${n} fixture was accepted" >&2
        exit 1
      fi
    '') fixtures
  )}
  ${pkgs.lib.concatStrings (
    pkgs.lib.mapAttrsToList (n: t: ''
      if ${obsCheck} --check --hyperdx-template ${t} 2>/dev/null; then
        echo "grafana-queries: the ${n} template was accepted" >&2
        exit 1
      fi
    '') templateFixtures
  )}
  touch $out
''
