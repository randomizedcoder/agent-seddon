//! The pure core of `obs-check`: which queries a Grafana dashboard holds, filled in as the
//! browser fills them, sent to Grafana's `/api/ds/query`, and what each reply says.
//!
//! The browser interpolates a dashboard's own variables (`$job`, `${prometheus}`) before
//! it asks Grafana for data; Grafana's macros (`$__rate_interval`, `$__timeFilter(…)`)
//! are expanded by Grafana and its data-source plugins, so they are sent as they are.
//! That makes a query that passes here one that would render in a panel.
//!
//! Dashboards are the repo's own, but a reply is the server's: every reply is parsed
//! fail-closed (a missing result is an error, never "ok"), and sizes and counts are capped.

use serde_json::{json, Map, Value};

pub mod hyperdx;
pub mod prometheus;

/// Largest reply read from Grafana.
pub const MAX_BODY_BYTES: usize = 8 << 20;
/// Largest dashboard file read.
pub const MAX_DASHBOARD_BYTES: u64 = 4 << 20;
/// Most queries one run sends, over every dashboard (and each dashboard alone).
pub const MAX_QUERIES: usize = 2000;
/// Deepest panel nesting followed (a collapsed row holds panels; nothing nests deeper).
const MAX_DEPTH: usize = 4;
/// Longest error message kept from a reply.
const MAX_ERROR_CHARS: usize = 300;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

/// A data source as `GET /api/datasources` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataSource {
    pub uid: String,
    pub kind: String,
    pub is_default: bool,
}

/// A data-source uid as Grafana issues one: letters, digits, `-` and `_`, at most 40.
pub fn valid_uid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parse `GET /api/datasources`. A source with a malformed uid or type fails the reply.
pub fn parse_datasources(v: &Value) -> Result<Vec<DataSource>> {
    let items = v
        .as_array()
        .ok_or_else(|| invalid("datasources: not an array"))?;
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let o = item
                .as_object()
                .ok_or_else(|| invalid(format!("datasources[{i}]: not an object")))?;
            let field = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or_default();
            let (uid, kind) = (field("uid"), field("type"));
            if !valid_uid(uid) || !valid_uid(kind) {
                return Err(invalid(format!("datasources[{i}]: malformed uid or type")));
            }
            Ok(DataSource {
                uid: uid.to_string(),
                kind: kind.to_string(),
                is_default: o.get("isDefault").and_then(Value::as_bool) == Some(true),
            })
        })
        .collect()
}

/// The source a `${var}` of `kind` resolves to: the default of that type, else the first.
fn source_of_kind<'a>(sources: &'a [DataSource], kind: &str) -> Option<&'a DataSource> {
    let mut of_kind = sources.iter().filter(|s| s.kind == kind);
    let first = of_kind.clone().next();
    of_kind.find(|s| s.is_default).or(first)
}

/// A dashboard's variables, resolved as the browser would on first load.
#[derive(Debug, Default)]
pub struct Vars {
    /// `(name, value)`, longest name first so `$jobs` never matches `$job`'s prefix.
    values: Vec<(String, String)>,
    /// Data-source variables: name → (uid, type).
    sources: Vec<(String, DataSource)>,
}

impl Vars {
    pub fn from_dashboard(dash: &Value, sources: &[DataSource]) -> Result<Vars> {
        let mut vars = Vars::default();
        let list = dash
            .pointer("/templating/list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for v in list {
            let name = v.get("name").and_then(Value::as_str).unwrap_or_default();
            if !is_var_name(name) {
                return Err(invalid(format!("variable {name:?}: not a name")));
            }
            let kind = v.get("type").and_then(Value::as_str).unwrap_or_default();
            if kind == "datasource" {
                let plugin = v.get("query").and_then(Value::as_str).unwrap_or_default();
                let ds = source_of_kind(sources, plugin).ok_or_else(|| {
                    invalid(format!(
                        "variable {name}: no data source of type {plugin:?}"
                    ))
                })?;
                vars.values.push((name.to_string(), ds.uid.clone()));
                vars.sources.push((name.to_string(), ds.clone()));
            } else {
                vars.values.push((name.to_string(), var_value(v)));
            }
        }
        vars.values
            .sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        Ok(vars)
    }

    /// The data source a target names: `{uid, type}`, a `${var}` uid, or the dashboard's
    /// data-source variable of that type.
    fn source(&self, ds: &Value, sources: &[DataSource]) -> Result<DataSource> {
        let uid = ds.get("uid").and_then(Value::as_str).unwrap_or_default();
        let kind = ds.get("type").and_then(Value::as_str).unwrap_or_default();
        if uid.is_empty() {
            // No data source named: Grafana uses its default.
            return sources
                .iter()
                .find(|s| s.is_default)
                .cloned()
                .ok_or_else(|| invalid("no data source named, and none is the default"));
        }
        let var = uid
            .strip_prefix("${")
            .and_then(|u| u.strip_suffix('}'))
            .or_else(|| uid.strip_prefix('$'));
        if let Some(name) = var {
            return self
                .sources
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, d)| d.clone())
                .ok_or_else(|| invalid(format!("data source variable {name:?} is not defined")));
        }
        if let Some(d) = sources.iter().find(|s| s.uid == uid) {
            return Ok(d.clone());
        }
        Err(invalid(format!(
            "data source {uid:?} ({kind}) is not provisioned"
        )))
    }
}

