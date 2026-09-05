//! `FileConfigStore` — the file-backed [`ConfigStore`] for the portal-settings seam.
//!
//! It exposes the agent's own `config/agent.toml` as a JSON-Schema + effective
//! values, validates candidate edits, and writes them back **in place** with
//! `toml_edit` (comments and layout preserved). Edits take effect on the next
//! agent start (the running `Config` is immutable), so [`FileConfigStore::status`]
//! reports the on-disk-vs-running drift that drives the portal's "restart
//! required" banner.
//!
//! Lives inside `agent-runtime` (not a sibling crate) because it needs the
//! `Config` type, `parse_config`, [`build_schema`](crate::build_schema), and
//! [`validate_config`](crate::validate_config) — a sibling crate depending on
//! those while `agent-runtime` depends on it would be a dependency cycle.
//!
//! Security: the file path is fixed by the server; every edit `path`/`value` is
//! untrusted. Paths are allowlisted against the schema, sizes/counts are capped,
//! validation runs before any write, the write is atomic, and secrets are masked
//! on read and never blanked by a round-trip. See the module tests.

use crate::config_schema::{is_secret_path, resolve_schema_pointer, SECRET_PATHS};
use agent_core::{
    ConfigEdit, ConfigIssue, ConfigIssueCode, ConfigStatus, ConfigStore, Error, Result,
    MAX_CONFIG_ARRAY_LEN, MAX_CONFIG_DEPTH, MAX_CONFIG_DOC_BYTES, MAX_CONFIG_EDITS,
    MAX_CONFIG_PATH_LEN, MAX_CONFIG_VALUE_BYTES,
};
use async_trait::async_trait;
use serde_json::Value;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table};

/// A file-backed config store over one `agent.toml`.
pub struct FileConfigStore {
    /// The config file the CLI read (write-back target).
    path: PathBuf,
    /// The running config serialized to JSON (defaults filled), captured at build
    /// before `Config` was consumed. Secrets are held unmasked and masked on read.
    snapshot: Value,
}

impl FileConfigStore {
    /// `snapshot` is `serde_json::to_value(&cfg)` of the config the agent booted
    /// with (defaults already applied by serde).
    pub fn new(path: PathBuf, snapshot: Value) -> Self {
        Self { path, snapshot }
    }

    /// Structural checks that never touch the file: caps + path allowlist.
    fn structural_issues(&self, schema: &Value, edits: &[ConfigEdit]) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();
        if edits.len() > MAX_CONFIG_EDITS {
            issues.push(issue(
                "",
                ConfigIssueCode::TooMany,
                format!(
                    "{} edits exceeds the cap of {MAX_CONFIG_EDITS}",
                    edits.len()
                ),
            ));
            return issues;
        }
        for e in edits {
            if e.path.len() > MAX_CONFIG_PATH_LEN {
                issues.push(issue(
                    &e.path,
                    ConfigIssueCode::TooLarge,
                    "path too long".into(),
                ));
                continue;
            }
            // Array-of-tables are edited by replacing the whole array at its
            // section path (e.g. `pool.members`), not per index — reject `[n]`.
            if e.path.contains('[') || e.path.contains(']') {
                issues.push(issue(
                    &e.path,
                    ConfigIssueCode::UnknownPath,
                    "indexed paths are not supported — replace the whole array by its path".into(),
                ));
                continue;
            }
            if e.path.split('.').count() > MAX_CONFIG_DEPTH {
                issues.push(issue(
                    &e.path,
                    ConfigIssueCode::TooLarge,
                    "path too deep".into(),
                ));
                continue;
            }
            if resolve_schema_pointer(schema, &e.path).is_none() {
                issues.push(issue(
                    &e.path,
                    ConfigIssueCode::UnknownPath,
                    "not a known config field".into(),
                ));
                continue;
            }
            if let Some(v) = &e.value {
                let size = serde_json::to_string(v).map(|s| s.len()).unwrap_or(0);
                if size > MAX_CONFIG_VALUE_BYTES {
                    issues.push(issue(
                        &e.path,
                        ConfigIssueCode::TooLarge,
                        "value too large".into(),
                    ));
                    continue;
                }
                if let Some(arr) = v.as_array() {
                    if arr.len() > MAX_CONFIG_ARRAY_LEN {
                        issues.push(issue(
                            &e.path,
                            ConfigIssueCode::TooLarge,
                            "array too long".into(),
                        ));
                        continue;
                    }
                }
            }
        }
        issues
    }

    /// Apply `edits` to a parsed document, returning apply issues (empty = ok).
    fn apply(doc: &mut DocumentMut, edits: &[ConfigEdit]) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();
        for e in edits {
            // An edit to a secret whose value is empty is a no-op: a form
            // round-trip (GetValues masks it to "") must never blank the stored
            // secret it was never shown.
            if is_secret_path(&e.path) {
                let empty = matches!(&e.value, Some(Value::String(s)) if s.is_empty());
                if empty {
                    continue;
                }
            }
            if let Err((code, detail)) = set_leaf(doc, &e.path, e.value.as_ref()) {
                issues.push(issue(&e.path, code, detail));
            }
        }
        issues
    }
}

