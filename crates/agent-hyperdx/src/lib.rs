//! `agent-hyperdx` — provisions the agent-seddon HyperDX dashboards and saved searches.
//!
//! The dashboards are data (`nix/hyperdx/dashboards.nix`). HyperDX seeds each team's
//! sources with random ids, so a template names its source by kind (`"source":
//! "$trace"`), and [`resolve`] swaps in the team's real id. [`plan`] then decides, per
//! item, whether to create it, update the copy we made before (matched by name and the
//! [`MANAGED_TAG`] tag), or leave a same-named item a person made alone. Every write goes
//! through HyperDX's own API, which validates it against HyperDX's schema.
//!
//! The API's replies are untrusted input: an id becomes a URL path segment, so it must
//! be a 24-hex ObjectId; arrays, bodies and nesting are capped; anything malformed fails
//! closed rather than being guessed at.

use async_trait::async_trait;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The tag on every item this tool owns. A same-named item without it is a person's.
pub const MANAGED_TAG: &str = "agent-seddon";
/// Largest reply or template file read, in bytes.
pub const MAX_BODY_BYTES: usize = 4 << 20;
/// Most items accepted in one list (a reply or a template file).
pub const MAX_ITEMS: usize = 500;
/// Deepest JSON nesting walked by [`resolve`].
pub const MAX_DEPTH: usize = 32;
/// Longest item name accepted.
pub const MAX_NAME_CHARS: usize = 200;

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("the team has no {0:?} source yet")]
    NoSource(String),
    #[error("the team has {count} {kind:?} sources; name one as \"${kind}:<name>\"")]
    AmbiguousSource { kind: String, count: usize },
    #[error("HTTP {status} from {path}")]
    Http { status: u16, path: String },
    #[error("{0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}

/// A Mongo ObjectId as HyperDX prints it: exactly 24 hex digits.
pub fn is_object_id(s: &str) -> bool {
    s.len() == 24 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The two kinds of item this tool writes, and their API collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collection {
    Dashboards,
    SavedSearches,
}

impl Collection {
    pub const ALL: [Collection; 2] = [Collection::Dashboards, Collection::SavedSearches];

    pub fn path(self) -> &'static str {
        match self {
            Collection::Dashboards => "/dashboards",
            Collection::SavedSearches => "/saved-search",
        }
    }

    /// The key in the template file.
    pub fn key(self) -> &'static str {
        match self {
            Collection::Dashboards => "dashboards",
            Collection::SavedSearches => "savedSearches",
        }
    }
}

/// The team's sources, by kind, from `GET /sources`.
#[derive(Debug, Default)]
pub struct Sources {
    by_kind: BTreeMap<String, Vec<(String, String)>>,
}

impl Sources {
    /// `kind` alone needs exactly one source of that kind; `kind:name` picks by name.
    pub fn id_for(&self, spec: &str) -> Result<&str> {
        let (kind, name) = match spec.split_once(':') {
            Some((k, n)) => (k, Some(n)),
            None => (spec, None),
        };
        let all = self.by_kind.get(kind).map(Vec::as_slice).unwrap_or(&[]);
        let hits: Vec<&(String, String)> = match name {
            Some(n) => all.iter().filter(|(nm, _)| nm == n).collect(),
            None => all.iter().collect(),
        };
        match hits.as_slice() {
            [] => Err(Error::NoSource(spec.to_string())),
            [(_, id)] => Ok(id),
            many => Err(Error::AmbiguousSource {
                kind: kind.to_string(),
                count: many.len(),
            }),
        }
    }
}

fn array<'a>(v: &'a Value, what: &str) -> Result<&'a Vec<Value>> {
    let a = v
        .as_array()
        .ok_or_else(|| invalid(format!("{what}: expected a JSON array")))?;
    if a.len() > MAX_ITEMS {
        return Err(invalid(format!(
            "{what}: {} items is more than {MAX_ITEMS}",
            a.len()
        )));
    }
    Ok(a)
}

fn str_field<'a>(o: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a str> {
    o.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{what}: missing string {key:?}")))
}

