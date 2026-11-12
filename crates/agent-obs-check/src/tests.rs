use super::*;
use rstest::rstest;

fn ds(uid: &str, kind: &str, is_default: bool) -> DataSource {
    DataSource {
        uid: uid.into(),
        kind: kind.into(),
        is_default,
    }
}

fn sources() -> Vec<DataSource> {
    vec![
        ds("prom-a", "prometheus", false),
        ds("prom-b", "prometheus", true),
        ds("clickhouse", "grafana-clickhouse-datasource", false),
    ]
}

/// A dashboard shaped like nix/grafana/dashboards.nix writes them.
fn dash() -> Value {
    json!({
        "uid": "d1",
        "templating": { "list": [
            { "name": "prometheus", "type": "datasource", "query": "prometheus" },
            { "name": "clickhouse", "type": "datasource", "query": "grafana-clickhouse-datasource" },
            { "name": "job", "type": "query", "includeAll": true, "allValue": ".*" },
            { "name": "jobs", "type": "custom", "current": { "value": "many" } },
            { "name": "page", "type": "custom", "query": "home, agent" }
        ]},
        "panels": [
            { "title": "rate", "datasource": { "uid": "${prometheus}" },
              "targets": [ { "refId": "Q0", "expr": "rate(x{job=~\"$job\"}[$__rate_interval])" } ] },
            { "title": "row", "type": "row", "panels": [
                { "title": "sql", "targets": [ { "refId": "Q0",
                    "datasource": { "uid": "${clickhouse}", "type": "grafana-clickhouse-datasource" },
                    "rawSql": "SELECT 1 WHERE $__timeFilter(ts) AND page = '${page}'" } ] }
            ]}
        ]
    })
}

fn vars() -> Vars {
    Vars::from_dashboard(&dash(), &sources()).unwrap()
}

#[rstest]
#[case::positive_dollar("x{job=~\"$job\"}", "x{job=~\".*\"}")]
#[case::positive_braces("${job}", ".*")]
#[case::positive_braces_with_format("${job:regex}", ".*")]
#[case::positive_brackets("[[jobs]]", "many")]
#[case::positive_datasource_var("${clickhouse}", "clickhouse")]
#[case::corner_exact_name_not_a_prefix("$jobs/$job", "many/.*")]
#[case::corner_grafana_macros_left_alone(
    "[$__rate_interval] $__timeFilter(ts)",
    "[$__rate_interval] $__timeFilter(ts)"
)]
#[case::negative_unknown_name_kept("$nope ${nope} [[nope]]", "$nope ${nope} [[nope]]")]
#[case::boundary_name_at_end("a $job", "a .*")]
#[case::boundary_lone_dollar_and_bracket("$ [ ${ [[", "$ [ ${ [[")]
#[case::adversarial_unicode_survives("é$jobé", "é.*é")]
fn interpolate_cases(#[case] input: &str, #[case] want: &str) {
    assert_eq!(interpolate(input, &vars()), want);
}

/// A value that itself looks like a variable is not expanded again.
#[test]
fn adversarial_a_value_is_never_expanded_twice() {
    let d = json!({ "templating": { "list": [
        { "name": "a", "type": "custom", "current": { "value": "$b" } },
        { "name": "b", "type": "custom", "current": { "value": "boom" } }
    ]}});
    let v = Vars::from_dashboard(&d, &[]).unwrap();
    assert_eq!(interpolate("$a", &v), "$b");
}

#[rstest]
#[case::positive_saved_value(json!({ "current": { "value": "x" } }), "x")]
#[case::positive_all_uses_all_value(json!({ "current": { "value": "$__all" }, "allValue": "a|b" }), "a|b")]
#[case::corner_all_without_all_value(json!({ "current": { "value": ["$__all"] } }), ".*")]
#[case::corner_include_all_unset_current(json!({ "includeAll": true }), ".*")]
#[case::corner_first_option(json!({ "options": [ { "value": "o1" }, { "value": "o2" } ] }), "o1")]
#[case::corner_first_of_custom_query(json!({ "query": " p1 , p2" }), "p1")]
#[case::negative_nothing_at_all(json!({}), "")]
fn var_value_cases(#[case] v: Value, #[case] want: &str) {
    assert_eq!(var_value(&v), want);
}