#[async_trait]
impl ConfigStore for FileConfigStore {
    async fn schema(&self) -> Result<Value> {
        Ok(crate::build_schema())
    }

    async fn values(&self) -> Result<Value> {
        let mut v = self.snapshot.clone();
        mask_secrets(&mut v);
        Ok(v)
    }

    async fn validate(&self, edits: &[ConfigEdit]) -> Result<Vec<ConfigIssue>> {
        let schema = crate::build_schema();
        let structural = self.structural_issues(&schema, edits);
        if !structural.is_empty() {
            return Ok(structural);
        }
        let current = read_capped(&self.path)?;
        let mut doc = match current.parse::<DocumentMut>() {
            Ok(d) => d,
            Err(e) => {
                return Ok(vec![issue(
                    "",
                    ConfigIssueCode::Parse,
                    format!("current config does not parse: {e}"),
                )])
            }
        };
        let apply_issues = Self::apply(&mut doc, edits);
        if !apply_issues.is_empty() {
            return Ok(apply_issues);
        }
        let candidate = doc.to_string();
        if candidate.len() > MAX_CONFIG_DOC_BYTES {
            return Ok(vec![issue(
                "",
                ConfigIssueCode::TooLarge,
                "resulting config exceeds the size cap".into(),
            )]);
        }
        match crate::parse_config(&candidate) {
            Ok(cfg) => Ok(crate::validate_config(&cfg)),
            Err(e) => Ok(vec![issue(
                "",
                ConfigIssueCode::Parse,
                format!("resulting config does not parse: {e}"),
            )]),
        }
    }

    async fn put(&self, edits: Vec<ConfigEdit>) -> Result<Vec<ConfigIssue>> {
        let issues = self.validate(&edits).await?;
        if !issues.is_empty() {
            return Ok(issues); // reject wholesale — write nothing
        }
        let current = read_capped(&self.path)?;
        let mut doc = current
            .parse::<DocumentMut>()
            .map_err(|e| Error::Config(format!("config does not parse: {e}")))?;
        // Already validated; apply is infallible here in practice, but keep issues.
        let apply_issues = Self::apply(&mut doc, &edits);
        if !apply_issues.is_empty() {
            return Ok(apply_issues);
        }
        atomic_write(&self.path, doc.to_string().as_bytes())?;
        Ok(Vec::new())
    }

    async fn status(&self) -> Result<ConfigStatus> {
        let ondisk = match std::fs::read_to_string(&self.path) {
            Ok(s) => crate::parse_config(&s)
                .ok()
                .and_then(|c| serde_json::to_value(&c).ok())
                .unwrap_or(Value::Null),
            Err(_) => Value::Null,
        };
        let running = self.snapshot.clone();
        let mut pending = Vec::new();
        diff_leaves(String::new(), &running, &ondisk, &mut pending);
        // Never leak a secret through the drift report.
        for edit in &mut pending {
            if is_secret_path(&edit.path) {
                edit.value = Some(Value::String(String::new()));
            }
        }
        Ok(ConfigStatus {
            restart_required: !pending.is_empty(),
            pending,
            loaded_hash: hash_value(&running),
            ondisk_hash: hash_value(&ondisk),
        })
    }
}

// --- helpers ---------------------------------------------------------------

fn issue(path: &str, code: ConfigIssueCode, detail: String) -> ConfigIssue {
    ConfigIssue {
        path: path.to_string(),
        code,
        detail,
    }
}

fn read_capped(path: &Path) -> Result<String> {
    let s = std::fs::read_to_string(path)?;
    if s.len() > MAX_CONFIG_DOC_BYTES {
        return Err(Error::Config("config file exceeds the size cap".into()));
    }
    Ok(s)
}

