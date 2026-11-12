//! The HyperDX tiles and saved searches (`nix/hyperdx/dashboards.nix`, as the template
//! `hyperdx-provision` writes), each rendered to the ClickHouse SQL it asks for and run
//! as HyperDX's own connection (the read-only `agent_viewer`).
//!
//! HyperDX builds a tile's SQL in the browser from its parts: aggregates over a value
//! expression, a SQL `where`, a `groupBy`, and a time bucket for the charts. The parts
//! are what this repo writes, so they are what can be wrong (a column that isn't there,
//! a typo'd function); [`render_tile`] puts them together the same way, so a part that
//! fails here would leave a tile blank or red. The bucket width and the exact wrapping
//! HyperDX adds are not reproduced; the parts' SQL is.
//!
//! The template is the repo's own, but parts are spliced into SQL, so they are checked
//! anyway: a `;` (a second statement) or a NUL is refused, a level must be in (0, 1),
//! an unknown aggregate fails closed, and each part is bounded. The login is read-only
//! besides, and ClickHouse's HTTP interface runs one statement per request.

use crate::{bounded, invalid, valid_table, Outcome, Result};
use serde_json::Value;

/// Most tiles and saved searches read from one template.
pub const MAX_ITEMS: usize = 500;
/// Longest SQL part (a `where`, a value expression, a `groupBy`) accepted.
pub const MAX_PART_CHARS: usize = 4096;
/// Most rows a check asks ClickHouse for.
pub const MAX_ROWS: usize = 1000;

/// One tile or saved search, rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileQuery {
    /// The dashboard's name, or `saved search` for a saved search.
    pub dashboard: String,
    pub name: String,
    pub sql: String,
}

/// Where a `"$trace"` source reads: its table and its timestamp column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub table: String,
    pub timestamp: String,
}

/// A `now-<n><unit>` Grafana-style start (`s`, `m`, `h`, `d`, `w`), in seconds: the
/// window the tiles are checked over, the same one the Grafana panels use.
pub fn relative_secs(from: &str) -> Option<u64> {
    let rest = from.strip_prefix("now-")?;
    let unit = rest.chars().last()?;
    let n: u64 = rest[..rest.len() - unit.len_utf8()].parse().ok()?;
    let per = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        'w' => 604_800,
        _ => return None,
    };
    n.checked_mul(per).filter(|s| *s > 0)
}

/// A SQL part from the template: a string, bounded, with no second statement in it.
fn part<'a>(v: Option<&'a Value>, what: &str) -> Result<&'a str> {
    let s = match v {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err(invalid(format!("{what} is not a string"))),
    };
    if s.chars().count() > MAX_PART_CHARS {
        return Err(invalid(format!(
            "{what} is over {MAX_PART_CHARS} characters"
        )));
    }
    if s.contains([';', '\0']) {
        return Err(invalid(format!("{what} holds a `;` or a NUL")));
    }
    Ok(s.trim())
}

/// One aggregate of a tile's `select`, as SQL.
fn aggregate(a: &Value) -> Result<String> {
    let f = a
        .get("aggFn")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("an aggregate has no aggFn"))?;
    let expr = part(a.get("valueExpression"), "valueExpression")?;
    let cond = part(a.get("aggCondition"), "aggCondition")?;
    let need = |e: &str| {
        if e.is_empty() {
            Err(invalid(format!("{f} needs a valueExpression")))
        } else {
            Ok(())
        }
    };
    // `fIf(args…, cond)` when there is a condition, as HyperDX writes it.
    let call = |name: &str, args: &str| {
        let args = match (args.is_empty(), cond.is_empty()) {
            (_, true) => args.to_string(),
            (true, false) => cond.to_string(),
            (false, false) => format!("{args}, {cond}"),
        };
        let suffix = if cond.is_empty() { "" } else { "If" };
        format!("{name}{suffix}({args})")
    };
    Ok(match f {
        "count" => call("count", ""),
        "count_distinct" => {
            need(expr)?;
            call("uniq", expr)
        }
        "sum" | "avg" | "min" | "max" => {
            need(expr)?;
            call(f, expr)
        }
        "quantile" => {
            need(expr)?;
            let level = a
                .get("level")
                .and_then(Value::as_f64)
                .filter(|l| l.is_finite() && *l > 0.0 && *l < 1.0)
                .ok_or_else(|| invalid("a quantile's level is not in (0, 1)"))?;
            let inner = call("quantile", expr);
            // `quantile(expr)` → `quantile(0.95)(expr)`.
            let (name, args) = inner.split_once('(').unwrap_or((&inner, ")"));
            format!("{name}({level})({args}")
        }
        other => {
            return Err(invalid(format!(
                "aggregate {other:?} is not one this check renders"
            )))
        }
    })
}