fn is_var_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with("__")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A query or custom variable's first-load value: the saved current value (`All` is the
/// variable's `allValue`, else `.*`), else its first option.
fn var_value(v: &Value) -> String {
    let all = || {
        v.get("allValue")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(".*")
            .to_string()
    };
    let current = v.pointer("/current/value");
    let picked = match current {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(a)) => a.first().and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    match picked {
        Some(s) if s == "$__all" => all(),
        Some(s) => s,
        None if v.get("includeAll").and_then(Value::as_bool) == Some(true) => all(),
        None => v
            .pointer("/options/0/value")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                v.get("query")
                    .and_then(Value::as_str)
                    .and_then(|q| q.split(',').next())
                    .map(|s| s.trim().to_string())
            })
            .unwrap_or_default(),
    }
}

/// `s` with the dashboard's variables filled in, in one pass: `${name}`, `${name:format}`,
/// `[[name]]` and `$name` (not followed by a word character). A value is never expanded
/// again, and names Grafana owns (`$__…`) and unknown names are left as they are.
pub fn interpolate(s: &str, vars: &Vars) -> String {
    let lookup = |name: &str| {
        vars.values
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(['$', '[']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // `${name}` / `${name:format}`
        if let Some(body) = rest.strip_prefix("${") {
            if let Some(end) = body.find('}') {
                let name = body[..end].split(':').next().unwrap_or_default();
                if let Some(v) = lookup(name) {
                    out.push_str(v);
                    rest = &body[end + 1..];
                    continue;
                }
            }
        } else if let Some(body) = rest.strip_prefix("[[") {
            if let Some(end) = body.find("]]") {
                if let Some(v) = lookup(&body[..end]) {
                    out.push_str(v);
                    rest = &body[end + 2..];
                    continue;
                }
            }
        } else if let Some(body) = rest.strip_prefix('$') {
            let end = body
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(body.len());
            if let Some(v) = lookup(&body[..end]) {
                out.push_str(v);
                rest = &body[end..];
                continue;
            }
        }
        // Not a variable: keep the character and move on.
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// One panel query, ready to send.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelQuery {
    pub dashboard: String,
    pub panel: String,
    pub ref_id: String,
    pub source: DataSource,
    /// The target with the dashboard's variables filled in and its data source resolved.
    pub target: Value,
}

/// The fields of a target that carry query text.
const TEXT_FIELDS: [&str; 3] = ["expr", "rawSql", "query"];

/// Every query in `dash`, in panel order, rows' panels included.
pub fn panel_queries(dash: &Value, sources: &[DataSource]) -> Result<Vec<PanelQuery>> {
    let vars = Vars::from_dashboard(dash, sources)?;
    let uid = dash.get("uid").and_then(Value::as_str).unwrap_or("?");
    let mut out = Vec::new();
    let panels = dash.get("panels").and_then(Value::as_array);
    walk(panels, uid, &vars, sources, 0, &mut out)?;
    Ok(out)
}

fn walk(
    panels: Option<&Vec<Value>>,
    uid: &str,
    vars: &Vars,
    sources: &[DataSource],
    depth: usize,
    out: &mut Vec<PanelQuery>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(invalid(format!(
            "{uid}: panels nest deeper than {MAX_DEPTH}"
        )));
    }
    for p in panels.map(Vec::as_slice).unwrap_or_default() {
        walk(
            p.get("panels").and_then(Value::as_array),
            uid,
            vars,
            sources,
            depth + 1,
            out,
        )?;
        let title = p.get("title").and_then(Value::as_str).unwrap_or("?");
        for t in p
            .get("targets")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let Some(obj) = t.as_object() else {
                return Err(invalid(format!(
                    "{uid} / {title}: a target is not an object"
                )));
            };
            let ds = obj
                .get("datasource")
                .or_else(|| p.get("datasource"))
                .cloned()
                .unwrap_or(Value::Null);
            let source = vars
                .source(&ds, sources)
                .map_err(|Error::Invalid(e)| invalid(format!("{uid} / {title}: {e}")))?;
            let mut target: Map<String, Value> = obj.clone();
            for f in TEXT_FIELDS {
                if let Some(Value::String(s)) = target.get(f) {
                    let filled = interpolate(s, vars);
                    target.insert(f.to_string(), Value::String(filled));
                }
            }
            target.insert(
                "datasource".into(),
                json!({ "uid": source.uid, "type": source.kind }),
            );
            if out.len() >= MAX_QUERIES {
                return Err(invalid(format!("more than {MAX_QUERIES} queries")));
            }
            out.push(PanelQuery {
                dashboard: uid.to_string(),
                panel: title.to_string(),
                ref_id: obj
                    .get("refId")
                    .and_then(Value::as_str)
                    .unwrap_or("A")
                    .to_string(),
                source,
                target: Value::Object(target),
            });
        }
    }
    Ok(())
}