/// Write `bytes` to `path` atomically: a sibling temp file + rename, so a crash
/// never leaves `agent.toml` truncated.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("agent.toml")
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Set (or, with `None`, remove) the leaf at a plain dotted `path`, creating any
/// missing intermediate tables. Returns a typed issue on a type clash.
fn set_leaf(
    doc: &mut DocumentMut,
    path: &str,
    value: Option<&Value>,
) -> std::result::Result<(), (ConfigIssueCode, String)> {
    let segs: Vec<&str> = path.split('.').collect();
    let mut tbl: &mut Table = doc.as_table_mut();
    for seg in &segs[..segs.len() - 1] {
        let entry = tbl.entry(seg).or_insert(Item::Table(Table::new()));
        tbl = entry
            .as_table_mut()
            .ok_or((ConfigIssueCode::BadType, format!("`{seg}` is not a table")))?;
    }
    let leaf = segs[segs.len() - 1];
    match value {
        None => {
            tbl.remove(leaf);
        }
        Some(v) => {
            let tv = json_to_toml(v).ok_or((
                ConfigIssueCode::BadType,
                "value is not representable in TOML".into(),
            ))?;
            tbl.insert(leaf, Item::Value(tv));
        }
    }
    Ok(())
}

/// Convert a JSON value to a `toml_edit` value (scalars, arrays, inline tables).
/// A JSON `null` (or an object field that is null) is dropped.
fn json_to_toml(v: &Value) -> Option<toml_edit::Value> {
    use toml_edit::Value as TV;
    Some(match v {
        Value::Null => return None,
        Value::Bool(b) => TV::from(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TV::from(i)
            } else {
                TV::from(n.as_f64()?)
            }
        }
        Value::String(s) => TV::from(s.clone()),
        Value::Array(a) => {
            let mut arr = toml_edit::Array::new();
            for e in a {
                arr.push(json_to_toml(e)?);
            }
            TV::Array(arr)
        }
        Value::Object(o) => {
            let mut t = toml_edit::InlineTable::new();
            for (k, val) in o {
                if let Some(tv) = json_to_toml(val) {
                    t.insert(k, tv);
                }
            }
            TV::InlineTable(t)
        }
    })
}

/// Replace every secret leaf with an empty string, in place.
fn mask_secrets(root: &mut Value) {
    for path in SECRET_PATHS {
        if let Some((prefix, suffix)) = path.split_once("[]") {
            let prefix = prefix.trim_end_matches('.');
            let field = suffix.trim_start_matches('.');
            if let Some(arr) = value_at_mut(root, prefix).and_then(Value::as_array_mut) {
                for el in arr.iter_mut() {
                    blank_leaf(el, field);
                }
            }
        } else {
            blank_leaf(root, path);
        }
    }
}

/// If the leaf at plain dotted `dotted` is a non-empty string, blank it.
fn blank_leaf(root: &mut Value, dotted: &str) {
    if let Some(node) = value_at_mut(root, dotted) {
        if matches!(node, Value::String(s) if !s.is_empty()) {
            *node = Value::String(String::new());
        }
    }
}

fn value_at_mut<'a>(root: &'a mut Value, dotted: &str) -> Option<&'a mut Value> {
    let mut cur = root;
    for seg in dotted.split('.') {
        cur = cur.as_object_mut()?.get_mut(seg)?;
    }
    Some(cur)
}

/// Walk two JSON trees, emitting a [`ConfigEdit`] (with the on-disk value) for
/// every leaf path where they differ. Arrays and scalars are compared whole.
fn diff_leaves(prefix: String, running: &Value, ondisk: &Value, out: &mut Vec<ConfigEdit>) {
    match (running, ondisk) {
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let child = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                let ra = a.get(k).unwrap_or(&Value::Null);
                let rb = b.get(k).unwrap_or(&Value::Null);
                diff_leaves(child, ra, rb, out);
            }
        }
        _ => {
            if running != ondisk {
                out.push(ConfigEdit {
                    path: prefix,
                    value: Some(ondisk.clone()),
                });
            }
        }
    }
}