#[test]
fn positive_panel_queries_fill_variables_and_resolve_sources() {
    let qs = panel_queries(&dash(), &sources()).unwrap();
    assert_eq!(qs.len(), 2);
    assert_eq!(qs[0].panel, "rate");
    // `${prometheus}` resolves to the default Prometheus.
    assert_eq!(qs[0].source.uid, "prom-b");
    assert_eq!(
        qs[0].target["expr"],
        "rate(x{job=~\".*\"}[$__rate_interval])"
    );
    assert_eq!(
        qs[0].target["datasource"],
        json!({ "uid": "prom-b", "type": "prometheus" })
    );
    // A row's panels are followed; Grafana's macro is left for the plugin.
    assert_eq!(qs[1].panel, "sql");
    assert_eq!(qs[1].source.uid, "clickhouse");
    assert_eq!(
        qs[1].target["rawSql"],
        "SELECT 1 WHERE $__timeFilter(ts) AND page = 'home'"
    );
}

#[rstest]
#[case::negative_no_source_of_the_variables_type(
    json!({ "templating": { "list": [ { "name": "x", "type": "datasource", "query": "loki" } ] } }),
    "no data source of type \"loki\""
)]
#[case::negative_unknown_uid(
    json!({ "panels": [ { "title": "p", "targets": [ { "datasource": { "uid": "gone" } } ] } ] }),
    "\"gone\" () is not provisioned"
)]
#[case::negative_undefined_source_variable(
    json!({ "panels": [ { "title": "p", "targets": [ { "datasource": { "uid": "${ghost}" } } ] } ] }),
    "variable \"ghost\" is not defined"
)]
#[case::adversarial_target_not_an_object(
    json!({ "panels": [ { "title": "p", "targets": [ "DROP TABLE" ] } ] }),
    "a target is not an object"
)]
#[case::adversarial_variable_name_injection(
    json!({ "templating": { "list": [ { "name": "a}b", "type": "custom" } ] } }),
    "not a name"
)]
#[case::adversarial_grafana_owned_variable_name(
    json!({ "templating": { "list": [ { "name": "__rate_interval", "type": "custom" } ] } }),
    "not a name"
)]
fn panel_queries_refusals(#[case] d: Value, #[case] frag: &str) {
    let e = panel_queries(&d, &sources()).unwrap_err().to_string();
    assert!(e.contains(frag), "{e}");
}

#[test]
fn corner_no_source_named_uses_the_default() {
    let d = json!({ "panels": [ { "title": "p", "targets": [ { "expr": "up" } ] } ] });
    let qs = panel_queries(&d, &sources()).unwrap();
    assert_eq!(qs[0].source.uid, "prom-b");
}

#[test]
fn adversarial_deep_nesting_is_refused() {
    let mut p = json!({ "title": "leaf", "targets": [ { "expr": "up" } ] });
    for _ in 0..=MAX_DEPTH + 1 {
        p = json!({ "title": "row", "panels": [ p ] });
    }
    let e = panel_queries(&json!({ "panels": [ p ] }), &sources()).unwrap_err();
    assert!(e.to_string().contains("nest deeper"), "{e}");
}

#[test]
fn boundary_query_count_is_capped() {
    let target = json!({ "expr": "up" });
    let panel = json!({ "title": "p", "targets": vec![target; MAX_QUERIES + 1] });
    let e = panel_queries(&json!({ "panels": [ panel ] }), &sources()).unwrap_err();
    assert!(e.to_string().contains("more than"), "{e}");
}

fn named(d: Value) -> (String, Value) {
    ("d.json".into(), d)
}

fn panel_with(n: usize) -> Value {
    json!({ "panels": [ { "title": "p", "targets": vec![json!({ "expr": "up" }); n] } ] })
}

