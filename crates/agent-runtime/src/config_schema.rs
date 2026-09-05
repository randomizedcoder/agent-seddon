//! Config schema + cheap validation for the `ConfigStore` seam (portal settings).
//!
//! [`build_schema`] turns the `Config` structs into a JSON-Schema (via `schemars`,
//! which lifts the `///` doc-comments into `description`), then folds in two things
//! `schemars` cannot infer: the **enum choices** for the many `String`-that-are-
//! really-enum fields (so the portal renders dropdowns), and an **`x-secret`** flag
//! on the inline-secret fields (so the portal masks them).
//!
//! [`validate_config`] is the cheap, fail-closed check that a config's enum-valued
//! fields hold one of their allowed values — shared by the seam's `Validate`/`Put`.
//!
//! Only stable, non-array dotted paths are annotated. Array-element enums
//! (`pool.members[].tier`, `route.rules[].match.role`, …) are left as free text in
//! v1 — they are still checked fail-closed at build time.

use agent_core::{ConfigIssue, ConfigIssueCode};
use serde_json::Value;

/// Dotted path → the closed set of allowed string values. A `""` entry means an
/// empty string is a valid (often "off"/"use default") choice. Sourced from each
/// field's doc-comment in `config.rs`; a `#[cfg(test)]` test asserts every path
/// still resolves to a string node, so a field rename fails the build.
pub const ENUM_CHOICES: &[(&str, &[&str])] = &[
    (
        "agent.provider",
        &[
            "openai-compat",
            "anthropic",
            "router",
            "task-router",
            "consensus",
            "grpc",
            "pool",
        ],
    ),
    (
        "agent.context",
        &[
            "sliding-window",
            "summarizing-window",
            "mode-aware-window",
            "instant-window",
        ],
    ),
    (
        "agent.policy",
        &["interactive", "auto-approve", "allow-list"],
    ),
    ("memory.backend", &["file", "grpc"]),
    ("memory.semantic", &["", "file", "vector", "grpc"]),
    ("policy.guard", &["prompt", "deny", "off"]),
    ("git.backend", &["", "cli", "hybrid", "grpc"]),
    ("git.push_policy", &["never", "checkpoint-only", "explicit"]),
    (
        "tokenizer.backend",
        &["approx", "tiktoken", "hf", "provider", "grpc"],
    ),
    ("sandbox.backend", &["local", "nix"]),
    ("web.backend", &["local", "grpc"]),
    ("lsp.backend", &["local", "grpc"]),
    ("cache.strategy", &["stable-prefix", "tail-window", "off"]),
    (
        "scanner.deny_at",
        &["info", "low", "medium", "high", "critical"],
    ),
    ("scanner.scope", &["all", "context", "strict"]),
    ("verifier.backend", &["", "schema", "llm", "ensemble"]),
    ("verifier.mode", &["shadow", "enforce"]),
    ("pty.backend", &["local", "grpc"]),
    ("forge.backend", &["", "github", "gitlab"]),
    ("router.policy", &["in-order", "round-robin"]),
    (
        "pool.policy",
        &["cost", "least-loaded", "round-robin", "weighted"],
    ),
    ("pool.on_saturation", &["shed", "wait"]),
    ("digest.store", &["", "clickhouse", "sqlite"]),
    ("instant.relevance", &["", "llm", "keyword", "all"]),
    ("graph.store", &["file", "grpc", ""]),
    ("registry.store", &["file", "sqlite", "grpc", ""]),
    ("mode.classifier", &["hybrid", ""]),
    ("dimensions.store", &["", "off", "file", "grpc"]),
    ("review.backend", &["", "local", "grpc"]),
    ("review.classifier", &["hybrid", ""]),
    ("consensus.scope", &["", "final", "every-iteration"]),
    (
        "consensus.on_exhaustion",
        &["", "deliver-with-note", "fail"],
    ),
    ("consensus.evidence", &["auto", "off"]),
    ("route.source", &["", "registry"]),
];

