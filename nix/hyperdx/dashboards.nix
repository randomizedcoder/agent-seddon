# nix/hyperdx/dashboards.nix
#
# The agent-seddon HyperDX dashboards and saved searches, as data. Each tile reads the
# team's trace source; a source is named by kind (`"$trace"`, `"$log"`) because HyperDX
# gives it a random id when it seeds the team, and `hyperdx-provision`
# (crates/agent-hyperdx) swaps the placeholder for the real id before it writes anything
# through HyperDX's API (which validates every write against its own schema).
#
# A tile is `{ name; display; select; where; groupBy; w; h; … }` and `dashboard` lays
# the tiles out left to right in HyperDX's 24-column grid (a tile's `w`, default 12)
# and wraps. Durations are the trace table's `Duration` (nanoseconds), shown in ms.
{ lib }:

let
  trace = "$trace";

  # The ids are stable per dashboard so a re-provision updates tiles in place.
  layout =
    slug: tiles:
    let
      step =
        acc: t:
        let
          w = t.w or 12;
          h = t.h or 8;
          wrap = acc.x + w > 24;
          x = if wrap then 0 else acc.x;
          y = if wrap then acc.y + acc.rowH else acc.y;
          rowH = if wrap then h else lib.max acc.rowH h;
        in
        {
          x = x + w;
          inherit y rowH;
          out = acc.out ++ [
            {
              id = "${slug}-${toString (lib.length acc.out)}";
              inherit
                x
                y
                w
                h
                ;
              config = removeAttrs t [
                "w"
                "h"
              ];
            }
          ];
        };
    in
    (lib.foldl step {
      x = 0;
      y = 0;
      rowH = 0;
      out = [ ];
    } tiles).out;

  dashboard =
    {
      name,
      slug,
      tiles,
    }:
    {
      inherit name;
      tags = [ "agent-seddon" ];
      tiles = layout slug tiles;
    };

  # A builder tile over the trace source; `where` is SQL over otel_traces.
  tile =
    {
      name,
      select,
      display ? "line",
      where ? "",
      groupBy ? null,
      w ? 12,
      h ? 8,
      numberFormat ? null,
    }:
    {
      inherit
        name
        select
        where
        w
        h
        ;
      source = trace;
      displayType = display;
      whereLanguage = "sql";
      granularity = "auto";
    }
    // lib.optionalAttrs (groupBy != null) { inherit groupBy; }
    // lib.optionalAttrs (numberFormat != null) { inherit numberFormat; };

  count = alias: {
    aggFn = "count";
    valueExpression = "";
    aggCondition = "";
    inherit alias;
  };
  # `phi` quantile of a span's duration, in ms.
  durMs = phi: {
    aggFn = "quantile";
    level = phi;
    valueExpression = "Duration / 1e6";
    aggCondition = "";
    alias = "p${toString (builtins.floor (phi * 100))} ms";
  };
  sumOf = expr: alias: {
    aggFn = "sum";
    valueExpression = expr;
    aggCondition = "";
    inherit alias;
  };
  attr = k: "SpanAttributes['${k}']";
  attrNum = k: "toFloat64OrZero(${attr k})";
  # Only for tiles whose every series is a duration: HyperDX applies a tile's format to
  # all its columns, so a table with a count beside a duration leaves it off and lets the
  # `p95 ms` alias carry the unit.
  ms = {
    output = "number";
    mantissa = 0;
    unit = "ms";
  };

  isErr = "StatusCode = 'Error'";
  isChat = "SpanName = 'provider.stream' OR SpanName = 'provider.complete'";
  isTool = "SpanName = 'tool.execute'";
  # Every fleet poll opens a `fleet.review` span; `outcome` marks the ones that reviewed
  # (`drafted`, `failed`) apart from those answered before any work (`uptodate`, `at_capacity`).
  isReview = "SpanName = 'fleet.review' AND SpanAttributes['outcome'] IN ('drafted', 'failed')";
  ours = "ServiceName LIKE 'agent-seddon-%'";