/// Resolve a template's `source` (`"$trace"`) to where it reads.
fn source<'a>(v: Option<&Value>, trace: &'a Source) -> Result<&'a Source> {
    match v.and_then(Value::as_str) {
        Some("$trace") => Ok(trace),
        Some(other) => Err(invalid(format!("source {other:?} is not \"$trace\""))),
        None => Err(invalid("no source")),
    }
}

/// `WHERE <ts> over the last <window> s [AND (<where>)]`.
fn filter(src: &Source, window: u64, where_: &str) -> String {
    let mut w = format!(
        "{ts} >= now64(9) - INTERVAL {window} SECOND",
        ts = src.timestamp
    );
    if !where_.is_empty() {
        w.push_str(&format!(" AND ({where_})"));
    }
    w
}

/// A dashboard tile's `config`, as the SQL HyperDX asks ClickHouse for.
pub fn render_tile(cfg: &Value, trace: &Source, window: u64) -> Result<String> {
    let src = source(cfg.get("source"), trace)?;
    if let Some(lang) = cfg.get("whereLanguage").and_then(Value::as_str) {
        if lang != "sql" {
            return Err(invalid(format!("whereLanguage {lang:?} is not \"sql\"")));
        }
    }
    let aggs = cfg
        .get("select")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| invalid("select is not a list of aggregates"))?;
    let mut cols = Vec::with_capacity(aggs.len() + 2);
    for (i, a) in aggs.iter().enumerate() {
        cols.push(format!("{} AS `s{i}`", aggregate(a)?));
    }
    let group = part(cfg.get("groupBy"), "groupBy")?;
    let mut keys = Vec::new();
    let charted = matches!(
        cfg.get("displayType").and_then(Value::as_str),
        Some("line" | "stacked_bar")
    );
    if charted {
        keys.push(format!(
            "toStartOfInterval({}, INTERVAL 1 MINUTE) AS `__bucket`",
            src.timestamp
        ));
    }
    if !group.is_empty() {
        keys.push(group.to_string());
    }
    let where_ = part(cfg.get("where"), "where")?;
    let mut sql = format!(
        "SELECT {} FROM {} WHERE {}",
        cols.into_iter()
            .chain(keys.iter().cloned())
            .collect::<Vec<_>>()
            .join(", "),
        src.table,
        filter(src, window, where_)
    );
    if !keys.is_empty() {
        let by: Vec<&str> = keys
            .iter()
            .map(|k| {
                k.strip_suffix(" AS `__bucket`")
                    .map_or(k.as_str(), |_| "`__bucket`")
            })
            .collect();
        sql.push_str(&format!(" GROUP BY {}", by.join(", ")));
    }
    sql.push_str(&format!(" LIMIT {MAX_ROWS}"));
    Ok(sql)
}

/// A saved search, as the count HyperDX's search page starts with.
pub fn render_search(s: &Value, trace: &Source, window: u64) -> Result<String> {
    let src = source(s.get("source"), trace)?;
    let where_ = part(s.get("where"), "where")?;
    Ok(format!(
        "SELECT count() AS `s0` FROM {} WHERE {}",
        src.table,
        filter(src, window, where_)
    ))
}

/// Every tile and saved search in a template, rendered. At least one must be there.
pub fn tile_queries(template: &Value, trace: &Source, window: u64) -> Result<Vec<TileQuery>> {
    if !valid_table(&trace.table) {
        return Err(invalid(format!("{:?} is not db.table", trace.table)));
    }
    if trace.timestamp.is_empty()
        || !trace
            .timestamp
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(invalid(format!("{:?} is not a column", trace.timestamp)));
    }
    let list = |k: &str| -> Result<&[Value]> {
        match template.get(k) {
            None => Ok(&[]),
            Some(Value::Array(a)) => Ok(a.as_slice()),
            Some(_) => Err(invalid(format!("{k} is not a list"))),
        }
    };
    let name_of = |v: &Value| bounded(v.get("name").and_then(Value::as_str).unwrap_or("?"));
    let mut out = Vec::new();
    let mut push = |q: TileQuery| {
        if out.len() >= MAX_ITEMS {
            return Err(invalid(format!("more than {MAX_ITEMS} tiles and searches")));
        }
        out.push(q);
        Ok(())
    };
    for d in list("dashboards")? {
        let dashboard = name_of(d);
        let tiles = d
            .get("tiles")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid(format!("{dashboard}: no tiles list")))?;
        for t in tiles {
            let cfg = t.get("config").unwrap_or(t);
            let name = name_of(cfg);
            let sql = render_tile(cfg, trace, window).map_err(|crate::Error::Invalid(e)| {
                invalid(format!("{dashboard} / {name}: {e}"))
            })?;
            push(TileQuery {
                dashboard: dashboard.clone(),
                name,
                sql,
            })?;
        }
    }
    for s in list("savedSearches")? {
        let name = name_of(s);
        let sql = render_search(s, trace, window)
            .map_err(|crate::Error::Invalid(e)| invalid(format!("saved search / {name}: {e}")))?;
        push(TileQuery {
            dashboard: "saved search".into(),
            name,
            sql,
        })?;
    }
    if out.is_empty() {
        return Err(invalid("no tiles or saved searches in the template"));
    }
    Ok(out)
}