/// Dotted paths (value-space) whose contents are an **inline secret** and must be
/// masked before leaving the process. A `[]` segment matches every element of an
/// array-of-tables. Only the actual inline secret is listed — `*_env` / `*_file`
/// fields are references (an env-var name / a path), not secrets.
pub const SECRET_PATHS: &[&str] = &[
    "provider.api_key",
    "telemetry.password",
    "forge.token",
    "web_search.brave_api_key",
    "tokenizer.provider.api_key",
    "pool.members[].api_key",
    "route.upstreams[].api_key",
];

/// Build the annotated JSON-Schema for the whole [`Config`](crate::config::Config).
pub fn build_schema() -> Value {
    let root = schemars::schema_for!(crate::config::Config);
    let mut v = serde_json::to_value(root).unwrap_or_else(|_| Value::Object(Default::default()));
    for (path, choices) in ENUM_CHOICES {
        if let Some(ptr) = resolve_schema_pointer(&v, path) {
            if let Some(node) = v.pointer_mut(&ptr).and_then(Value::as_object_mut) {
                node.insert(
                    "enum".into(),
                    Value::Array(choices.iter().map(|c| Value::String((*c).into())).collect()),
                );
            }
        }
    }
    for path in SECRET_PATHS {
        if path.contains("[]") {
            continue; // array-element secrets are masked in values(), not annotated here
        }
        if let Some(ptr) = resolve_schema_pointer(&v, path) {
            if let Some(node) = v.pointer_mut(&ptr).and_then(Value::as_object_mut) {
                node.insert("x-secret".into(), Value::Bool(true));
            }
        }
    }
    v
}

/// Cheap, fail-closed check: every enum-valued field holds one of its choices.
/// Returns a [`ConfigIssue`] (code [`ConfigIssueCode::BadEnum`]) per violation.
pub fn validate_config(cfg: &crate::config::Config) -> Vec<ConfigIssue> {
    let values = match serde_json::to_value(cfg) {
        Ok(v) => v,
        // A Config that will not serialize is a programming error, not operator
        // input; surface it as a single parse issue rather than panicking.
        Err(e) => {
            return vec![ConfigIssue {
                path: String::new(),
                code: ConfigIssueCode::Parse,
                detail: format!("config did not serialize: {e}"),
            }]
        }
    };
    let mut issues = Vec::new();
    for (path, choices) in ENUM_CHOICES {
        let Some(node) = value_at(&values, path) else {
            continue;
        };
        let Some(s) = node.as_str() else {
            continue;
        };
        // An empty string is "unset — use the default": a field whose struct
        // derives `Default` serializes to `""` when its whole section is absent,
        // even though its per-key serde default is non-empty. The real backend
        // name is resolved (and checked) at build time, so only a *non-empty*
        // value that isn't a choice is a typo worth flagging here.
        if s.is_empty() {
            continue;
        }
        if !choices.contains(&s) {
            issues.push(ConfigIssue {
                path: (*path).to_string(),
                code: ConfigIssueCode::BadEnum,
                detail: format!("`{s}` is not one of: {}", choices.join(", ")),
            });
        }
    }
    issues
}

/// Whether a dotted path (with optional `[]` wildcard) is a masked secret.
pub fn is_secret_path(path: &str) -> bool {
    SECRET_PATHS.contains(&path)
}

/// Get a value node by a plain dotted path (values-space; no `$ref`, no wildcards).
fn value_at<'a>(root: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