/// desc: `all_queries` is what a run sends: never nothing, never more than the cap in
/// all (each dashboard can be under it while the sum is over).
#[rstest]
#[case::positive_two_dashboards(vec![named(dash()), named(dash())], Ok(4))]
#[case::boundary_at_the_cap_in_all(vec![named(panel_with(MAX_QUERIES / 2)), named(panel_with(MAX_QUERIES / 2))], Ok(MAX_QUERIES))]
#[case::boundary_over_the_cap_in_all(vec![named(panel_with(MAX_QUERIES / 2)), named(panel_with(MAX_QUERIES / 2 + 1))], Err("in all"))]
#[case::negative_no_dashboards(vec![], Err("no panel queries in 0 dashboard(s)"))]
#[case::negative_dashboards_without_queries(vec![named(json!({ "panels": [ { "title": "text" } ] }))], Err("no panel queries in 1"))]
#[case::corner_error_names_the_file(vec![named(json!({ "panels": [ { "title": "p", "targets": [ 1 ] } ] }))], Err("d.json: "))]
fn all_queries_cases(
    #[case] ds: Vec<(String, Value)>,
    #[case] want: std::result::Result<usize, &str>,
) {
    match (all_queries(&ds, &sources()), want) {
        (Ok(qs), Ok(n)) => assert_eq!(qs.len(), n),
        (Err(e), Err(frag)) => assert!(e.to_string().contains(frag), "{e}"),
        (got, want) => panic!("got {:?}, want {want:?}", got.map(|q| q.len())),
    }
}

#[test]
fn positive_query_body_carries_the_ref_and_range() {
    let qs = panel_queries(&dash(), &sources()).unwrap();
    let b = query_body(&qs[0], "now-24h", "now");
    assert_eq!(b["from"], "now-24h");
    assert_eq!(b["queries"][0]["refId"], "Q0");
    assert_eq!(b["queries"][0]["datasource"]["uid"], "prom-b");
    assert_eq!(b["queries"][0]["intervalMs"], 60_000);
}

#[rstest]
#[case::positive_rows(200, json!({ "results": { "A": { "status": 200, "frames": [
    { "data": { "values": [ [1, 2, 3], ["a", "b", "c"] ] } },
    { "data": { "values": [ [4] ] } } ] } } }), Outcome::Data(4))]
#[case::corner_no_frames(200, json!({ "results": { "A": { "status": 200 } } }), Outcome::Empty)]
#[case::corner_empty_frame(200, json!({ "results": { "A": { "frames": [ { "data": { "values": [] } } ] } } }), Outcome::Empty)]
#[case::negative_plugin_error(400, json!({ "results": { "A": { "error": "Code: 47. Unknown identifier", "status": 400 } } }),
    Outcome::Failed("Code: 47. Unknown identifier".into()))]
#[case::negative_result_status_without_message(200, json!({ "results": { "A": { "status": 500 } } }),
    Outcome::Failed("HTTP 200 (result 500)".into()))]
#[case::negative_http_error_with_message(401, json!({ "message": "Unauthorized" }), Outcome::Failed("Unauthorized".into()))]
#[case::adversarial_other_ref_only(200, json!({ "results": { "B": { "frames": [ { "data": { "values": [ [1] ] } } ] } } }),
    Outcome::Failed("HTTP 200, no result for A".into()))]
#[case::adversarial_not_an_object(200, json!("ok"), Outcome::Failed("HTTP 200, no result for A".into()))]
#[case::adversarial_values_not_arrays(200, json!({ "results": { "A": { "frames": [ { "data": { "values": "x" } } ] } } }), Outcome::Empty)]
// A status that is there but not a number is not Grafana's "ok", whatever the frames say.
#[case::adversarial_string_status(200, json!({ "results": { "A": { "status": "500", "frames": [ { "data": { "values": [ [1] ] } } ] } } }),
    Outcome::Failed("HTTP 200, a result status of \"500\"".into()))]
#[case::adversarial_null_status(200, json!({ "results": { "A": { "status": null, "frames": [ { "data": { "values": [ [1] ] } } ] } } }),
    Outcome::Failed("HTTP 200, a result status of null".into()))]
#[case::adversarial_negative_status(200, json!({ "results": { "A": { "status": -1 } } }),
    Outcome::Failed("HTTP 200, a result status of -1".into()))]