/// A value that says something happened: not null, zero, NaN or empty. ClickHouse
/// quotes 64-bit numbers in JSON, so a numeric string is read as its number.
fn signal(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => false,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && f.is_finite()),
        Value::String(s) => match s.parse::<f64>() {
            Ok(f) => f != 0.0 && f.is_finite(),
            Err(_) => !s.is_empty() && s != "nan" && s != "inf" && s != "-inf",
        },
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Bool(true) => true,
    }
}

/// The verdict on a ClickHouse HTTP reply (`FORMAT JSON`). An aggregate without a
/// `GROUP BY` always returns one row, so rows count as data only when one of their
/// values does ([`signal`]): `count() = 0` over an empty window is `Empty`.
pub fn classify_reply(status: u16, body: &[u8]) -> Outcome {
    if !(200..300).contains(&status) {
        let text = String::from_utf8_lossy(body);
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        return Outcome::Failed(format!("HTTP {status}: {}", bounded(first.trim())));
    }
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return Outcome::Failed(format!("HTTP {status}: a reply that is not JSON"));
    };
    let Some(rows) = v.get("data").and_then(Value::as_array) else {
        return Outcome::Failed(format!("HTTP {status}: a reply with no data list"));
    };
    let with = rows
        .iter()
        .filter(|r| match r {
            Value::Object(o) => o.iter().any(|(k, v)| k != "__bucket" && signal(v)),
            other => signal(other),
        })
        .count();
    if with == 0 {
        Outcome::Empty
    } else {
        Outcome::Data(with)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;

    fn trace() -> Source {
        Source {
            table: "default.otel_traces".into(),
            timestamp: "Timestamp".into(),
        }
    }

    fn count() -> Value {
        json!({ "aggFn": "count", "valueExpression": "", "aggCondition": "", "alias": "n" })
    }

    fn tile(extra: Value) -> Value {
        let mut t = json!({ "name": "t", "source": "$trace", "displayType": "table",
                            "whereLanguage": "sql", "where": "", "select": [ count() ] });
        for (k, v) in extra.as_object().unwrap() {
            t[k] = v.clone();
        }
        t
    }

    const W: &str = "Timestamp >= now64(9) - INTERVAL 3600 SECOND";

    #[rstest]
    #[case::positive_table(tile(json!({})),
        Ok(format!("SELECT count() AS `s0` FROM default.otel_traces WHERE {W} LIMIT 1000")))]
    #[case::positive_chart_buckets_and_groups(
        tile(json!({ "displayType": "stacked_bar", "groupBy": "ServiceName", "where": "StatusCode = 'Error'" })),
        Ok(format!("SELECT count() AS `s0`, toStartOfInterval(Timestamp, INTERVAL 1 MINUTE) AS `__bucket`, ServiceName \
                    FROM default.otel_traces WHERE {W} AND (StatusCode = 'Error') GROUP BY `__bucket`, ServiceName LIMIT 1000")))]
    #[case::positive_quantile_and_sum(
        tile(json!({ "select": [
            { "aggFn": "quantile", "level": 0.95, "valueExpression": "Duration / 1e6", "aggCondition": "" },
            { "aggFn": "sum", "valueExpression": "toFloat64OrZero(SpanAttributes['n'])" } ] })),
        Ok(format!("SELECT quantile(0.95)(Duration / 1e6) AS `s0`, sum(toFloat64OrZero(SpanAttributes['n'])) AS `s1` \
                    FROM default.otel_traces WHERE {W} LIMIT 1000")))]
    #[case::corner_agg_condition(
        tile(json!({ "select": [
            { "aggFn": "count", "aggCondition": "StatusCode = 'Error'" },
            { "aggFn": "quantile", "level": 0.5, "valueExpression": "Duration", "aggCondition": "SpanKind = 'x'" } ] })),
        Ok(format!("SELECT countIf(StatusCode = 'Error') AS `s0`, quantileIf(0.5)(Duration, SpanKind = 'x') AS `s1` \
                    FROM default.otel_traces WHERE {W} LIMIT 1000")))]
    #[case::negative_unknown_agg(tile(json!({ "select": [ { "aggFn": "last_value", "valueExpression": "x" } ] })), Err("not one this check renders"))]
    #[case::negative_sum_without_expr(tile(json!({ "select": [ { "aggFn": "sum" } ] })), Err("needs a valueExpression"))]
    #[case::negative_empty_select(tile(json!({ "select": [] })), Err("select is not a list"))]
    #[case::negative_log_source(tile(json!({ "source": "$log" })), Err("not \"$trace\""))]
    #[case::negative_lucene_where(tile(json!({ "whereLanguage": "lucene" })), Err("not \"sql\""))]
    #[case::boundary_level_one(tile(json!({ "select": [ { "aggFn": "quantile", "level": 1.0, "valueExpression": "x" } ] })), Err("not in (0, 1)"))]
    #[case::boundary_level_zero(tile(json!({ "select": [ { "aggFn": "quantile", "level": 0.0, "valueExpression": "x" } ] })), Err("not in (0, 1)"))]
    #[case::adversarial_second_statement(tile(json!({ "where": "1; DROP TABLE agent.agent_events" })), Err("`;` or a NUL"))]
    #[case::adversarial_statement_in_group_by(tile(json!({ "groupBy": "a; SELECT 1" })), Err("`;` or a NUL"))]
    #[case::adversarial_statement_in_value(tile(json!({ "select": [ { "aggFn": "sum", "valueExpression": "1); SELECT (1" } ] })), Err("`;` or a NUL"))]
    #[case::adversarial_nul(tile(json!({ "where": "a\u{0}b" })), Err("`;` or a NUL"))]
    #[case::adversarial_huge_part(tile(json!({ "where": "1".repeat(MAX_PART_CHARS + 1) })), Err("over 4096"))]
    #[case::adversarial_where_not_a_string(tile(json!({ "where": { "sql": "1" } })), Err("not a string"))]
    #[case::adversarial_level_nan_as_string(tile(json!({ "select": [ { "aggFn": "quantile", "level": "NaN", "valueExpression": "x" } ] })), Err("not in (0, 1)"))]
    fn render_tile_cases(#[case] cfg: Value, #[case] want: std::result::Result<String, &str>) {
        match (render_tile(&cfg, &trace(), 3600), want) {
            (Ok(got), Ok(want)) => assert_eq!(got, want),
            (Err(e), Err(frag)) => assert!(e.to_string().contains(frag), "{e}"),
            (got, want) => panic!("got {got:?}, want {want:?}"),
        }
    }

    #[rstest]
    #[case::positive(json!({ "source": "$trace", "where": "SpanName = 'x'" }),
        Ok(format!("SELECT count() AS `s0` FROM default.otel_traces WHERE {W} AND (SpanName = 'x')")))]
    #[case::corner_no_where(json!({ "source": "$trace" }),
        Ok(format!("SELECT count() AS `s0` FROM default.otel_traces WHERE {W}")))]
    #[case::negative_no_source(json!({ "where": "1" }), Err("no source"))]
    #[case::adversarial_second_statement(json!({ "source": "$trace", "where": "1;SELECT 2" }), Err("`;`"))]
    fn render_search_cases(#[case] s: Value, #[case] want: std::result::Result<String, &str>) {
        match (render_search(&s, &trace(), 3600), want) {
            (Ok(got), Ok(want)) => assert_eq!(got, want),
            (Err(e), Err(frag)) => assert!(e.to_string().contains(frag), "{e}"),
            (got, want) => panic!("got {got:?}, want {want:?}"),
        }
    }

    fn template(tiles: Vec<Value>, searches: Vec<Value>) -> Value {
        json!({ "dashboards": [ { "name": "D", "tiles": tiles.into_iter()
                    .map(|c| json!({ "id": "d-0", "x": 0, "y": 0, "w": 12, "h": 8, "config": c }))
                    .collect::<Vec<_>>() } ],
                "savedSearches": searches })
    }

    #[rstest]
    #[case::positive(template(vec![tile(json!({}))], vec![json!({ "name": "s", "source": "$trace" })]), Ok(2))]
    #[case::negative_empty(template(vec![], vec![]), Err("no tiles or saved searches"))]
    #[case::negative_no_lists(json!({}), Err("no tiles or saved searches"))]
    #[case::corner_error_names_the_tile(template(vec![tile(json!({ "name": "Bad", "source": "$x" }))], vec![]), Err("D / Bad: "))]
    #[case::adversarial_dashboards_not_a_list(json!({ "dashboards": "x" }), Err("not a list"))]
    #[case::adversarial_too_many(template(vec![tile(json!({})); MAX_ITEMS + 1], vec![]), Err("more than 500"))]
    fn tile_queries_cases(#[case] t: Value, #[case] want: std::result::Result<usize, &str>) {
        match (tile_queries(&t, &trace(), 3600), want) {
            (Ok(qs), Ok(n)) => assert_eq!(qs.len(), n),
            (Err(e), Err(frag)) => assert!(e.to_string().contains(frag), "{e}"),
            (got, want) => panic!("got {:?}, want {want:?}", got.map(|q| q.len())),
        }
    }

    #[rstest]
    #[case::adversarial_table_injection("default.otel_traces; DROP", "Timestamp", "not db.table")]
    #[case::adversarial_timestamp_injection(
        "default.otel_traces",
        "Timestamp) OR (1",
        "not a column"
    )]
    #[case::negative_empty_timestamp("default.otel_traces", "", "not a column")]
    fn source_refusals(#[case] table: &str, #[case] ts: &str, #[case] frag: &str) {
        let src = Source {
            table: table.into(),
            timestamp: ts.into(),
        };
        let e = tile_queries(&template(vec![tile(json!({}))], vec![]), &src, 60).unwrap_err();
        assert!(e.to_string().contains(frag), "{e}");
    }

    #[rstest]
    #[case::positive_rows(200, r#"{"meta":[],"data":[{"s0":"3","ServiceName":"a"},{"s0":"1","ServiceName":"b"}],"rows":2}"#, Outcome::Data(2))]
    #[case::corner_zero_count_is_empty(200, r#"{"data":[{"s0":"0"}],"rows":1}"#, Outcome::Empty)]
    #[case::corner_nan_quantile_is_empty(
        200,
        r#"{"data":[{"s0":"nan","s1":null}],"rows":1}"#,
        Outcome::Empty
    )]
    #[case::corner_bucket_alone_is_not_data(
        200,
        r#"{"data":[{"__bucket":"2026-11-12 00:00:00","s0":0}]}"#,
        Outcome::Empty
    )]
    #[case::boundary_no_rows(200, r#"{"data":[],"rows":0}"#, Outcome::Empty)]
    #[case::positive_float(200, r#"{"data":[{"s0":12.5}]}"#, Outcome::Data(1))]
    #[case::negative_unknown_column(404,
        "Code: 47. DB::Exception: Missing columns: 'Nope' while processing query. (UNKNOWN_IDENTIFIER)\n",
        Outcome::Failed("HTTP 404: Code: 47. DB::Exception: Missing columns: 'Nope' while processing query. (UNKNOWN_IDENTIFIER)".into()))]
    #[case::negative_auth(401, "Code: 516. DB::Exception: agent_viewer: Authentication failed", Outcome::Failed("HTTP 401: Code: 516. DB::Exception: agent_viewer: Authentication failed".into()))]
    #[case::adversarial_not_json(200, "<html>proxy</html>", Outcome::Failed("HTTP 200: a reply that is not JSON".into()))]
    #[case::adversarial_no_data(200, r#"{"rows":5}"#, Outcome::Failed("HTTP 200: a reply with no data list".into()))]
    #[case::adversarial_huge_error_is_bounded(500, &"e".repeat(10_000), Outcome::Failed(format!("HTTP 500: {}", "e".repeat(300))))]
    fn classify_reply_cases(#[case] status: u16, #[case] body: &str, #[case] want: Outcome) {
        assert_eq!(classify_reply(status, body.as_bytes()), want);
    }

    #[rstest]
    #[case::positive_hours("now-6h", Some(21_600))]
    #[case::positive_minutes("now-15m", Some(900))]
    #[case::positive_weeks("now-1w", Some(604_800))]
    #[case::negative_absolute("2026-11-12", None)]
    #[case::negative_unknown_unit("now-6y", None)]
    #[case::boundary_zero("now-0h", None)]
    #[case::boundary_overflow("now-99999999999999999w", None)]
    #[case::adversarial_no_number("now-h", None)]
    #[case::adversarial_sign("now--1h", None)]
    #[case::adversarial_non_ascii_unit("now-1é", None)]
    fn relative_secs_cases(#[case] s: &str, #[case] want: Option<u64>) {
        assert_eq!(relative_secs(s), want);
    }
}