fn hash_value(v: &Value) -> String {
    let s = serde_json::to_string(v).unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    const BASE: &str = "\
# a comment that must survive
[agent]
provider = \"openai-compat\" # inline comment
context = \"sliding-window\"
max_iterations = 25

[provider]
model = \"m\"
api_key = \"sekret\"
";

    fn store_with(dir: &Path, toml: &str) -> (FileConfigStore, PathBuf) {
        let path = dir.join("agent.toml");
        std::fs::write(&path, toml).unwrap();
        let cfg = crate::parse_config(toml).unwrap();
        let snap = serde_json::to_value(&cfg).unwrap();
        (FileConfigStore::new(path.clone(), snap), path)
    }

    fn edit(path: &str, v: Value) -> ConfigEdit {
        ConfigEdit {
            path: path.into(),
            value: Some(v),
        }
    }

    #[tokio::test]
    async fn positive_put_scalar_preserves_comments() {
        let tmp = agent_testkit::tempdir();
        let (store, path) = store_with(tmp.as_path(), BASE);
        let issues = store
            .put(vec![edit(
                "agent.context",
                Value::String("mode-aware-window".into()),
            )])
            .await
            .unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("a comment that must survive"),
            "top comment lost"
        );
        assert!(after.contains("# inline comment"), "inline comment lost");
        assert!(after.contains("mode-aware-window"), "edit not applied");
    }

    #[tokio::test]
    async fn positive_values_masks_secret() {
        let tmp = agent_testkit::tempdir();
        let (store, _p) = store_with(tmp.as_path(), BASE);
        let v = store.values().await.unwrap();
        assert_eq!(v["provider"]["api_key"], Value::String(String::new()));
    }

    #[tokio::test]
    async fn positive_status_reports_drift() {
        let tmp = agent_testkit::tempdir();
        let (store, path) = store_with(tmp.as_path(), BASE);
        // Snapshot took context=sliding-window; change the file underneath.
        let changed = BASE.replace("sliding-window", "summarizing-window");
        std::fs::write(&path, changed).unwrap();
        let st = store.status().await.unwrap();
        assert!(st.restart_required);
        assert!(st.pending.iter().any(|e| e.path == "agent.context"));
    }

    #[tokio::test]
    async fn boundary_empty_secret_edit_leaves_secret_intact() {
        let tmp = agent_testkit::tempdir();
        let (store, path) = store_with(tmp.as_path(), BASE);
        let issues = store
            .put(vec![edit("provider.api_key", Value::String(String::new()))])
            .await
            .unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("api_key = \"sekret\""),
            "secret was blanked: {after}"
        );
    }

    #[rstest]
    #[case::traversal("../../etc/passwd")]
    #[case::injection("agent.context\n[evil]\nx")]
    #[case::indexed("pool.members[0].api_key")]
    #[case::unknown("agent.definitely_not_a_field")]
    #[tokio::test]
    async fn adversarial_bad_path_is_rejected_and_writes_nothing(#[case] path: &str) {
        let tmp = agent_testkit::tempdir();
        let (store, file) = store_with(tmp.as_path(), BASE);
        let before = std::fs::read_to_string(&file).unwrap();
        let issues = store
            .put(vec![edit(path, Value::String("x".into()))])
            .await
            .unwrap();
        assert!(!issues.is_empty(), "expected rejection for `{path}`");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            before,
            "file was written"
        );
    }

    #[tokio::test]
    async fn adversarial_bad_enum_value_writes_nothing() {
        let tmp = agent_testkit::tempdir();
        let (store, file) = store_with(tmp.as_path(), BASE);
        let before = std::fs::read_to_string(&file).unwrap();
        let issues = store
            .put(vec![edit("agent.context", Value::String("bogus".into()))])
            .await
            .unwrap();
        assert!(issues.iter().any(|i| i.code == ConfigIssueCode::BadEnum));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    }

    #[tokio::test]
    async fn adversarial_bad_type_writes_nothing() {
        let tmp = agent_testkit::tempdir();
        let (store, file) = store_with(tmp.as_path(), BASE);
        let before = std::fs::read_to_string(&file).unwrap();
        // max_iterations is a number; a string must be rejected at parse.
        let issues = store
            .put(vec![edit(
                "agent.max_iterations",
                Value::String("lots".into()),
            )])
            .await
            .unwrap();
        assert!(!issues.is_empty());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    }

    #[tokio::test]
    async fn adversarial_too_many_edits_is_capped() {
        let tmp = agent_testkit::tempdir();
        let (store, _f) = store_with(tmp.as_path(), BASE);
        let edits: Vec<ConfigEdit> = (0..=MAX_CONFIG_EDITS)
            .map(|_| edit("agent.context", Value::String("sliding-window".into())))
            .collect();
        let issues = store.validate(&edits).await.unwrap();
        assert!(issues.iter().any(|i| i.code == ConfigIssueCode::TooMany));
    }

    #[tokio::test]
    async fn adversarial_oversized_value_is_capped() {
        let tmp = agent_testkit::tempdir();
        let (store, _f) = store_with(tmp.as_path(), BASE);
        let huge = "x".repeat(MAX_CONFIG_VALUE_BYTES + 1);
        let issues = store
            .validate(&[edit("agent.working_dir", Value::String(huge))])
            .await
            .unwrap();
        assert!(issues.iter().any(|i| i.code == ConfigIssueCode::TooLarge));
    }

    #[tokio::test]
    async fn positive_delete_reverts_to_default() {
        let tmp = agent_testkit::tempdir();
        let (store, path) = store_with(tmp.as_path(), BASE);
        let issues = store
            .put(vec![ConfigEdit {
                path: "agent.max_iterations".into(),
                value: None,
            }])
            .await
            .unwrap();
        assert!(issues.is_empty(), "{issues:?}");
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            !after.contains("max_iterations"),
            "key not removed: {after}"
        );
    }
}