fn item_id<'a>(o: &'a Map<String, Value>, what: &str) -> Result<&'a str> {
    let id = o
        .get("id")
        .or_else(|| o.get("_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("{what}: missing id")))?;
    if !is_object_id(id) {
        return Err(invalid(format!("{what}: id is not an ObjectId")));
    }
    Ok(id)
}

/// Parse `GET /sources`. A source with a bad id fails the whole reply: its id would be
/// written into dashboards.
pub fn parse_sources(v: &Value) -> Result<Sources> {
    let mut s = Sources::default();
    for (i, item) in array(v, "sources")?.iter().enumerate() {
        let what = format!("sources[{i}]");
        let o = item
            .as_object()
            .ok_or_else(|| invalid(format!("{what}: expected an object")))?;
        let id = item_id(o, &what)?;
        let kind = str_field(o, "kind", &what)?;
        let name = o.get("name").and_then(Value::as_str).unwrap_or_default();
        s.by_kind
            .entry(kind.to_string())
            .or_default()
            .push((name.to_string(), id.to_string()));
    }
    Ok(s)
}

/// Replace every `"source": "$<spec>"` in `template` with that source's id. Only the
/// `source` key is touched; any other `$` string is left as it is.
pub fn resolve(template: &Value, sources: &Sources) -> Result<Value> {
    fn walk(v: &Value, key: Option<&str>, s: &Sources, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(invalid(format!("template nests deeper than {MAX_DEPTH}")));
        }
        Ok(match v {
            Value::String(t) if key == Some("source") => match t.strip_prefix('$') {
                Some(spec) => {
                    let ok = !spec.is_empty()
                        && spec.len() <= MAX_NAME_CHARS
                        && spec
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b" _-:.".contains(&b));
                    if !ok {
                        return Err(invalid(format!("bad source placeholder {t:?}")));
                    }
                    Value::String(s.id_for(spec)?.to_string())
                }
                None => v.clone(),
            },
            Value::Array(a) => Value::Array(
                a.iter()
                    .map(|x| walk(x, None, s, depth + 1))
                    .collect::<Result<_>>()?,
            ),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, x)| Ok((k.clone(), walk(x, Some(k), s, depth + 1)?)))
                    .collect::<Result<_>>()?,
            ),
            _ => v.clone(),
        })
    }
    walk(template, None, sources, 0)
}

/// Every string under a `source` key, after [`resolve`] (`--check` asserts each is an
/// ObjectId: a template that spells a source some other way is caught offline).
pub fn sources_in(v: &Value) -> Vec<String> {
    match v {
        Value::Object(o) => o
            .iter()
            .flat_map(|(k, x)| match (k.as_str(), x) {
                ("source", Value::String(s)) => vec![s.clone()],
                _ => sources_in(x),
            })
            .collect(),
        Value::Array(a) => a.iter().flat_map(sources_in).collect(),
        _ => Vec::new(),
    }
}

/// An item already in HyperDX.
#[derive(Debug, Clone, PartialEq)]
pub struct Existing {
    pub id: String,
    pub name: String,
    pub managed: bool,
}

