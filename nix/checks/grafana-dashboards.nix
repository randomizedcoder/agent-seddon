# nix/checks/grafana-dashboards.nix
#
# Gate for the generated Grafana dashboards (nix/grafana/dashboards.nix):
#
#   - structure: dashboard uids are unique and none matches a hand-written
#     nix/grafana/dashboards/<uid>.json (dashboardsDir would overwrite it), panel ids are
#     unique within a dashboard, every panel sits inside the 24-column grid and has at
#     least one query;
#   - metrics: every `agent_*` metric a Prometheus query names (less a histogram's
#     `_bucket`/`_sum`/`_count`) is a string literal somewhere in crates/*/src, so a
#     renamed or removed metric fails here instead of leaving an empty panel;
#   - tables: every `agent.<table>` a ClickHouse query reads is created in
#     nix/clickhouse/schema.sql.
#
# Check-the-checks: the same validators run on a broken fixture (a duplicate uid, a uid
# that collides with a hand-written file, a duplicate panel id, an off-grid panel, an unknown metric, an unknown table) and
# must report every one of them.
{
  pkgs,
  lib,
  src,
}:

let
  dashboards = lib.attrValues (import ../grafana/dashboards.nix { inherit lib; });

  # The uids nix/grafana/default.nix copies in from hand-written JSON files.
  handWritten = map (lib.removeSuffix ".json") (
    lib.filter (lib.hasSuffix ".json") (lib.attrNames (builtins.readDir ../grafana/dashboards))
  );

  # --- structure (pure eval) ------------------------------------------------------
  dupes = xs: lib.unique (lib.filter (x: lib.count (y: y == x) xs > 1) xs);

  structural =
    hand: ds:
    let
      uidDupes = dupes (map (d: d.uid) ds);
      clashes = lib.filter (u: lib.elem u hand) (lib.unique (map (d: d.uid) ds));
      perDash =
        d:
        let
          ids = map (p: p.id) d.panels;
          bad = lib.filter (
            p:
            p.gridPos.x < 0
            || p.gridPos.x + p.gridPos.w > 24
            || p.gridPos.w < 1
            || (p.type != "row" && (p.targets or [ ]) == [ ])
          ) d.panels;
        in
        map (i: "${d.uid}: duplicate panel id ${toString i}") (dupes ids)
        ++ map (p: "${d.uid}: panel '${p.title}' is off the grid or has no query") bad;
    in
    map (u: "duplicate dashboard uid ${u}") uidDupes
    ++ map (u: "uid ${u} collides with hand-written dashboards/${u}.json") clashes
    ++ lib.concatMap perDash ds;

  # --- names the queries use (pure eval; checked against the tree at build) -------
  queries =
    d:
    lib.concatMap (
      p:
      map (t: {
        prom = t.expr or "";
        sql = t.rawSql or "";
      }) (p.targets or [ ])
    ) d.panels;

  matches =
    re: s: lib.concatMap (m: if lib.isList m then [ (lib.head m) ] else [ ]) (builtins.split re s);

  promText = ds: lib.concatMapStringsSep "\n" (q: q.prom) (lib.concatMap queries ds);
  sqlText = ds: lib.concatMapStringsSep "\n" (q: q.sql) (lib.concatMap queries ds);

  baseName =
    m:
    let
      r = builtins.match "(.*)_(bucket|sum|count)" m;
    in
    if r == null then m else lib.head r;

  metricsOf =
    ds: lib.sort lib.lessThan (lib.unique (map baseName (matches "(agent_[a-z0-9_]+)" (promText ds))));
  tablesOf = ds: lib.sort lib.lessThan (lib.unique (matches "agent\\.([a-z0-9_]+)" (sqlText ds)));

  # --- the broken fixture -----------------------------------------------------------
  broken =
    let
      d = lib.head dashboards;
      p = lib.elemAt d.panels 1;
    in
    [
      d
      (
        d
        // {
          panels = d.panels ++ [
            (
              p
              // {
                gridPos = p.gridPos // {
                  x = 20;
                  w = 12;
                };
                targets = [
                  {
                    expr = "rate(agent_no_such_metric_total[5m])";
                  }
                  {
                    rawSql = "SELECT 1 FROM agent.no_such_table";
                  }
                ];
              }
            )
          ];
        }
      )
    ];
  brokenStructural = structural [ (lib.head dashboards).uid ] broken;
  structuralCaught =
    lib.length brokenStructural >= 3
    && lib.any (lib.hasPrefix "duplicate dashboard uid") brokenStructural
    && lib.any (lib.hasInfix "collides with hand-written") brokenStructural
    && lib.any (lib.hasInfix "duplicate panel id") brokenStructural
    && lib.any (lib.hasInfix "off the grid") brokenStructural;

  realStructural = structural handWritten dashboards;

  lines = xs: pkgs.writeText "names" (lib.concatMapStrings (x: "${x}\n") xs);
in
assert lib.assertMsg structuralCaught
  "grafana-dashboards: the structural validator missed a fixture defect: ${toString brokenStructural}";
assert lib.assertMsg (
  realStructural == [ ]
) "grafana-dashboards: ${lib.concatStringsSep "; " realStructural}";
pkgs.runCommand "grafana-dashboards"
  {
    nativeBuildInputs = [
      pkgs.gnugrep
      pkgs.coreutils
    ];
  }
  ''
    set -euo pipefail
    export LC_ALL=C # comm needs the byte order Nix's sort produced
    grep -rhoE --include='*.rs' '"agent_[a-z0-9_]+"' ${src}/crates | tr -d '"' | sort -u > literals
    grep -oE 'CREATE TABLE IF NOT EXISTS agent\.[a-z0-9_]+' ${src}/nix/clickhouse/schema.sql \
      | sed 's/.*agent\.//' | sort -u > tables

    # Names in $1 that are missing from $2.
    missing() { comm -23 "$1" "$2"; }

    # Check-the-checks: the fixture's unknown metric and table must be reported.
    missing ${lines (metricsOf broken)} literals | grep -qx agent_no_such_metric_total
    missing ${lines (tablesOf broken)} tables | grep -qx no_such_table

    bad_m="$(missing ${lines (metricsOf dashboards)} literals)"
    bad_t="$(missing ${lines (tablesOf dashboards)} tables)"
    if [ -n "$bad_m" ] || [ -n "$bad_t" ]; then
      [ -z "$bad_m" ] || printf 'dashboards query metrics no crate registers:\n%s\n' "$bad_m" >&2
      [ -z "$bad_t" ] || printf 'dashboards read tables schema.sql does not create:\n%s\n' "$bad_t" >&2
      exit 1
    fi
    echo "grafana dashboards: ${toString (lib.length dashboards)} dashboards, ${toString (lib.length (metricsOf dashboards))} metrics, ${toString (lib.length (tablesOf dashboards))} tables, all known" > $out
  ''
