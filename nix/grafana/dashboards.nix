# nix/grafana/dashboards.nix
#
# The agent-seddon Grafana dashboards, as data (builders in ./lib.nix). Each is written
# to <uid>.json by ./default.nix and provisioned by `grafana-up` and by
# `services.agentSeddon.grafana` (nix/nixos/agent-stack.nix).
#
# Every `agent_*` metric named here must be one the agents register: the
# `grafana-dashboards` check (nix/checks/grafana-dashboards.nix) fails the build on a
# name that is not a string literal in crates/*/src, so a renamed metric shows up as a
# red check instead of an empty panel. ClickHouse panels read the agent tables
# (nix/clickhouse/schema.sql) and the OTLP `default.otel_traces`, as the read-only
# `agent_viewer`.
{ lib }:

let
  g = import ./lib.nix { inherit lib; };
  inherit (g)
    rate
    quantile
    q
    ts
    stat
    chTs
    chTable
    sel
    selUser
    selTenant
    ;

  promVars = [
    g.vars.prometheus
    g.vars.job
    g.vars.tenant
  ];
  allVars = promVars ++ [ g.vars.clickhouse ];

  # A p50 and a p95 line for one histogram.
  p50p95 = m: by: s: [
    (quantile {
      inherit m by s;
      phi = 0.5;
    })
    (quantile { inherit m by s; })
  ];

  # Errors as a share of all calls: `num` over `den`, both `sum(rate(…))`.
  ratio =
    num: den: legend:
    q "(${num}) / clamp_min(${den}, 1e-9)" legend;

  # --- Overview -------------------------------------------------------------------
  overview = g.dashboard {
    uid = "agent-seddon";
    title = "agent-seddon";
    description = "The whole stack at a glance; the other agent-seddon dashboards (menu above) go deeper.";
    vars = promVars;
    rows = [
      {
        title = "Now";
        panels = [
          (stat {
            title = "Active runs";
            w = 4;
            h = 5;
            targets = [ (q "sum(agent_active{${selUser}})" "active") ];
          })
          (stat {
            title = "Runs (window)";
            w = 4;
            h = 5;
            targets = [ (q "sum(increase(agent_runs_total{${selUser}}[$__range]))" "runs") ];
          })
          (stat {
            title = "Tokens (window)";
            w = 4;
            h = 5;
            targets = [ (q "sum(increase(agent_tokens_total{${selUser}}[$__range]))" "tokens") ];
          })
          (stat {
            title = "Reviews finished (window)";
            w = 4;
            h = 5;
            targets = [
              (q "sum(increase(agent_review_runs_total{${sel}}[$__range]))" "reviews")
            ];
          })
          (stat {
            title = "Tool error share";
            w = 4;
            h = 5;
            unit = "percentunit";
            thresholds = [
              {
                color = "green";
                value = null;
              }
              {
                color = "orange";
                value = 0.05;
              }
              {
                color = "red";
                value = 0.2;
              }
            ];
            targets = [
              (ratio "sum(rate(agent_tool_errors_total{${sel}}[$__range]))"
                "sum(rate(agent_tool_calls_total{${selUser}}[$__range]))"
                "errors"
              )
            ];
          })
          (stat {
            title = "Provider errors (window)";
            w = 4;
            h = 5;
            thresholds = [
              {
                color = "green";
                value = null;
              }
              {
                color = "red";
                value = 1;
              }
            ];
            targets = [
              (q "sum(increase(agent_provider_errors_total{${sel}}[$__range]))" "errors")
            ];
          })
        ];
      }
      {
        title = "Runs";
        panels = [
          (ts {
            title = "Runs/sec by outcome";
            targets = [
              (rate {
                m = "agent_runs_total";
                by = "outcome";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Run duration p50/p95";
            description = "The histogram's top bucket is 10 s, so a line flat at 10 s means 10 s or longer.";
            unit = "s";
            targets = p50p95 "agent_run_duration_seconds" "" selUser;
          })
          (ts {
            title = "Loop iterations/sec by job";
            targets = [
              (rate {
                m = "agent_iterations_total";
                by = "job";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Early loop stops/sec";
            description = "The loop's non-convergence guard stopped a run.";
            targets = [
              (rate {
                m = "agent_loop_early_stops_total";
                by = "job";
                s = selUser;
              })
            ];
          })
        ];
      }
      {
        title = "Model calls";
        panels = [
          (ts {
            title = "Model calls/sec by model, finish reason";
            targets = [
              (rate {
                m = "agent_api_calls_total";
                by = "model, finish_reason";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Model call latency p50/p95 by model";
            description = "The histogram's top bucket is 10 s, so a line flat at 10 s means 10 s or longer.";
            unit = "s";
            targets = p50p95 "agent_api_call_duration_seconds" "model" sel;
          })
          (ts {
            title = "Tokens/sec by kind";
            stack = true;
            targets = [
              (rate {
                m = "agent_tokens_total";
                by = "kind";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Provider errors/sec by provider, kind";
            targets = [
              (rate {
                m = "agent_provider_errors_total";
                by = "provider, kind";
              })
            ];
          })
        ];
      }
      {
        title = "Tools";
        panels = [
          (ts {
            title = "Tool calls/sec by tool";
            stack = true;
            targets = [
              (rate {
                m = "agent_tool_calls_total";
                by = "tool";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Tool errors/sec by tool, kind";
            targets = [
              (rate {
                m = "agent_tool_errors_total";
                by = "tool, kind";
              })
            ];
          })
        ];
      }
      {
        title = "Review fleet";
        panels = [
          (ts {
            title = "Triggers/sec by source, repo";
            targets = [
              (rate {
                m = "agent_fleet_triggers_total";
                by = "source, repo";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Review transitions/sec by status, repo";
            targets = [
              (rate {
                m = "agent_fleet_reviews_total";
                by = "status, repo";
                s = selUser;
              })
            ];
          })
        ];
      }
      {
        title = "Serving";
        panels = [
          (ts {
            title = "gRPC requests/sec by outcome";
            targets = [
              (rate {
                m = "agent_grpc_server_rpc_total";
                by = "outcome";
                s = selTenant;
              })
            ];
          })
          (ts {
            title = "gRPC p95 by rpc";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_grpc_server_rpc_seconds";
                by = "rpc";
              })
            ];
          })
        ];
      }
    ];
  };

  # --- LLM & router ---------------------------------------------------------------
  llm = g.dashboard {
    uid = "agent-seddon-llm";
    title = "agent-seddon · LLM & router";
    description = "Model calls, tokens, cost and the task router's upstream choices.";
    tags = [ "llm" ];
    vars = allVars;
    rows = [
      {
        title = "Calls";
        panels = [
          (ts {
            title = "Model calls/sec by model, finish reason";
            targets = [
              (rate {
                m = "agent_api_calls_total";
                by = "model, finish_reason";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Model call latency p50/p95 by model";
            description = "The histogram's top bucket is 10 s, so a line flat at 10 s means 10 s or longer.";
            unit = "s";
            targets = p50p95 "agent_api_call_duration_seconds" "model" sel;
          })
          (ts {
            title = "Provider request p95 by provider, stream";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_provider_request_seconds";
                by = "provider, stream";
              })
            ];
          })
          (ts {
            title = "Time to first token p50/p95 by provider";
            unit = "s";
            targets = p50p95 "agent_provider_ttft_seconds" "provider" sel;
          })
          (ts {
            title = "Stream chunks/sec by provider";
            targets = [
              (rate {
                m = "agent_provider_stream_chunks_total";
                by = "provider";
              })
            ];
          })
          (ts {
            title = "Provider errors/sec by provider, kind";
            targets = [
              (rate {
                m = "agent_provider_errors_total";
                by = "provider, kind";
              })
            ];
          })
        ];
      }
      {
        title = "Tokens & cost";
        panels = [
          (ts {
            title = "Tokens/sec by model, kind";
            stack = true;
            targets = [
              (rate {
                m = "agent_tokens_total";
                by = "model, kind";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Prompt-cache tokens/sec by kind";
            description = "read = served from the provider's prompt cache, write = cache created.";
            targets = [
              (rate {
                m = "agent_cache_tokens_total";
                by = "kind";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Prompt-cache hit share";
            unit = "percentunit";
            max = 1;
            targets = [
              (ratio ''sum(rate(agent_cache_tokens_total{${selUser}, kind="read"}[$__rate_interval]))''
                ''sum(rate(agent_tokens_total{${selUser}, kind="prompt"}[$__rate_interval]))''
                "hit share"
              )
            ];
          })
          (ts {
            title = "Tokens/sec by tenant";
            stack = true;
            targets = [
              (rate {
                m = "agent_tokens_total";
                by = "user";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Spend, USD/hour by model";
            description = "Priced by the loop's price table or the serving card; zero for unpriced local models.";
            unit = "currencyUSD";
            targets = [
              (q "sum by (model) (rate(agent_cost_usd_total{${selUser}}[$__rate_interval])) * 3600" "{{model}}")
            ];
          })
          (ts {
            title = "Router spend, USD/hour by upstream";
            description = "Priced from the serving provider card; empty until an upstream has a price.";
            unit = "currencyUSD";
            targets = [
              (q "sum by (upstream) (rate(agent_router_cost_usd_total{${sel}}[$__rate_interval])) * 3600" "{{upstream}}")
            ];
          })
          (ts {
            title = "Upstream tokens/sec by upstream, kind";
            targets = [
              (rate {
                m = "agent_upstream_tokens_total";
                by = "upstream, kind";
              })
            ];
          })
          (chTs {
            title = "Tokens per hour (agent_usage)";
            stack = true;
            sql = "SELECT toStartOfHour(ts) AS time, sum(prompt_tokens) AS prompt, sum(completion_tokens) AS completion FROM agent.agent_usage WHERE $__timeFilter(ts) GROUP BY time ORDER BY time";
          })
          (chTable {
            title = "Heaviest sessions (agent_usage)";
            sql = "SELECT session_id, user, count() AS calls, sum(prompt_tokens) AS prompt, sum(completion_tokens) AS completion, max(iter) AS iterations FROM agent.agent_usage WHERE $__timeFilter(ts) GROUP BY session_id, user ORDER BY prompt + completion DESC LIMIT 25";
          })
        ];
      }
      {
        title = "Task router";
        panels = [
          (ts {
            title = "Dispatches/sec by upstream, outcome";
            targets = [
              (rate {
                m = "agent_router_dispatch_total";
                by = "upstream, outcome";
              })
            ];
          })
          (ts {
            title = "In flight per upstream";
            stack = true;
            targets = [ (q "sum by (upstream) (agent_router_upstream_inflight{${sel}})" "{{upstream}}") ];
          })
          (ts {
            title = "Policy decisions/sec by role, rule, chosen";
            targets = [
              (rate {
                m = "agent_router_decisions_total";
                by = "role, rule, chosen";
              })
            ];
          })
          (ts {
            title = "Failovers/sec by from → to, reason";
            targets = [
              (rate {
                m = "agent_router_failover_total";
                by = "from, to, reason";
              })
            ];
          })
          (ts {
            title = "Route decisions/sec by target, decision";
            targets = [
              (rate {
                m = "agent_route_decisions_total";
                by = "target, decision";
              })
            ];
          })
          (ts {
            title = "Pool saturation sheds/sec";
            targets = [
              (rate {
                m = "agent_pool_saturation_shed_total";
                by = "job";
              })
            ];
          })
        ];
      }
      {
        title = "Model-call spans (traces)";
        panels = [
          (chTs {
            title = "Model calls per interval by model (gen_ai spans)";
            stack = true;
            sql = "SELECT $__timeInterval(Timestamp) AS time, SpanAttributes['gen_ai.request.model'] AS model, count() AS calls FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.operation.name'] = 'chat' GROUP BY time, model ORDER BY time";
          })
          (chTs {
            title = "Model call p95 by model (gen_ai spans)";
            unit = "ms";
            sql = "SELECT $__timeInterval(Timestamp) AS time, SpanAttributes['gen_ai.request.model'] AS model, quantile(0.95)(Duration) / 1e6 AS p95_ms FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.operation.name'] = 'chat' GROUP BY time, model ORDER BY time";
          })
          (chTable {
            title = "Model calls by model (gen_ai spans)";
            w = 24;
            sql = "SELECT SpanAttributes['gen_ai.provider.name'] AS provider, SpanAttributes['gen_ai.request.model'] AS model, count() AS calls, countIf(StatusCode = 'Error') AS errors, round(quantile(0.5)(Duration) / 1e6) AS p50_ms, round(quantile(0.95)(Duration) / 1e6) AS p95_ms, sum(toUInt64OrZero(SpanAttributes['gen_ai.usage.input_tokens'])) AS input_tokens, sum(toUInt64OrZero(SpanAttributes['gen_ai.usage.output_tokens'])) AS output_tokens, sum(toUInt64OrZero(SpanAttributes['gen_ai.usage.cache_read_input_tokens'])) AS cache_read_tokens, round(sum(toFloat64OrZero(SpanAttributes['gen_ai.usage.cost'])), 4) AS cost_usd FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.operation.name'] = 'chat' GROUP BY provider, model ORDER BY calls DESC";
          })
        ];
      }
    ];
  };

  # --- Review fleet ---------------------------------------------------------------
  review = g.dashboard {
    uid = "agent-seddon-review";
    title = "agent-seddon · review fleet";
    description = "Triggers, repo indexes, the static-analysis fan-out and the drafts it produced.";
    tags = [ "review" ];
    vars = allVars;
    time = "now-7d";
    rows = [
      {
        title = "Drafts";
        panels = [
          (chTable {
            title = "Latest drafts";
            w = 24;
            h = 10;
            sql = "SELECT ts, repo, pr_number, substring(head_sha, 1, 10) AS head, status, round(risk_score, 2) AS risk, gate_failed, n_findings, files_changed, additions, deletions, approved_by FROM agent.agent_review_drafts WHERE $__timeFilter(ts) ORDER BY ts DESC LIMIT 50";
          })
          (chTs {
            title = "Drafts per day by status";
            stack = true;
            sql = "SELECT toStartOfDay(ts) AS time, status, count() AS drafts FROM agent.agent_review_drafts WHERE $__timeFilter(ts) GROUP BY time, status ORDER BY time";
          })
          (chTable {
            title = "Open review feedback by repo, severity";
            description = "Items the reviews raised that a later push has not addressed.";
            sql = "SELECT repo, severity, count() AS open_items FROM (SELECT repo, item_id, argMax(severity, ts) AS severity, argMax(status, ts) AS status FROM agent.agent_review_feedback GROUP BY repo, item_id) WHERE status != 'addressed' GROUP BY repo, severity ORDER BY repo, open_items DESC";
          })
        ];
      }
      {
        title = "Triggers & reviews";
        panels = [
          (ts {
            title = "Triggers/sec by source, repo";
            targets = [
              (rate {
                m = "agent_fleet_triggers_total";
                by = "source, repo";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Review transitions/sec by status, repo";
            targets = [
              (rate {
                m = "agent_fleet_reviews_total";
                by = "status, repo";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Review runs/sec by project, outcome";
            targets = [
              (rate {
                m = "agent_review_runs_total";
                by = "project, outcome";
              })
            ];
          })
          (ts {
            title = "Fact collection p50/p95 by project";
            unit = "s";
            targets = p50p95 "agent_review_total_duration_seconds" "project" sel;
          })
          (ts {
            title = "Progress beats/sec by beat, outcome";
            targets = [
              (rate {
                m = "agent_fleet_progress_total";
                by = "beat, outcome";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Approvals/sec by outcome, repo";
            targets = [
              (rate {
                m = "agent_fleet_approvals_total";
                by = "outcome, repo";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Approval latency p95 by repo";
            description = "Draft recorded → approved.";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_fleet_approval_latency_seconds";
                by = "repo";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Post failures/sec by transport, repo";
            targets = [
              (rate {
                m = "agent_fleet_post_failures_total";
                by = "transport, repo";
                s = selUser;
              })
            ];
          })
        ];
      }
      {
        title = "Repo index";
        panels = [
          (ts {
            title = "Index jobs/sec by outcome, repo";
            targets = [
              (rate {
                m = "agent_fleet_index_runs_total";
                by = "outcome, repo";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Index build p50/p95 by repo";
            unit = "s";
            targets = p50p95 "agent_fleet_index_seconds" "repo" selUser;
          })
          (ts {
            title = "Index size by repo";
            unit = "bytes";
            targets = [ (q "max by (repo) (agent_fleet_index_bytes{${selUser}})" "{{repo}}") ];
          })
          (ts {
            title = "Indexed files by repo";
            targets = [ (q "max by (repo) (agent_fleet_index_files{${selUser}})" "{{repo}}") ];
          })
        ];
      }
      {
        title = "Analysis fan-out";
        panels = [
          (ts {
            title = "Collector p95 by collector";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_review_collector_duration_seconds";
                by = "collector";
              })
            ];
          })
          (ts {
            title = "Collector runs/sec by collector, status";
            targets = [
              (rate {
                m = "agent_review_collectors_total";
                by = "collector, status";
              })
            ];
          })
          (ts {
            title = "Findings/sec by tool, severity";
            stack = true;
            targets = [
              (rate {
                m = "agent_review_findings_total";
                by = "tool, severity";
              })
            ];
          })
          (ts {
            title = "Findings in the change vs elsewhere";
            stack = true;
            targets = [
              (rate {
                m = "agent_review_findings_total";
                by = "in_change";
              })
            ];
          })
          (ts {
            title = "Parallelism payoff (collector work ÷ wall clock) p50";
            targets = [
              (quantile {
                m = "agent_review_parallelism_ratio";
                phi = 0.5;
              })
            ];
          })
          (ts {
            title = "Highest file risk score p50/p95";
            max = 1;
            targets = p50p95 "agent_review_risk_max_score" "" sel;
          })
          (ts {
            title = "Changed files per review p50/p95";
            targets = p50p95 "agent_review_change_files" "" sel;
          })
          (ts {
            title = "Call-graph nodes / edges p95";
            targets = [
              (quantile {
                m = "agent_review_callgraph_nodes";
                legend = "nodes p95";
              })
              (quantile {
                m = "agent_review_callgraph_edges";
                legend = "edges p95";
              })
            ];
          })
          (ts {
            title = "Signature changes/sec by language, kind";
            targets = [
              (rate {
                m = "agent_review_signature_changes_total";
                by = "lang, kind";
              })
            ];
          })
          (ts {
            title = "Risk, salience, churn and co-change signals/sec";
            targets = [
              (rate {
                m = "agent_review_risk_total";
                by = "kind";
                legend = "risk {{kind}}";
              })
              (rate {
                m = "agent_review_salience_total";
                by = "kind";
                legend = "salience {{kind}}";
              })
              (rate {
                m = "agent_review_churn_total";
                by = "kind";
                legend = "churn {{kind}}";
              })
              (rate {
                m = "agent_review_cochange_total";
                by = "kind";
                legend = "co-change {{kind}}";
              })
            ];
          })
        ];
      }
      {
        title = "Tool and collector status (ClickHouse)";
        panels = [
          (chTable {
            title = "Static-analysis tools by status";
            sql = "SELECT tool, status, count() AS runs, sum(finding_count) AS findings, round(quantile(0.95)(duration_ms)) AS p95_ms, any(reason) AS example_reason FROM agent.agent_review_tools WHERE $__timeFilter(ts) GROUP BY tool, status ORDER BY tool, status";
          })
          (chTable {
            title = "Collectors that did not finish ok";
            sql = "SELECT collector, status, count() AS runs, any(reason) AS example_reason, max(ts) AS last FROM agent.agent_review_collectors WHERE $__timeFilter(ts) AND status != 'ok' GROUP BY collector, status ORDER BY runs DESC";
          })
          (chTable {
            title = "Slowest reviews";
            w = 24;
            sql = "SELECT ts, project, session_id, changed_files, findings, findings_in_diff, round(total_ms / 1000) AS total_s, round(sum_work_ms / greatest(total_ms, 1), 1) AS parallelism, critical_path FROM agent.agent_reviews WHERE $__timeFilter(ts) ORDER BY total_ms DESC LIMIT 25";
          })
        ];
      }
    ];
  };

  # --- Tools & sandbox ------------------------------------------------------------
  tools = g.dashboard {
    uid = "agent-seddon-tools";
    title = "agent-seddon · tools & sandbox";
    description = "What the model runs: tool calls, errors, latency, the sandbox and web fetches.";
    tags = [ "tools" ];
    vars = allVars;
    rows = [
      {
        title = "Tool calls";
        panels = [
          (ts {
            title = "Tool calls/sec by tool";
            stack = true;
            targets = [
              (rate {
                m = "agent_tool_calls_total";
                by = "tool";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Tool calls/sec by status";
            targets = [
              (rate {
                m = "agent_tool_calls_total";
                by = "status";
                s = selUser;
              })
            ];
          })
          (ts {
            title = "Tool latency p95 by tool";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_tool_exec_seconds";
                by = "tool";
              })
            ];
          })
          (ts {
            title = "Tool errors/sec by tool, kind";
            targets = [
              (rate {
                m = "agent_tool_errors_total";
                by = "tool, kind";
              })
            ];
          })
          (chTs {
            title = "Tool spans per interval by tool (traces)";
            stack = true;
            sql = "SELECT $__timeInterval(Timestamp) AS time, SpanAttributes['gen_ai.tool.name'] AS tool, count() AS calls FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.operation.name'] = 'execute_tool' GROUP BY time, tool ORDER BY time";
          })
          (chTable {
            title = "Slowest tool calls (traces)";
            sql = "SELECT Timestamp, ServiceName, SpanAttributes['gen_ai.tool.name'] AS tool, round(Duration / 1e6) AS ms, StatusCode, TraceId FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.operation.name'] = 'execute_tool' ORDER BY Duration DESC LIMIT 25";
          })
        ];
      }
      {
        title = "Sandbox & exec";
        panels = [
          (ts {
            title = "Sandboxed execs/sec by backend, outcome";
            targets = [
              (rate {
                m = "agent_sandbox_exec_total";
                by = "backend, outcome";
              })
            ];
          })
          (ts {
            title = "Sandboxed exec p50/p95 by backend";
            unit = "s";
            targets = p50p95 "agent_sandbox_exec_seconds" "backend" sel;
          })
          (ts {
            title = "Live pty sessions";
            targets = [ (q "sum(agent_pty_active_sessions{${sel}})" "pty") ];
          })
          (ts {
            title = "Content scan p95";
            unit = "s";
            targets = [ (quantile { m = "agent_scan_duration_seconds"; }) ];
          })
        ];
      }
      {
        title = "Web & references";
        panels = [
          (ts {
            title = "web_fetch p50/p95";
            unit = "s";
            targets = p50p95 "agent_web_fetch_seconds" "" sel;
          })
          (ts {
            title = "web_fetch body size p50/p95";
            unit = "bytes";
            targets = p50p95 "agent_web_fetch_bytes" "" sel;
          })
          (ts {
            title = "@-reference expansion p95";
            unit = "s";
            targets = [ (quantile { m = "agent_reference_resolve_seconds"; }) ];
          })
          (ts {
            title = "References dropped for the token budget/sec";
            targets = [
              (rate {
                m = "agent_reference_blocked_total";
                by = "job";
              })
            ];
          })
          (ts {
            title = "Content blocks/sec by modality";
            targets = [
              (rate {
                m = "agent_content_blocks_total";
                by = "modality";
              })
              (rate {
                m = "agent_content_blocks_dropped_total";
                legend = "dropped (no vision)";
              })
            ];
          })
          (ts {
            title = "Structured-output validation p95";
            unit = "s";
            targets = [ (quantile { m = "agent_structured_validate_seconds"; }) ];
          })
        ];
      }
    ];
  };

  # --- Sessions, context & memory -------------------------------------------------
  cognition = g.dashboard {
    uid = "agent-seddon-cognition";
    title = "agent-seddon · sessions, context & memory";
    description = "Working-set size, compaction, memory, task modes and the background distiller.";
    tags = [ "cognition" ];
    vars = allVars;
    rows = [
      {
        title = "Context";
        panels = [
          (ts {
            title = "Prompt tokens of the last request by session (top 10)";
            targets = [ (q "topk(10, max by (session) (agent_context_tokens{${selUser}}))" "{{session}}") ];
          })
          (ts {
            title = "Messages in the working set by session (top 10)";
            targets = [ (q "topk(10, max by (session) (agent_context_messages{${selUser}}))" "{{session}}") ];
          })
          (ts {
            title = "Context strategy p95 by op";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_context_op_seconds";
                by = "op";
              })
            ];
          })
          (ts {
            title = "Compactions/sec";
            targets = [
              (rate {
                m = "agent_context_compactions_total";
                by = "job";
              })
            ];
          })
          (ts {
            title = "Compaction tokens before / after";
            targets = [ (q "max by (when) (agent_context_compact_tokens{${sel}})" "{{when}}") ];
          })
          (ts {
            title = "Memory errors/sec by op";
            targets = [
              (rate {
                m = "agent_memory_errors_total";
                by = "op";
              })
            ];
          })
          (ts {
            title = "Prompt-cache breakpoints/sec by strategy";
            targets = [
              (rate {
                m = "agent_cache_breakpoints_total";
                by = "strategy";
              })
            ];
          })
          (ts {
            title = "Prompt fragments selected/sec by mode, action";
            targets = [
              (rate {
                m = "agent_prompt_fragments_selected_total";
                by = "mode, action";
              })
            ];
          })
        ];
      }
      {
        title = "Memory & distiller";
        panels = [
          (ts {
            title = "Memory op p95 by op";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_memory_op_seconds";
                by = "op";
              })
            ];
          })
          (ts {
            title = "Items per recall p50/p95";
            targets = p50p95 "agent_memory_recall_items" "" sel;
          })
          (ts {
            title = "Distiller jobs/sec by kind, outcome";
            targets = [
              (rate {
                m = "agent_distill_jobs_total";
                by = "kind, outcome";
              })
            ];
          })
          (ts {
            title = "Distiller lag p95 by kind";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_distill_lag_seconds";
                by = "kind";
              })
            ];
          })
          (ts {
            title = "Dimensional summarize p95";
            unit = "s";
            targets = [ (quantile { m = "agent_dimension_summarize_duration_seconds"; }) ];
          })
          (chTs {
            title = "Turn digests per interval by kind";
            stack = true;
            sql = "SELECT $__timeInterval(ts) AS time, kind, count() AS digests FROM agent.agent_turn_digests WHERE $__timeFilter(ts) GROUP BY time, kind ORDER BY time";
          })
        ];
      }
      {
        title = "Task modes, gate & plans";
        panels = [
          (ts {
            title = "Task-mode classifications/sec by mode, stage";
            targets = [
              (rate {
                m = "agent_mode_classifications_total";
                by = "mode, via";
              })
            ];
          })
          (ts {
            title = "Mode-switch confidence p50";
            max = 1;
            targets = [
              (quantile {
                m = "agent_mode_switch_confidence";
                phi = 0.5;
              })
            ];
          })
          (ts {
            title = "Consensus-gate critic rounds p50/p95";
            targets = p50p95 "agent_gate_rounds" "" sel;
          })
          (ts {
            title = "Gate alternatives recorded/sec";
            targets = [
              (rate {
                m = "agent_gate_alternatives_total";
                by = "job";
              })
            ];
          })
          (ts {
            title = "Plan todos (open / closed)";
            targets = [
              (q "sum(agent_tasks_open{${sel}})" "open")
              (q "sum(agent_tasks_closed{${sel}})" "closed")
            ];
          })
          (ts {
            title = "Checkpoint objects reclaimed/sec";
            targets = [
              (rate {
                m = "agent_session_gc_reclaimed_total";
                by = "job";
              })
            ];
          })
          (ts {
            title = "Scheduled run p95";
            unit = "s";
            targets = [ (quantile { m = "agent_scheduled_run_duration_seconds"; }) ];
          })
          (ts {
            title = "Campaign tick p95, errors/sec";
            targets = [
              (quantile {
                m = "agent_campaign_tick_seconds";
                legend = "tick p95 (s)";
              })
              (rate {
                m = "agent_campaign_tick_errors_total";
                legend = "errors/sec";
              })
            ];
          })
        ];
      }
    ];
  };

  # --- Platform: gRPC, stores, search, repos --------------------------------------
  platform = g.dashboard {
    uid = "agent-seddon-platform";
    title = "agent-seddon · gRPC, stores & search";
    description = "The serving seams, the config store and policy, the search index and the repo backend.";
    tags = [ "platform" ];
    vars = allVars;
    rows = [
      {
        title = "gRPC";
        panels = [
          (ts {
            title = "Requests/sec by rpc";
            stack = true;
            targets = [
              (rate {
                m = "agent_grpc_server_rpc_total";
                by = "rpc";
                s = selTenant;
              })
            ];
          })
          (ts {
            title = "Non-ok responses/sec by rpc, outcome";
            targets = [
              (q ''sum by (rpc, outcome) (rate(agent_grpc_server_rpc_total{${selTenant}, outcome!="ok"}[$__rate_interval]))'' "{{rpc}} {{outcome}}")
            ];
          })
          (ts {
            title = "Latency p95 by rpc";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_grpc_server_rpc_seconds";
                by = "rpc";
              })
            ];
          })
          (ts {
            title = "Requests shed under overload/sec";
            targets = [
              (rate {
                m = "agent_grpc_overload_shed_total";
                by = "job";
              })
            ];
          })
        ];
      }
      {
        title = "Config store & policy";
        panels = [
          (ts {
            title = "Config-store ops/sec by collection, op, outcome";
            targets = [
              (rate {
                m = "agent_config_store_ops_total";
                by = "collection, op, outcome";
                s = selTenant;
              })
            ];
          })
          (ts {
            title = "Config-store p95 by op";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_config_store_op_seconds";
                by = "op";
              })
            ];
          })
          (ts {
            title = "Policy decisions/sec by policy, decision";
            targets = [
              (rate {
                m = "agent_policy_authorize_total";
                by = "policy, decision";
                s = selTenant;
              })
            ];
          })
          (ts {
            title = "Policy authorize p95";
            unit = "s";
            targets = [ (quantile { m = "agent_policy_authorize_seconds"; }) ];
          })
        ];
      }
      {
        title = "Auth, guards & hooks";
        panels = [
          (ts {
            title = "Token verifications/sec by outcome";
            targets = [
              (rate {
                m = "agent_auth_verify_total";
                by = "outcome";
              })
            ];
          })
          (ts {
            title = "Authz decisions/sec by action, decision";
            targets = [
              (rate {
                m = "agent_authz_decisions_total";
                by = "action, decision";
              })
            ];
          })
          (ts {
            title = "Policy guard hits/sec by category, action";
            targets = [
              (rate {
                m = "agent_policy_guard_total";
                by = "category, action";
                s = selTenant;
              })
            ];
          })
          (ts {
            title = "Hook dispatches/sec by hook, point";
            targets = [
              (rate {
                m = "agent_hook_dispatches_total";
                by = "hook, point";
                s = selTenant;
              })
            ];
          })
          (ts {
            title = "Session-history ops/sec by op";
            targets = [
              (rate {
                m = "agent_session_ops_total";
                by = "op";
                s = selTenant;
              })
            ];
          })
        ];
      }
      {
        title = "Message transport";
        panels = [
          (ts {
            title = "Posts/sec by kind, outcome";
            targets = [
              (rate {
                m = "agent_transport_posts_total";
                by = "kind, outcome";
              })
            ];
          })
          (ts {
            title = "Post p95 by kind";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_transport_post_seconds";
                by = "kind";
              })
            ];
          })
          (ts {
            title = "Rate-limit decisions/sec by kind, decision";
            targets = [
              (rate {
                m = "agent_transport_ratelimit_total";
                by = "kind, decision";
              })
            ];
          })
        ];
      }
      {
        title = "Search index";
        panels = [
          (ts {
            title = "Index fresh (1 = matches the tree) by backend";
            max = 1;
            targets = [ (q "max by (backend) (agent_search_index_fresh{${sel}})" "{{backend}}") ];
          })
          (ts {
            title = "Indexed files by backend";
            targets = [ (q "max by (backend) (agent_search_index_files{${sel}})" "{{backend}}") ];
          })
          (ts {
            title = "Reindex p50/p95 by backend";
            unit = "s";
            targets = p50p95 "agent_search_index_seconds" "backend" sel;
          })
          (ts {
            title = "Reindexes/sec by backend, trigger";
            targets = [
              (rate {
                m = "agent_search_reindex_total";
                by = "backend, trigger";
              })
            ];
          })
          (ts {
            title = "Queries/sec by backend, mode";
            targets = [
              (q "sum by (backend, mode) (rate(agent_search_query_seconds_count{${sel}}[$__rate_interval]))" "{{backend}} {{mode}}")
            ];
          })
          (ts {
            title = "Query p50/p95 by backend, mode";
            unit = "s";
            targets = p50p95 "agent_search_query_seconds" "backend, mode" sel;
          })
          (ts {
            title = "Hits per query p50 by backend";
            targets = [
              (quantile {
                m = "agent_search_hits";
                phi = 0.5;
                by = "backend";
              })
            ];
          })
          (ts {
            title = "Search errors/sec by backend, op";
            targets = [
              (rate {
                m = "agent_search_errors_total";
                by = "backend, op";
              })
            ];
          })
        ];
      }
      {
        title = "Repo backend & graph";
        panels = [
          (ts {
            title = "Repo op p95 by backend, op";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_repo_op_seconds";
                by = "backend, op";
              })
            ];
          })
          (ts {
            title = "Repo errors/sec by backend, op";
            targets = [
              (rate {
                m = "agent_repo_errors_total";
                by = "backend, op";
              })
            ];
          })
          (ts {
            title = "Mirror fetch p95 by backend";
            unit = "s";
            targets = [
              (quantile {
                m = "agent_repo_fetch_seconds";
                by = "backend";
              })
            ];
          })
          (ts {
            title = "Live worktrees by backend";
            targets = [ (q "sum by (backend) (agent_repo_worktrees_live{${sel}})" "{{backend}}") ];
          })
          (ts {
            title = "Repo graph nodes / edges";
            targets = [
              (q "max(agent_repo_graph_nodes{${sel}})" "nodes")
              (q "max(agent_repo_graph_edges{${sel}})" "edges")
            ];
          })
          (ts {
            title = "Repo graph index p95, truncations/sec";
            targets = [
              (quantile {
                m = "agent_repo_graph_index_seconds";
                legend = "index p95 (s)";
              })
              (rate {
                m = "agent_repo_graph_truncated_total";
                legend = "truncated/sec";
              })
            ];
          })
        ];
      }
    ];
  };

  # --- Traces (ClickHouse otel_traces) --------------------------------------------
  traces = g.dashboard {
    uid = "agent-seddon-traces";
    title = "agent-seddon · traces";
    description = "Spans the agents export over OTLP, read straight from ClickHouse (default.otel_traces). Open a TraceId in HyperDX for the waterfall.";
    tags = [ "traces" ];
    vars = [ g.vars.clickhouse ];
    time = "now-6h";
    rows = [
      {
        title = "Spans";
        panels = [
          (chTs {
            title = "Spans per interval by service";
            stack = true;
            sql = "SELECT $__timeInterval(Timestamp) AS time, ServiceName, count() AS spans FROM default.otel_traces WHERE $__timeFilter(Timestamp) GROUP BY time, ServiceName ORDER BY time";
          })
          (chTs {
            title = "Error spans per interval by service";
            sql = "SELECT $__timeInterval(Timestamp) AS time, ServiceName, count() AS errors FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND StatusCode = 'Error' GROUP BY time, ServiceName ORDER BY time";
          })
          (chTable {
            title = "Span names by p95 duration";
            sql = "SELECT ServiceName, SpanName, count() AS spans, countIf(StatusCode = 'Error') AS errors, round(quantile(0.5)(Duration) / 1e6, 1) AS p50_ms, round(quantile(0.95)(Duration) / 1e6, 1) AS p95_ms FROM default.otel_traces WHERE $__timeFilter(Timestamp) GROUP BY ServiceName, SpanName ORDER BY p95_ms DESC LIMIT 50";
          })
          (chTable {
            title = "Slowest root spans";
            sql = "SELECT Timestamp, ServiceName, SpanName, round(Duration / 1e6) AS ms, StatusCode, TraceId FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND ParentSpanId = '' ORDER BY Duration DESC LIMIT 25";
          })
          (chTable {
            title = "Latest error spans";
            w = 24;
            sql = "SELECT Timestamp, ServiceName, SpanName, StatusMessage, SpanAttributes['error.type'] AS error_type, TraceId FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND StatusCode = 'Error' ORDER BY Timestamp DESC LIMIT 50";
          })
        ];
      }
      {
        title = "LLM (gen_ai spans)";
        panels = [
          (chTs {
            title = "Input / output tokens per interval";
            stack = true;
            sql = "SELECT $__timeInterval(Timestamp) AS time, sum(toUInt64OrZero(SpanAttributes['gen_ai.usage.input_tokens'])) AS input, sum(toUInt64OrZero(SpanAttributes['gen_ai.usage.output_tokens'])) AS output FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.operation.name'] = 'chat' GROUP BY time ORDER BY time";
          })
          (chTs {
            title = "Model calls by finish reason";
            stack = true;
            sql = "SELECT $__timeInterval(Timestamp) AS time, SpanAttributes['gen_ai.response.finish_reasons'] AS finish, count() AS calls FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.operation.name'] = 'chat' GROUP BY time, finish ORDER BY time";
          })
          (chTable {
            title = "Busiest conversations";
            w = 24;
            sql = "SELECT SpanAttributes['gen_ai.conversation.id'] AS conversation, countIf(SpanAttributes['gen_ai.operation.name'] = 'chat') AS model_calls, countIf(SpanAttributes['gen_ai.operation.name'] = 'execute_tool') AS tool_calls, sum(toUInt64OrZero(SpanAttributes['gen_ai.usage.input_tokens'])) AS input_tokens, sum(toUInt64OrZero(SpanAttributes['gen_ai.usage.output_tokens'])) AS output_tokens, round(sumIf(Duration, SpanAttributes['gen_ai.operation.name'] = 'chat') / 1e9) AS model_s, min(Timestamp) AS first, max(Timestamp) AS last FROM default.otel_traces WHERE $__timeFilter(Timestamp) AND SpanAttributes['gen_ai.conversation.id'] != '' GROUP BY conversation ORDER BY input_tokens + output_tokens DESC LIMIT 25";
          })
        ];
      }
    ];
  };
in
{
  inherit
    overview
    llm
    review
    tools
    cognition
    platform
    traces
    ;
}
