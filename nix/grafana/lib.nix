# nix/grafana/lib.nix
#
# Small builders for Grafana dashboard JSON, so the dashboards in ./dashboards.nix read as
# a list of panels and queries instead of hand-placed JSON. A dashboard is
#
#   { uid; title; tags; vars; rows = [ { title; panels = [ (ts { … }) … ]; } … ]; }
#
# and `dashboard` lays it out: each row gets a header, its panels flow left to right in
# a 24-column grid (a panel's `w`, default 12) and wrap, and panel ids are numbered in
# order. Panels pick their data source through dashboard variables (`$prometheus`,
# `$clickhouse`), so the dashboards use whatever Prometheus and ClickHouse data sources
# the Grafana already has instead of a fixed uid.
{ lib }:

let
  prom = {
    type = "prometheus";
    uid = "\${prometheus}";
  };
  ch = {
    type = "grafana-clickhouse-datasource";
    uid = "\${clickhouse}";
  };

  # The label filter every agent query carries: the scrape job and the tenant. Agent
  # metrics name the tenant `user` (fleet, runs, tokens) or `tenant` (gRPC, policy, config
  # store); a series without the label still matches `=~".*"`.
  sel = ''job=~"$job"'';
  selUser = ''job=~"$job", user=~"$tenant"'';
  selTenant = ''job=~"$job", tenant=~"$tenant"'';

  q = expr: legendFormat: { inherit expr legendFormat; };

  # `sum by (<by>) (rate(<m>{<s>}[$__rate_interval]))`; `by` may be "".
  rate =
    {
      m,
      by ? "",
      s ? sel,
      legend ? null,
    }:
    q "sum${lib.optionalString (by != "") " by (${by})"} (rate(${m}{${s}}[$__rate_interval]))" (
      if legend != null then legend else lib.concatMapStringsSep " " (l: "{{${l}}}") (splitBy by)
    );

  # The `phi` quantile of histogram `m`, per `by`.
  quantile =
    {
      m,
      phi ? 0.95,
      by ? "",
      s ? sel,
      legend ? null,
    }:
    let
      p = "p${toString (builtins.floor (phi * 100))}";
    in
    q
      "histogram_quantile(${toString phi}, sum by (${
        lib.concatStringsSep ", " ([ "le" ] ++ splitBy by)
      }) (rate(${m}_bucket{${s}}[$__rate_interval])))"
      (
        if legend != null then
          legend
        else if by == "" then
          p
        else
          "${lib.concatMapStringsSep " " (l: "{{${l}}}") (splitBy by)} ${p}"
      );

  splitBy = by: lib.filter (s: s != "") (map lib.trim (lib.splitString "," by));

  fieldConfig =
    {
      unit,
      min ? null,
      max ? null,
      stack ? false,
      thresholds ? null,
    }:
    {
      defaults = {
        inherit unit;
        custom = lib.optionalAttrs stack {
          stacking.mode = "normal";
          fillOpacity = 30;
        };
      }
      // lib.optionalAttrs (min != null) { inherit min; }
      // lib.optionalAttrs (max != null) { inherit max; }
      // {
        # Without explicit steps Grafana's default turns a stat red above 80; a panel
        # that names no thresholds stays neutral instead.
        thresholds = {
          mode = "absolute";
          steps =
            if thresholds != null then
              thresholds
            else
              [
                {
                  color = "green";
                  value = null;
                }
              ];
        };
      };
      overrides = [ ];
    };

  panel =
    type:
    {
      title,
      description ? null,
      w ? 12,
      h ? 8,
      ds ? prom,
      targets,
      unit ? "short",
      min ? null,
      max ? null,
      stack ? false,
      thresholds ? null,
      options ? { },
    }:
    {
      inherit
        type
        title
        w
        h
        options
        ;
      datasource = ds;
      targets = lib.imap0 (
        i: t:
        {
          refId = builtins.elemAt (lib.stringToCharacters "ABCDEFGHIJKLMNOPQRSTUVWXYZ") i;
          datasource = ds;
        }
        // t
      ) targets;
      fieldConfig = fieldConfig {
        inherit
          unit
          min
          max
          stack
          thresholds
          ;
      };
    }
    // lib.optionalAttrs (description != null) { inherit description; };

  # A ClickHouse SQL target. `format`: 0 time series (a `time` column, string columns
  # become series), 1 table.
  sql = format: rawSql: {
    inherit rawSql format;
    queryType = "sql";
    editorType = "sql";
  };

  dsVar = name: label: type: {
    inherit name label;
    type = "datasource";
    query = type;
    hide = 0;
    current = { };
  };

  queryVar = name: label: query: {
    inherit name label query;
    type = "query";
    datasource = prom;
    includeAll = true;
    multi = true;
    allValue = ".*";
    current = {
      text = "All";
      value = "$__all";
    };
    refresh = 2;
    sort = 1;
  };

  # Lay the rows out on the grid and number the panels.
  layout =
    rows:
    let
      step =
        acc: row:
        let
          header = {
            type = "row";
            inherit (row) title;
            collapsed = false;
            panels = [ ];
            gridPos = {
              h = 1;
              w = 24;
              x = 0;
              inherit (acc) y;
            };
          };
          place =
            st: p:
            let
              wrap = st.x + p.w > 24;
              x = if wrap then 0 else st.x;
              y = if wrap then st.y + st.rowH else st.y;
              rowH = if wrap then p.h else lib.max st.rowH p.h;
            in
            {
              x = x + p.w;
              inherit y rowH;
              out = st.out ++ [
                (
                  removeAttrs p [
                    "w"
                    "h"
                  ]
                  // {
                    gridPos = {
                      inherit (p) w h;
                      inherit x y;
                    };
                  }
                )
              ];
            };
          placed = builtins.foldl' place {
            x = 0;
            y = acc.y + 1;
            rowH = 0;
            out = [ ];
          } row.panels;
        in
        {
          y = placed.y + placed.rowH;
          out = acc.out ++ [ header ] ++ placed.out;
        };
      all =
        (builtins.foldl' step {
          y = 0;
          out = [ ];
        } rows).out;
    in
    lib.imap1 (i: p: p // { id = i; }) all;
in
rec {
  inherit
    prom
    ch
    sel
    selUser
    selTenant
    q
    rate
    quantile
    sql
    ;

  ts = panel "timeseries";
  stat = panel "stat";
  table = panel "table";
  bar = panel "bargauge";

  # ClickHouse panels: `ts`/`table` over one SQL query.
  chTs =
    args:
    ts (
      removeAttrs args [ "sql" ]
      // {
        ds = ch;
        targets = [ (sql 0 args.sql) ];
      }
    );
  chTable =
    args:
    table (
      removeAttrs args [ "sql" ]
      // {
        ds = ch;
        targets = [ (sql 1 args.sql) ];
        h = args.h or 9;
      }
    );

  vars = {
    prometheus = dsVar "prometheus" "Prometheus" "prometheus";
    clickhouse = dsVar "clickhouse" "ClickHouse" "grafana-clickhouse-datasource";
    job = queryVar "job" "Job" ''label_values({__name__=~"agent_.+"}, job)'';
    tenant = queryVar "tenant" "Tenant" "label_values(agent_fleet_reviews_total, user)";
  };

  # The dashboards that link to each other (the "agent-seddon" tag) share a menu.
  links = [
    {
      title = "agent-seddon";
      type = "dashboards";
      tags = [ "agent-seddon" ];
      asDropdown = true;
      includeVars = true;
      keepTime = true;
    }
  ];

  dashboard =
    {
      uid,
      title,
      description ? "",
      tags ? [ ],
      vars ? [ ],
      rows,
      time ? "now-24h",
    }:
    {
      inherit uid title description;
      tags = [ "agent-seddon" ] ++ tags;
      editable = true;
      graphTooltip = 1;
      schemaVersion = 39;
      refresh = "1m";
      time = {
        from = time;
        to = "now";
      };
      annotations.list = [ ];
      inherit links;
      templating.list = vars;
      panels = layout rows;
    };
}