#[case::adversarial_newline_in_error(400, json!({ "results": { "A": { "error": "x\nFAILED  forged" } } }),
    Outcome::Failed("x FAILED  forged".into()))]
fn classify_cases(#[case] status: u16, #[case] body: Value, #[case] want: Outcome) {
    assert_eq!(classify("A", status, &body), want);
}

#[test]
fn boundary_error_message_is_bounded() {
    let body = json!({ "results": { "A": { "error": "e".repeat(10_000) } } });
    let Outcome::Failed(m) = classify("A", 400, &body) else {
        panic!("expected a failure");
    };
    assert_eq!(m.chars().count(), MAX_ERROR_CHARS);
}

#[rstest]
#[case::positive(json!([ { "uid": "prom", "type": "prometheus", "isDefault": true } ]), Ok(vec![ds("prom", "prometheus", true)]))]
#[case::corner_empty(json!([]), Ok(vec![]))]
#[case::negative_not_an_array(json!({}), Err("not an array"))]
#[case::negative_missing_uid(json!([ { "type": "prometheus" } ]), Err("malformed"))]
#[case::adversarial_uid_with_quote(json!([ { "uid": "a\"b", "type": "prometheus" } ]), Err("malformed"))]
#[case::adversarial_uid_too_long(json!([ { "uid": "a".repeat(41), "type": "prometheus" } ]), Err("malformed"))]
fn parse_datasources_cases(
    #[case] v: Value,
    #[case] want: std::result::Result<Vec<DataSource>, &str>,
) {
    match (parse_datasources(&v), want) {
        (Ok(got), Ok(want)) => assert_eq!(got, want),
        (Err(e), Err(frag)) => assert!(e.to_string().contains(frag), "{e}"),
        (got, want) => panic!("got {got:?}, want {want:?}"),
    }
}

#[rstest]
#[case::positive("default.otel_traces", true)]
#[case::boundary_128_bytes(&format!("d.{}", "t".repeat(126)), true)]
#[case::boundary_129_bytes(&format!("d.{}", "t".repeat(127)), false)]
#[case::negative_no_db("otel_traces", false)]
#[case::negative_empty_part("default.", false)]
#[case::corner_three_parts("a.b.c", false)]
#[case::adversarial_injection("default.otel_traces; DROP", false)]
#[case::adversarial_quote("default.`otel_traces`", false)]
fn valid_table_cases(#[case] s: &str, #[case] want: bool) {
    assert_eq!(valid_table(s), want);
}

fn sql_query(field: &str, text: &str) -> PanelQuery {
    PanelQuery {
        dashboard: "d".into(),
        panel: "p".into(),
        ref_id: "A".into(),
        source: ds("clickhouse", "grafana-clickhouse-datasource", false),
        target: json!({ field: text }),
    }
}

#[rstest]
#[case::positive("rawSql", "SELECT 1 FROM default.otel_traces WHERE x", true)]
#[case::positive_join(
    "rawSql",
    "FROM a p INNER JOIN default.otel_traces AS t ON t.x = p.y",
    true
)]
#[case::corner_case_insensitive("rawSql", "FROM DEFAULT.OTEL_TRACES", true)]
#[case::boundary_at_end("rawSql", "FROM default.otel_traces", true)]
#[case::boundary_followed_by_paren("rawSql", "FROM (SELECT * FROM default.otel_traces)", true)]
#[case::negative_other_table("rawSql", "FROM agent.agent_logs", false)]
#[case::negative_longer_name("rawSql", "FROM default.otel_traces_1m", false)]
#[case::negative_prefixed_name("rawSql", "FROM xdefault.otel_traces", false)]
#[case::negative_not_a_text_field("format", "default.otel_traces", false)]
// A quoted name isn't recognised: the query then runs and fails, it is never hidden.
#[case::corner_quoted_name_runs("rawSql", "FROM `default`.`otel_traces`", false)]
#[case::adversarial_name_in_a_longer_path("rawSql", "FROM other.default.otel_traces", false)]
fn names_table_cases(#[case] field: &str, #[case] text: &str, #[case] want: bool) {
    assert_eq!(
        names_table(&sql_query(field, text), "default.otel_traces"),
        want
    );
}