in
{
  dashboards = {
    overview = dashboard {
      name = "agent-seddon · Overview";
      slug = "overview";
      tiles = [
        (tile {
          name = "Spans by service";
          display = "stacked_bar";
          select = [ (count "spans") ];
          groupBy = "ServiceName";
        })
        (tile {
          name = "Error spans by span name";
          display = "stacked_bar";
          select = [ (count "errors") ];
          where = isErr;
          groupBy = "SpanName";
        })
        (tile {
          name = "Slowest operations (p95)";
          display = "table";
          select = [
            (durMs 0.95)
            (count "spans")
          ];
          where = ours;
          groupBy = "ServiceName, SpanName";
        })
        (tile {
          name = "gRPC calls by method";
          display = "table";
          select = [
            (count "calls")
            (durMs 0.95)
          ];
          where = "SpanName = 'grpc.server'";
          groupBy = attr "rpc";
        })
      ];
    };

    models = dashboard {
      name = "agent-seddon · Model calls";
      slug = "models";
      tiles = [
        (tile {
          name = "Model calls by model";
          display = "stacked_bar";
          select = [ (count "calls") ];
          where = isChat;
          groupBy = attr "gen_ai.request.model";
        })
        (tile {
          name = "Model call latency";
          select = [
            (durMs 0.5)
            (durMs 0.95)
          ];
          where = isChat;
          numberFormat = ms;
        })
        (tile {
          name = "Tokens";
          display = "stacked_bar";
          select = [
            (sumOf (attrNum "gen_ai.usage.input_tokens") "input")
            (sumOf (attrNum "gen_ai.usage.output_tokens") "output")
            (sumOf (attrNum "gen_ai.usage.cache_read_input_tokens") "cache read")
          ];
          where = isChat;
        })
        (tile {
          name = "Finish reasons";
          display = "table";
          select = [ (count "calls") ];
          where = isChat;
          groupBy = attr "gen_ai.response.finish_reasons";
        })
      ];
    };

    tools = dashboard {
      name = "agent-seddon · Tools";
      slug = "tools";
      tiles = [
        (tile {
          name = "Tool calls by tool";
          display = "stacked_bar";
          select = [ (count "calls") ];
          where = isTool;
          groupBy = attr "gen_ai.tool.name";
        })
        (tile {
          name = "Tool latency by tool (p95)";
          display = "table";
          select = [
            (durMs 0.95)
            (count "calls")
          ];
          where = isTool;
          groupBy = attr "gen_ai.tool.name";
        })
        (tile {
          name = "Tool errors";
          display = "stacked_bar";
          select = [ (count "errors") ];
          where = "${isTool} AND ${isErr}";
          groupBy = attr "gen_ai.tool.name";
        })
        (tile {
          name = "Policy decisions";
          display = "stacked_bar";
          select = [ (count "decisions") ];
          where = "SpanName = 'policy.authorize'";
          groupBy = attr "decision";
        })
      ];
    };

    fleet = dashboard {
      name = "agent-seddon · Review fleet";
      slug = "fleet";
      tiles = [
        (tile {
          name = "Reviews by repo";
          display = "stacked_bar";
          select = [ (count "reviews") ];
          where = isReview;
          groupBy = "${attr "repo"}, ${attr "outcome"}";
        })
        (tile {
          name = "Review duration";
          select = [
            (durMs 0.5)
            (durMs 0.95)
          ];
          where = isReview;
          numberFormat = ms;
        })
        (tile {
          name = "Collectors (p95)";
          display = "table";
          select = [
            (durMs 0.95)
            (count "runs")
          ];
          where = "SpanName = 'review.collector'";
          groupBy = attr "collector";
        })
        (tile {
          name = "Sandboxed commands (p95)";
          select = [ (durMs 0.95) ];
          where = "SpanName = 'sandbox.exec'";
          groupBy = "ServiceName";
          numberFormat = ms;
        })
      ];
    };
  };

  savedSearches = {
    errors = {
      name = "agent-seddon · Error spans";
      where = "${ours} AND ${isErr}";
    };
    models = {
      name = "agent-seddon · Model calls";
      where = isChat;
    };
    slowTools = {
      name = "agent-seddon · Tool calls over 10 s";
      where = "${isTool} AND Duration > 10000000000";
    };
    reviews = {
      name = "agent-seddon · Fleet reviews";
      where = isReview;
    };
  };

  # Saved searches share these: the trace source, SQL, newest first.
  savedSearchDefaults = {
    source = trace;
    select = "";
    whereLanguage = "sql";
    orderBy = "Timestamp DESC";
    tags = [ "agent-seddon" ];
  };
}