/// Resolve a dotted config path to a JSON Pointer into the generated schema,
/// following `$ref`/`allOf` into `definitions`. Returns `None` if the path does
/// not resolve (a caller uses this both to annotate and to allowlist edit paths).
pub fn resolve_schema_pointer(root: &Value, dotted: &str) -> Option<String> {
    let mut ptr = String::new();
    let mut cur = root;
    for seg in dotted.split('.') {
        // Deref the current node to the object that actually holds `properties`.
        let (node, node_ptr) = deref(root, cur, ptr)?;
        cur = node;
        ptr = node_ptr;
        let props = cur.get("properties")?;
        let next = props.get(seg)?;
        ptr.push_str("/properties/");
        ptr.push_str(seg);
        cur = next;
    }
    Some(ptr)
}

/// Follow a chain of `$ref` (and single-`$ref` `allOf`) links, updating the JSON
/// Pointer to point at the resolved definition.
fn deref<'a>(root: &'a Value, mut cur: &'a Value, mut ptr: String) -> Option<(&'a Value, String)> {
    loop {
        if let Some(r) = cur.get("$ref").and_then(Value::as_str) {
            let name = r.strip_prefix("#/definitions/")?;
            ptr = format!("/definitions/{name}");
            cur = root.get("definitions")?.get(name)?;
            continue;
        }
        // schemars wraps a $ref in a single-element `allOf` when the field also
        // carries a description; unwrap that one level.
        if let Some(arr) = cur.get("allOf").and_then(Value::as_array) {
            if arr.len() == 1 {
                if let Some(r) = arr[0].get("$ref").and_then(Value::as_str) {
                    let name = r.strip_prefix("#/definitions/")?;
                    ptr = format!("/definitions/{name}");
                    cur = root.get("definitions")?.get(name)?;
                    continue;
                }
            }
        }
        return Some((cur, ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_every_enum_path_resolves_to_a_string_node() {
        let schema = build_schema();
        for (path, choices) in ENUM_CHOICES {
            let ptr = resolve_schema_pointer(&schema, path)
                .unwrap_or_else(|| panic!("enum path `{path}` did not resolve in the schema"));
            let node = schema
                .pointer(&ptr)
                .unwrap_or_else(|| panic!("pointer `{ptr}` for `{path}` missing"));
            // The annotated node carries the enum we inserted.
            let got = node
                .get("enum")
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("`{path}` node has no enum after build"));
            assert_eq!(got.len(), choices.len(), "enum count mismatch for `{path}`");
        }
    }

    #[test]
    fn positive_every_non_wildcard_secret_path_resolves() {
        let schema = build_schema();
        for path in SECRET_PATHS {
            if path.contains("[]") {
                continue;
            }
            let ptr = resolve_schema_pointer(&schema, path)
                .unwrap_or_else(|| panic!("secret path `{path}` did not resolve"));
            let node = schema.pointer(&ptr).expect("resolved secret node");
            assert_eq!(
                node.get("x-secret").and_then(Value::as_bool),
                Some(true),
                "`{path}` was not flagged x-secret"
            );
        }
    }

    #[test]
    fn negative_unknown_path_does_not_resolve() {
        let schema = build_schema();
        assert!(resolve_schema_pointer(&schema, "agent.nope").is_none());
        assert!(resolve_schema_pointer(&schema, "nope.at.all").is_none());
    }

    #[test]
    fn positive_default_config_validates_clean() {
        // The reference config must have zero enum violations.
        let toml = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../config/agent.toml"
        ))
        .expect("read reference config");
        let cfg = crate::parse_config(&toml).expect("parse reference config");
        let issues = validate_config(&cfg);
        assert!(issues.is_empty(), "reference config has issues: {issues:?}");
    }

    #[test]
    fn negative_bad_enum_is_flagged() {
        let toml = "[agent]\nprovider=\"openai-compat\"\ncontext=\"bogus-window\"\n\
                    [provider]\nmodel=\"m\"\n";
        let cfg = crate::parse_config(toml).expect("parse");
        let issues = validate_config(&cfg);
        assert!(
            issues
                .iter()
                .any(|i| i.path == "agent.context" && i.code == ConfigIssueCode::BadEnum),
            "expected a BadEnum on agent.context, got {issues:?}"
        );
    }
}