/// Every query of every dashboard (`(name, dashboard)`, in order). At least one query
/// must be there, and at most [`MAX_QUERIES`] in all: a run that checks nothing (an
/// empty or mis-built dashboards directory) is a failure, not a pass.
pub fn all_queries(
    dashboards: &[(String, Value)],
    sources: &[DataSource],
) -> Result<Vec<PanelQuery>> {
    let mut all = Vec::new();
    for (name, d) in dashboards {
        let qs = panel_queries(d, sources)
            .map_err(|Error::Invalid(e)| invalid(format!("{name}: {e}")))?;
        if all.len() + qs.len() > MAX_QUERIES {
            return Err(invalid(format!("more than {MAX_QUERIES} queries in all")));
        }
        all.extend(qs);
    }
    if all.is_empty() {
        return Err(invalid(format!(
            "no panel queries in {} dashboard(s)",
            dashboards.len()
        )));
    }
    Ok(all)
}

/// The `/api/ds/query` body for one query over `[from, to]` (Grafana time strings such
/// as `now-24h`).
pub fn query_body(q: &PanelQuery, from: &str, to: &str) -> Value {
    let mut target = q.target.clone();
    if let Value::Object(o) = &mut target {
        o.insert("refId".into(), Value::String(q.ref_id.clone()));
        o.entry("intervalMs").or_insert(json!(60_000));
        o.entry("maxDataPoints").or_insert(json!(500));
    }
    json!({ "from": from, "to": to, "queries": [target] })
}

/// A `--skip-table` name: `db.table`, each part `[A-Za-z0-9_]`, at most 128 bytes.
pub fn valid_table(s: &str) -> bool {
    let ident =
        |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    s.len() <= 128
        && s.split_once('.')
            .is_some_and(|(db, t)| ident(db) && ident(t))
}

/// Whether any of `q`'s query text names `table` (`db.table`) as a whole identifier,
/// case-insensitively: `default.otel_traces` matches, `default.otel_traces_1m` and
/// `xdefault.otel_traces` do not.
pub fn names_table(q: &PanelQuery, table: &str) -> bool {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.';
    let table = table.to_ascii_lowercase();
    TEXT_FIELDS.iter().any(|f| {
        let Some(text) = q.target.get(*f).and_then(Value::as_str) else {
            return false;
        };
        let text = text.to_ascii_lowercase();
        let (hay, n) = (text.as_bytes(), table.len());
        text.match_indices(&table).any(|(i, _)| {
            (i == 0 || !is_ident(hay[i - 1])) && hay.get(i + n).is_none_or(|b| !is_ident(*b))
        })
    })
}

/// What one query's reply says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Rows (or points) came back.
    Data(usize),
    /// The query ran and matched nothing.
    Empty,
    /// The query failed; the message is Grafana's, bounded.
    Failed(String),
}

/// `s` cut to [`MAX_ERROR_CHARS`], control characters as spaces, so a server's text
/// prints as one bounded line and can't forge another (`\nFAILED …`).
pub(crate) fn bounded(s: &str) -> String {
    s.chars()
        .take(MAX_ERROR_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The verdict for `ref_id` from an `/api/ds/query` reply with HTTP `status`. Anything
/// that is not a well-formed result for that ref is a failure, never "ok".
pub fn classify(ref_id: &str, status: u16, body: &Value) -> Outcome {
    let Some(result) = body.pointer("/results").and_then(|r| r.get(ref_id)) else {
        let msg = body
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| format!("HTTP {status}, no result for {ref_id}"), bounded);
        return Outcome::Failed(msg);
    };
    if let Some(e) = result.get("error").and_then(Value::as_str) {
        if !e.is_empty() {
            return Outcome::Failed(bounded(e));
        }
    }
    // An absent status is Grafana's "ok"; one that is there but not a number is not a
    // well-formed result.
    let inner = match result.get("status") {
        None => 200,
        Some(v) => match v.as_u64() {
            Some(n) => n,
            None => return Outcome::Failed(format!("HTTP {status}, a result status of {v}")),
        },
    };
    if !(200..300).contains(&status) || !(200..300).contains(&inner) {
        return Outcome::Failed(format!("HTTP {status} (result {inner})"));
    }
    let Some(frames) = result.get("frames").and_then(Value::as_array) else {
        return Outcome::Empty;
    };
    let rows: usize = frames
        .iter()
        .map(|f| {
            f.pointer("/data/values")
                .and_then(Value::as_array)
                .and_then(|cols| cols.first())
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        })
        .sum();
    if rows == 0 {
        Outcome::Empty
    } else {
        Outcome::Data(rows)
    }
}

#[cfg(test)]
mod tests;