/// Parse `GET /dashboards` or `GET /saved-search`.
pub fn parse_existing(v: &Value, what: &str) -> Result<Vec<Existing>> {
    array(v, what)?
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let what = format!("{what}[{i}]");
            let o = item
                .as_object()
                .ok_or_else(|| invalid(format!("{what}: expected an object")))?;
            let managed = o
                .get("tags")
                .and_then(Value::as_array)
                .is_some_and(|t| t.iter().any(|x| x.as_str() == Some(MANAGED_TAG)));
            Ok(Existing {
                id: item_id(o, &what)?.to_string(),
                name: str_field(o, "name", &what)?.to_string(),
                managed,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Create {
        name: String,
        body: Value,
    },
    Update {
        id: String,
        name: String,
        body: Value,
        /// More tagged items with this name: only `id` is updated.
        duplicates: usize,
    },
    /// A same-named item exists without [`MANAGED_TAG`]: a person's, so left alone.
    SkipUserOwned {
        name: String,
    },
}

/// What to do with each desired item, given what is there. Desired items must be
/// objects with a unique, non-empty `name` and the [`MANAGED_TAG`] tag.
pub fn plan(desired: &[Value], existing: &[Existing]) -> Result<Vec<Action>> {
    let mut seen = BTreeSet::new();
    desired
        .iter()
        .enumerate()
        .map(|(i, body)| {
            let what = format!("template[{i}]");
            let o = body
                .as_object()
                .ok_or_else(|| invalid(format!("{what}: expected an object")))?;
            let name = str_field(o, "name", &what)?;
            if name.trim().is_empty() || name.chars().count() > MAX_NAME_CHARS {
                return Err(invalid(format!("{what}: name is empty or too long")));
            }
            if !seen.insert(name) {
                return Err(invalid(format!("{what}: duplicate name {name:?}")));
            }
            let tagged = o
                .get("tags")
                .and_then(Value::as_array)
                .is_some_and(|t| t.iter().any(|x| x.as_str() == Some(MANAGED_TAG)));
            if !tagged {
                return Err(invalid(format!("{what}: missing the {MANAGED_TAG:?} tag")));
            }
            let same: Vec<&Existing> = existing.iter().filter(|e| e.name == name).collect();
            let name = name.to_string();
            Ok(match same.iter().find(|e| e.managed) {
                Some(e) => Action::Update {
                    id: e.id.clone(),
                    name,
                    body: body.clone(),
                    duplicates: same.iter().filter(|e| e.managed).count() - 1,
                },
                None if !same.is_empty() => Action::SkipUserOwned { name },
                None => Action::Create {
                    name,
                    body: body.clone(),
                },
            })
        })
        .collect()
}

/// Columns in HyperDX's dashboard grid.
pub const GRID_COLS: u64 = 24;

/// A dashboard template's tiles fit the grid: each has a unique id and a positive
/// size inside the 24 columns, and no two overlap. HyperDX accepts overlapping
/// tiles and then shuffles them, so this is where a layout slip is caught.
pub fn check_layout(dashboard: &Value) -> Result<()> {
    let name = dashboard
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("dashboard");
    let tiles = array(
        dashboard.get("tiles").unwrap_or(&Value::Null),
        &format!("{name}: tiles"),
    )?;
    let mut ids = BTreeSet::new();
    let mut boxes: Vec<(&str, [u64; 4])> = Vec::new();
    for (i, t) in tiles.iter().enumerate() {
        let what = format!("{name}: tiles[{i}]");
        let o = t
            .as_object()
            .ok_or_else(|| invalid(format!("{what}: expected an object")))?;
        let id = str_field(o, "id", &what)?;
        if !ids.insert(id) {
            return Err(invalid(format!("{what}: duplicate tile id {id:?}")));
        }
        let n = |k: &str| {
            o.get(k)
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid(format!("{what}: {k} is not a whole number")))
        };
        let (x, y, w, h) = (n("x")?, n("y")?, n("w")?, n("h")?);
        if w == 0 || h == 0 || x.checked_add(w).is_none_or(|r| r > GRID_COLS) {
            return Err(invalid(format!(
                "{what}: x={x} w={w} h={h} is outside the {GRID_COLS}-column grid"
            )));
        }
        let b = [x, y, x + w, y.saturating_add(h)];
        if let Some((other, _)) = boxes
            .iter()
            .find(|(_, c)| b[0] < c[2] && c[0] < b[2] && b[1] < c[3] && c[1] < b[3])
        {
            return Err(invalid(format!("{what}: {id:?} overlaps {other:?}")));
        }
        boxes.push((id, b));
    }
    Ok(())
}

/// The template file: `{ "dashboards": [...], "savedSearches": [...] }`.
#[derive(Debug, Default)]
pub struct Desired {
    pub dashboards: Vec<Value>,
    pub saved_searches: Vec<Value>,
}

impl Desired {
    pub fn parse(bytes: &[u8]) -> Result<Desired> {
        if bytes.len() > MAX_BODY_BYTES {
            return Err(invalid(format!(
                "template file is over {MAX_BODY_BYTES} bytes"
            )));
        }
        let v: Value =
            serde_json::from_slice(bytes).map_err(|e| invalid(format!("template file: {e}")))?;
        let list = |c: Collection| -> Result<Vec<Value>> {
            match v.get(c.key()) {
                None => Ok(Vec::new()),
                Some(x) => Ok(array(x, c.key())?.clone()),
            }
        };
        Ok(Desired {
            dashboards: list(Collection::Dashboards)?,
            saved_searches: list(Collection::SavedSearches)?,
        })
    }

    pub fn items(&self, c: Collection) -> &[Value] {
        match c {
            Collection::Dashboards => &self.dashboards,
            Collection::SavedSearches => &self.saved_searches,
        }
    }
}

/// The HyperDX API, as this tool uses it. `path` is always one of ours plus, for
/// `patch`, a validated ObjectId.
#[async_trait]
pub trait Api {
    async fn get(&self, path: &str) -> Result<Value>;
    async fn post(&self, path: &str, body: &Value) -> Result<()>;
    async fn patch(&self, path: &str, body: &Value) -> Result<()>;
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub skipped: Vec<String>,
    /// Updated items that have more than one tagged copy (the others are left as they
    /// are; worth deleting).
    pub duplicates: Vec<String>,
}

/// Resolve, plan and apply every collection. Stops at the first failed write.
pub async fn provision(api: &(dyn Api + Sync), desired: &Desired) -> Result<Report> {
    let sources = parse_sources(&api.get("/sources").await?)?;
    let mut report = Report::default();
    for c in Collection::ALL {
        let items = desired.items(c);
        if items.is_empty() {
            continue;
        }
        let resolved = items
            .iter()
            .map(|t| resolve(t, &sources))
            .collect::<Result<Vec<_>>>()?;
        let existing = parse_existing(&api.get(c.path()).await?, c.key())?;
        for action in plan(&resolved, &existing)? {
            match action {
                Action::Create { name, body } => {
                    api.post(c.path(), &body).await?;
                    report.created.push(name);
                }
                Action::Update {
                    id,
                    name,
                    body,
                    duplicates,
                } => {
                    // Checked by parse_existing; checked again where it meets the URL.
                    if !is_object_id(&id) {
                        return Err(invalid("refusing a non-ObjectId id in a URL"));
                    }
                    api.patch(&format!("{}/{id}", c.path()), &body).await?;
                    if duplicates > 0 {
                        report.duplicates.push(name.clone());
                    }
                    report.updated.push(name);
                }
                Action::SkipUserOwned { name } => report.skipped.push(name),
            }
        }
    }
    Ok(report)
}

/// Whether a failed attempt is worth retrying: the API not up yet (transport, 5xx,
/// 429) or the team's sources not seeded yet.
pub fn retryable(e: &Error) -> bool {
    match e {
        Error::Transport(_) | Error::NoSource(_) => true,
        Error::Http { status, .. } => agent_retry::http::retryable_status(*status),
        Error::Invalid(_) | Error::AmbiguousSource { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use serde_json::json;
    use std::sync::Mutex;

    const TRACE: &str = "6aefc0cbc9518aefacfc0ca7";
    const LOG: &str = "6aefc0cbc9518aefacfc0ca5";
    const D1: &str = "0123456789abcdef01234567";

    fn sources() -> Sources {
        parse_sources(&json!([
            {"id": LOG, "kind": "log", "name": "Logs"},
            {"_id": TRACE, "kind": "trace", "name": "Traces"},
        ]))
        .unwrap()
    }

    #[rstest]
    #[case::positive_lower(D1, true)]
    #[case::positive_upper("0123456789ABCDEF01234567", true)]
    #[case::boundary_23("0123456789abcdef0123456", false)]
    #[case::boundary_25("0123456789abcdef012345678", false)]
    #[case::negative_empty("", false)]
    #[case::adversarial_traversal("../../../../dashboards/x", false)]
    #[case::adversarial_slash_padded("0123456789abcdef0123/567", false)]
    #[case::adversarial_query("0123456789abcdef0123456?", false)]
    #[case::adversarial_unicode_digits("０123456789abcdef0123456", false)]
    fn object_id_cases(#[case] id: &str, #[case] want: bool) {
        assert_eq!(is_object_id(id), want);
    }

    #[rstest]
    #[case::positive_by_kind("trace", Ok(TRACE))]
    #[case::positive_by_kind_and_name("log:Logs", Ok(LOG))]
    #[case::negative_unknown_kind("metric", Err(Error::NoSource("metric".into())))]
    #[case::negative_wrong_name("trace:Spans", Err(Error::NoSource("trace:Spans".into())))]
    #[case::corner_empty_name("trace:", Err(Error::NoSource("trace:".into())))]
    fn id_for_cases(#[case] spec: &str, #[case] want: Result<&str>) {
        assert_eq!(sources().id_for(spec), want);
    }

    #[test]
    fn corner_two_sources_of_a_kind_need_a_name() {
        let s = parse_sources(&json!([
            {"id": TRACE, "kind": "trace", "name": "Traces"},
            {"id": D1, "kind": "trace", "name": "Other"},
        ]))
        .unwrap();
        assert_eq!(
            s.id_for("trace"),
            Err(Error::AmbiguousSource {
                kind: "trace".into(),
                count: 2
            })
        );
        assert_eq!(s.id_for("trace:Other"), Ok(D1));
    }

    #[rstest]
    #[case::positive_empty(json!([]), true)]
    #[case::negative_not_array(json!({"sources": []}), false)]
    #[case::negative_no_kind(json!([{"id": TRACE}]), false)]
    #[case::negative_no_id(json!([{"kind": "trace"}]), false)]
    #[case::negative_item_not_object(json!(["x"]), false)]
    #[case::adversarial_id_traversal(json!([{"id": "../../x", "kind": "trace"}]), false)]
    #[case::adversarial_id_number(json!([{"id": 7, "kind": "trace"}]), false)]
    fn parse_sources_cases(#[case] v: Value, #[case] ok: bool) {
        assert_eq!(parse_sources(&v).is_ok(), ok);
    }

    #[test]
    fn adversarial_huge_source_list_is_refused() {
        let many: Vec<Value> = (0..=MAX_ITEMS)
            .map(|_| json!({"id": TRACE, "kind": "trace"}))
            .collect();
        assert!(matches!(
            parse_sources(&Value::Array(many)),
            Err(Error::Invalid(_))
        ));
    }

    #[rstest]
    #[case::positive_tile_source(
        json!({"tiles": [{"config": {"source": "$trace", "where": ""}}]}),
        Ok(json!({"tiles": [{"config": {"source": TRACE, "where": ""}}]}))
    )]
    #[case::positive_named(json!({"source": "$log:Logs"}), Ok(json!({"source": LOG})))]
    #[case::corner_literal_id_kept(json!({"source": D1}), Ok(json!({"source": D1})))]
    #[case::corner_dollar_elsewhere_kept(
        json!({"where": "$trace", "name": "$x"}),
        Ok(json!({"where": "$trace", "name": "$x"}))
    )]
    #[case::corner_source_not_string_kept(json!({"source": 3}), Ok(json!({"source": 3})))]
    #[case::negative_unknown_kind(json!({"source": "$metric"}), Err(Error::NoSource("metric".into())))]
    #[case::boundary_bare_dollar(json!({"source": "$"}), Err(Error::Invalid("bad source placeholder \"$\"".into())))]
    #[case::adversarial_injection(
        json!({"source": "$trace\"}, {\"x"}),
        Err(Error::Invalid("bad source placeholder \"$trace\\\"}, {\\\"x\"".into()))
    )]
    #[case::adversarial_newline(
        json!({"source": "$trace\n"}),
        Err(Error::Invalid("bad source placeholder \"$trace\\n\"".into()))
    )]
    fn resolve_cases(#[case] t: Value, #[case] want: Result<Value>) {
        assert_eq!(resolve(&t, &sources()), want);
    }

    #[rstest]
    #[case::boundary_at_cap(MAX_DEPTH, true)]
    #[case::adversarial_depth_bomb(MAX_DEPTH + 1, false)]
    fn resolve_depth_cases(#[case] depth: usize, #[case] ok: bool) {
        let mut v = json!(1);
        for _ in 0..depth {
            v = json!([v]);
        }
        assert_eq!(resolve(&v, &sources()).is_ok(), ok);
    }

    #[rstest]
    #[case::positive_managed(json!([{"id": D1, "name": "A", "tags": ["agent-seddon"]}]), Ok(vec![Existing { id: D1.into(), name: "A".into(), managed: true }]))]
    #[case::positive_user_owned(json!([{"_id": D1, "name": "A"}]), Ok(vec![Existing { id: D1.into(), name: "A".into(), managed: false }]))]
    #[case::corner_tags_not_array(json!([{"id": D1, "name": "A", "tags": "agent-seddon"}]), Ok(vec![Existing { id: D1.into(), name: "A".into(), managed: false }]))]
    #[case::negative_no_name(json!([{"id": D1}]), Err(Error::Invalid("d[0]: missing string \"name\"".into())))]
    #[case::adversarial_id_path(json!([{"id": "x/../../team", "name": "A"}]), Err(Error::Invalid("d[0]: id is not an ObjectId".into())))]
    fn parse_existing_cases(#[case] v: Value, #[case] want: Result<Vec<Existing>>) {
        assert_eq!(parse_existing(&v, "d"), want);
    }

    fn mine(name: &str) -> Value {
        json!({"name": name, "tags": [MANAGED_TAG]})
    }

    fn ex(id: &str, name: &str, managed: bool) -> Existing {
        Existing {
            id: id.into(),
            name: name.into(),
            managed,
        }
    }

    #[rstest]
    #[case::positive_create(vec![mine("A")], vec![], Ok(vec!["create A"]))]
    #[case::positive_update(vec![mine("A")], vec![ex(D1, "A", true)], Ok(vec!["update A"]))]
    #[case::positive_user_owned_left_alone(vec![mine("A")], vec![ex(D1, "A", false)], Ok(vec!["skip A"]))]
    #[case::corner_user_and_managed_same_name(vec![mine("A")], vec![ex(TRACE, "A", false), ex(D1, "A", true)], Ok(vec!["update A"]))]
    #[case::corner_others_untouched(vec![mine("A")], vec![ex(D1, "B", true)], Ok(vec!["create A"]))]
    #[case::corner_two_managed_copies(vec![mine("A")], vec![ex(D1, "A", true), ex(TRACE, "A", true)], Ok(vec!["update A +1"]))]
    #[case::negative_duplicate(vec![mine("A"), mine("A")], vec![], Err(()))]
    #[case::negative_untagged(vec![json!({"name": "A"})], vec![], Err(()))]
    #[case::negative_not_object(vec![json!("A")], vec![], Err(()))]
    #[case::boundary_blank_name(vec![mine("  ")], vec![], Err(()))]
    #[case::adversarial_long_name(vec![mine(&"x".repeat(MAX_NAME_CHARS + 1))], vec![], Err(()))]
    fn plan_cases(
        #[case] desired: Vec<Value>,
        #[case] existing: Vec<Existing>,
        #[case] want: std::result::Result<Vec<&str>, ()>,
    ) {
        let got = plan(&desired, &existing).map(|a| {
            a.iter()
                .map(|x| match x {
                    Action::Create { name, .. } => format!("create {name}"),
                    Action::Update {
                        id,
                        name,
                        duplicates,
                        ..
                    } => {
                        assert_eq!(id, D1);
                        match duplicates {
                            0 => format!("update {name}"),
                            n => format!("update {name} +{n}"),
                        }
                    }
                    Action::SkipUserOwned { name } => format!("skip {name}"),
                })
                .collect::<Vec<_>>()
        });
        match want {
            Ok(w) => assert_eq!(got.unwrap(), w),
            Err(()) => assert!(matches!(got, Err(Error::Invalid(_))), "{got:?}"),
        }
    }

    #[rstest]
    #[case::positive_both(br#"{"dashboards":[{"name":"A"}],"savedSearches":[]}"#.to_vec(), Ok((1, 0)))]
    #[case::corner_missing_keys(b"{}".to_vec(), Ok((0, 0)))]
    #[case::negative_not_json(b"dashboards".to_vec(), Err(()))]
    #[case::negative_list_not_array(br#"{"dashboards":{}}"#.to_vec(), Err(()))]
    #[case::adversarial_oversize(vec![b' '; MAX_BODY_BYTES + 1], Err(()))]
    fn desired_parse_cases(
        #[case] bytes: Vec<u8>,
        #[case] want: std::result::Result<(usize, usize), ()>,
    ) {
        let got = Desired::parse(&bytes).map(|d| (d.dashboards.len(), d.saved_searches.len()));
        match want {
            Ok(w) => assert_eq!(got.unwrap(), w),
            Err(()) => assert!(got.is_err()),
        }
    }

    fn t(id: &str, x: i64, y: i64, w: i64, h: i64) -> Value {
        json!({"id": id, "x": x, "y": y, "w": w, "h": h, "config": {}})
    }

    #[rstest]
    #[case::positive_side_by_side(vec![t("a", 0, 0, 12, 8), t("b", 12, 0, 12, 8)], true)]
    #[case::positive_stacked(vec![t("a", 0, 0, 24, 8), t("b", 0, 8, 24, 8)], true)]
    #[case::corner_no_tiles(vec![], true)]
    #[case::boundary_full_width(vec![t("a", 0, 0, 24, 1)], true)]
    #[case::boundary_one_past_the_edge(vec![t("a", 1, 0, 24, 8)], false)]
    #[case::negative_overlap(vec![t("a", 0, 0, 12, 8), t("b", 6, 4, 12, 8)], false)]
    #[case::negative_duplicate_id(vec![t("a", 0, 0, 12, 8), t("a", 12, 0, 12, 8)], false)]
    #[case::negative_zero_width(vec![t("a", 0, 0, 0, 8)], false)]
    #[case::negative_negative_x(vec![t("a", -1, 0, 12, 8)], false)]
    #[case::adversarial_overflowing_x(vec![json!({"id": "a", "x": u64::MAX, "y": 0, "w": 2, "h": 1})], false)]
    #[case::adversarial_fractional(vec![json!({"id": "a", "x": 0.5, "y": 0, "w": 2, "h": 1})], false)]
    fn layout_cases(#[case] tiles: Vec<Value>, #[case] ok: bool) {
        let got = check_layout(&json!({"name": "D", "tiles": tiles}));
        assert_eq!(got.is_ok(), ok, "{got:?}");
    }

    #[test]
    fn negative_layout_needs_tiles() {
        assert!(check_layout(&json!({"name": "D"})).is_err());
    }

    #[rstest]
    #[case::positive_transport(Error::Transport("refused".into()), true)]
    #[case::positive_not_seeded(Error::NoSource("trace".into()), true)]
    #[case::positive_503(Error::Http { status: 503, path: "/sources".into() }, true)]
    #[case::positive_429(Error::Http { status: 429, path: "/sources".into() }, true)]
    #[case::negative_login_needed(Error::Http { status: 401, path: "/sources".into() }, false)]
    #[case::negative_schema_rejected(Error::Http { status: 400, path: "/dashboards".into() }, false)]
    #[case::negative_invalid(Error::Invalid("x".into()), false)]
    #[case::negative_ambiguous(Error::AmbiguousSource { kind: "trace".into(), count: 2 }, false)]
    fn retryable_cases(#[case] e: Error, #[case] want: bool) {
        assert_eq!(retryable(&e), want);
    }

    /// An in-memory HyperDX: canned GET replies, recorded writes, optional failures.
    #[derive(Default)]
    struct FakeApi {
        gets: BTreeMap<&'static str, Result<Value>>,
        fail_writes: Option<Error>,
        writes: Mutex<Vec<(String, String, Value)>>,
    }

    #[async_trait]
    impl Api for FakeApi {
        async fn get(&self, path: &str) -> Result<Value> {
            self.gets.get(path).cloned().unwrap_or_else(|| {
                Err(Error::Http {
                    status: 404,
                    path: path.into(),
                })
            })
        }
        async fn post(&self, path: &str, body: &Value) -> Result<()> {
            self.write("POST", path, body)
        }
        async fn patch(&self, path: &str, body: &Value) -> Result<()> {
            self.write("PATCH", path, body)
        }
    }

    impl FakeApi {
        fn write(&self, m: &str, path: &str, body: &Value) -> Result<()> {
            if let Some(e) = &self.fail_writes {
                return Err(e.clone());
            }
            self.writes
                .lock()
                .unwrap()
                .push((m.into(), path.into(), body.clone()));
            Ok(())
        }
    }

    fn desired() -> Desired {
        Desired {
            dashboards: vec![
                json!({"name": "New", "tags": [MANAGED_TAG], "tiles": [{"config": {"source": "$trace"}}]}),
                json!({"name": "Old", "tags": [MANAGED_TAG], "tiles": []}),
                json!({"name": "Mine", "tags": [MANAGED_TAG], "tiles": []}),
            ],
            saved_searches: vec![json!({"name": "S", "tags": [MANAGED_TAG], "source": "$trace"})],
        }
    }

    fn api() -> FakeApi {
        let mut a = FakeApi::default();
        a.gets.insert(
            "/sources",
            Ok(json!([{"id": TRACE, "kind": "trace", "name": "Traces"}])),
        );
        a.gets.insert(
            "/dashboards",
            Ok(json!([
                {"id": D1, "name": "Old", "tags": [MANAGED_TAG]},
                {"id": LOG, "name": "Mine", "tags": []},
            ])),
        );
        a.gets.insert("/saved-search", Ok(json!([])));
        a
    }

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn positive_provision_creates_updates_and_skips() {
        let a = api();
        let r = run(provision(&a, &desired())).unwrap();
        assert_eq!(
            r,
            Report {
                created: vec!["New".into(), "S".into()],
                updated: vec!["Old".into()],
                skipped: vec!["Mine".into()],
                duplicates: vec![],
            }
        );
        let w = a.writes.lock().unwrap();
        let calls: Vec<(&str, &str)> = w.iter().map(|(m, p, _)| (m.as_str(), p.as_str())).collect();
        assert_eq!(
            calls,
            vec![
                ("POST", "/dashboards"),
                ("PATCH", "/dashboards/0123456789abcdef01234567"),
                ("POST", "/saved-search"),
            ]
        );
        // The placeholder reached HyperDX as the real id.
        assert_eq!(w[0].2["tiles"][0]["config"]["source"], TRACE);
        assert_eq!(w[2].2["source"], TRACE);
    }

    #[rstest]
    #[case::positive_nested(json!({"tiles": [{"config": {"source": TRACE}}], "source": LOG}), vec![TRACE, LOG])]
    #[case::corner_none(json!({"where": "x"}), vec![])]
    #[case::corner_non_string_source(json!({"source": {"source": TRACE}}), vec![TRACE])]
    #[case::adversarial_unresolved_placeholder(json!({"source": "$trace"}), vec!["$trace"])]
    fn sources_in_cases(#[case] v: Value, #[case] want: Vec<&str>) {
        let mut got = sources_in(&v);
        let mut want: Vec<String> = want.into_iter().map(String::from).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn corner_duplicate_managed_copies_are_reported() {
        let mut a = api();
        a.gets.insert(
            "/dashboards",
            Ok(json!([
                {"id": D1, "name": "Old", "tags": [MANAGED_TAG]},
                {"id": TRACE, "name": "Old", "tags": [MANAGED_TAG]},
            ])),
        );
        let r = run(provision(&a, &desired())).unwrap();
        assert_eq!(r.duplicates, vec!["Old".to_string()]);
        assert!(r.updated.contains(&"Old".to_string()));
    }

    #[test]
    fn corner_empty_template_reads_sources_only() {
        let a = api();
        let r = run(provision(&a, &Desired::default())).unwrap();
        assert_eq!(r, Report::default());
        assert!(a.writes.lock().unwrap().is_empty());
    }

    #[rstest]
    #[case::negative_login_required("/sources", Error::Http { status: 401, path: "/sources".into() })]
    #[case::negative_list_fails("/dashboards", Error::Http { status: 500, path: "/dashboards".into() })]
    fn provision_read_error_cases(#[case] path: &'static str, #[case] e: Error) {
        let mut a = api();
        a.gets.insert(path, Err(e.clone()));
        assert_eq!(run(provision(&a, &desired())), Err(e));
        assert!(a.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn negative_unseeded_team_writes_nothing() {
        let mut a = api();
        a.gets.insert("/sources", Ok(json!([])));
        let got = run(provision(&a, &desired()));
        assert_eq!(got, Err(Error::NoSource("trace".into())));
        assert!(retryable(&got.unwrap_err()));
        assert!(a.writes.lock().unwrap().is_empty());
    }

    #[test]
    fn negative_rejected_write_stops() {
        let mut a = api();
        a.fail_writes = Some(Error::Http {
            status: 400,
            path: "/dashboards".into(),
        });
        let got = run(provision(&a, &desired()));
        assert!(matches!(got, Err(Error::Http { status: 400, .. })));
    }

    #[test]
    fn adversarial_hostile_existing_id_never_reaches_a_url() {
        let mut a = api();
        a.gets.insert(
            "/dashboards",
            Ok(json!([{"id": "../team/x", "name": "Old", "tags": [MANAGED_TAG]}])),
        );
        let got = run(provision(&a, &desired()));
        assert!(matches!(got, Err(Error::Invalid(_))), "{got:?}");
        assert!(a.writes.lock().unwrap().is_empty());
    }

    /// The real template file (nix/hyperdx/dashboards.nix rendered by the module) is
    /// covered by the `hyperdx-dashboards` check; this pins the shape it relies on.
    #[test]
    fn positive_template_shape_round_trips() {
        let bytes = serde_json::to_vec(&json!({
            "dashboards": [{"name": "agent-seddon · Overview", "tags": [MANAGED_TAG],
                "tiles": [{"id": "overview-0", "x": 0, "y": 0, "w": 12, "h": 8,
                    "config": {"source": "$trace", "select": [], "where": ""}}]}],
            "savedSearches": [{"name": "agent-seddon · Error spans", "tags": [MANAGED_TAG],
                "source": "$trace", "select": "", "where": "StatusCode = 'Error'"}],
        }))
        .unwrap();
        let d = Desired::parse(&bytes).unwrap();
        let r = resolve(&d.dashboards[0], &sources()).unwrap();
        assert_eq!(r["tiles"][0]["config"]["source"], TRACE);
        assert_eq!(r["tiles"][0]["id"], "overview-0");
    }
}
