# nix/checks/hyperdx-dashboards.nix
#
# The agent-seddon HyperDX dashboards and saved searches (nix/hyperdx/dashboards.nix),
# checked offline by `hyperdx-provision --check` (crates/agent-hyperdx): every source is
# a known placeholder, names are unique and tagged, and every dashboard's tiles fit the
# 24-column grid without overlapping. HyperDX's own schema is not available offline; the
# API validates each write when the module provisions (and the tiles were checked
# against the image's schema when they were written, see docs/deployment-nixos.md).
#
# Check-the-check: five broken templates must each be refused.
{ pkgs, agentSeddonDeploy }:

let
  provision = agentSeddonDeploy.hyperdxProvision;
  template = agentSeddonDeploy.hyperdx.dashboardsTemplate;
  tag = [ "agent-seddon" ];
  tile = id: x: {
    inherit id x;
    y = 0;
    w = 12;
    h = 8;
    config.source = "$trace";
  };
  bad = name: v: pkgs.writeText "hyperdx-bad-${name}.json" (builtins.toJSON v);
  fixtures = {
    overlap = bad "overlap" {
      dashboards = [
        {
          name = "D";
          tags = tag;
          tiles = [
            (tile "a" 0)
            (tile "b" 6)
          ];
        }
      ];
    };
    unknownSource = bad "unknown-source" {
      dashboards = [
        {
          name = "D";
          tags = tag;
          tiles = [ ((tile "a" 0) // { config.source = "$metric"; }) ];
        }
      ];
    };
    duplicateName = bad "duplicate-name" {
      savedSearches = [
        {
          name = "S";
          tags = tag;
          source = "$trace";
          where = "";
        }
        {
          name = "S";
          tags = tag;
          source = "$trace";
          where = "";
        }
      ];
    };
    badPlaceholder = bad "bad-placeholder" {
      dashboards = [
        {
          name = "D";
          tags = tag;
          tiles = [ ((tile "a" 0) // { config.source = "$trace\n"; }) ];
        }
      ];
    };
    untagged = bad "untagged" {
      savedSearches = [
        {
          name = "S";
          source = "$trace";
          where = "";
        }
      ];
    };
  };
in
pkgs.runCommand "hyperdx-dashboards" { } ''
  ${provision} --check --template ${template}
  ${pkgs.lib.concatStrings (
    pkgs.lib.mapAttrsToList (n: f: ''
      if ${provision} --check --template ${f} 2>/dev/null; then
        echo "hyperdx-dashboards: the ${n} fixture was accepted" >&2
        exit 1
      fi
    '') fixtures
  )}
  touch $out
''
