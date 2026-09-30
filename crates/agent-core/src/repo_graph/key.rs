//! The node-key grammar, the closed kind sets, and the pure derivations over a key
//! (`01-schema.md`).
//!
//! A node key is the graph's stable, human-readable identity for a symbol
//! (`rust:fn:agent_core::security::confine`). It is **model-visible** — it appears in tool
//! arguments and in citations — and therefore attacker-controlled, so [`NodeKey::parse`]
//! validates it at the trust boundary, once, and every rejection names the rule it broke and
//! never echoes the offending bytes. The typed constructors ([`NodeKey::rust_item`], …) build a
//! key from already-split parts and validate each segment, so an extractor cannot mint a key the
//! parser would reject.

use super::{NodeId, RepoGraphError, RepoGraphResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The maximum length of a node key, in bytes (`graph_nodes.node_key` CHECK).
pub const MAX_NODE_KEY_LEN: usize = 512;

/// The maximum length of a `file` / `doc` path segment, in bytes.
pub const MAX_PATH_LEN: usize = 512;

/// The maximum number of `name_tokens` kept for a node.
pub const MAX_NAME_TOKENS: usize = 16;

/// The maximum length of a single `name_token`, in bytes.
pub const MAX_NAME_TOKEN_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Closed enums: node kinds, edge kinds, languages
// ---------------------------------------------------------------------------

/// The closed set of node kinds (`graph_nodes.kind` CHECK, `01-schema.md`). Everything not on
/// this list is an attribute, so the kind set stays small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Repo,
    Crate,
    Package,
    Module,
    File,
    Fn,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Type,
    Const,
    Static,
    Macro,
    Test,
    Feature,
    Doc,
    ProtoService,
    ProtoRpc,
    ProtoMessage,
    Table,
    ConfigKey,
    Metric,
    Span,
}

impl NodeKind {
    /// The wire / SQL spelling (`Fn` → `"fn"`, `ProtoRpc` → `"proto_rpc"`).
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Repo => "repo",
            NodeKind::Crate => "crate",
            NodeKind::Package => "package",
            NodeKind::Module => "module",
            NodeKind::File => "file",
            NodeKind::Fn => "fn",
            NodeKind::Method => "method",
            NodeKind::Struct => "struct",
            NodeKind::Enum => "enum",
            NodeKind::Trait => "trait",
            NodeKind::Impl => "impl",
            NodeKind::Type => "type",
            NodeKind::Const => "const",
            NodeKind::Static => "static",
            NodeKind::Macro => "macro",
            NodeKind::Test => "test",
            NodeKind::Feature => "feature",
            NodeKind::Doc => "doc",
            NodeKind::ProtoService => "proto_service",
            NodeKind::ProtoRpc => "proto_rpc",
            NodeKind::ProtoMessage => "proto_message",
            NodeKind::Table => "table",
            NodeKind::ConfigKey => "config_key",
            NodeKind::Metric => "metric",
            NodeKind::Span => "span",
        }
    }

    /// Parse the wire / SQL spelling; `None` for anything off the closed set.
    pub fn parse(s: &str) -> Option<NodeKind> {
        Some(match s {
            "repo" => NodeKind::Repo,
            "crate" => NodeKind::Crate,
            "package" => NodeKind::Package,
            "module" => NodeKind::Module,
            "file" => NodeKind::File,
            "fn" => NodeKind::Fn,
            "method" => NodeKind::Method,
            "struct" => NodeKind::Struct,
            "enum" => NodeKind::Enum,
            "trait" => NodeKind::Trait,
            "impl" => NodeKind::Impl,
            "type" => NodeKind::Type,
            "const" => NodeKind::Const,
            "static" => NodeKind::Static,
            "macro" => NodeKind::Macro,
            "test" => NodeKind::Test,
            "feature" => NodeKind::Feature,
            "doc" => NodeKind::Doc,
            "proto_service" => NodeKind::ProtoService,
            "proto_rpc" => NodeKind::ProtoRpc,
            "proto_message" => NodeKind::ProtoMessage,
            "table" => NodeKind::Table,
            "config_key" => NodeKind::ConfigKey,
            "metric" => NodeKind::Metric,
            "span" => NodeKind::Span,
            _ => return None,
        })
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The closed set of edge kinds (`graph_edges.kind` CHECK, `01-schema.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Contains,
    DefinedIn,
    Imports,
    Implements,
    ImplFor,
    DependsOn,
    GatedBy,
    Tests,
    Documents,
    Calls,
    References,
    CoChangesWith,
    SimilarTo,
}

impl EdgeKind {
    /// The wire / SQL spelling (`DefinedIn` → `"defined_in"`).
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Contains => "contains",
            EdgeKind::DefinedIn => "defined_in",
            EdgeKind::Imports => "imports",
            EdgeKind::Implements => "implements",
            EdgeKind::ImplFor => "impl_for",
            EdgeKind::DependsOn => "depends_on",
            EdgeKind::GatedBy => "gated_by",
            EdgeKind::Tests => "tests",
            EdgeKind::Documents => "documents",
            EdgeKind::Calls => "calls",
            EdgeKind::References => "references",
            EdgeKind::CoChangesWith => "co_changes_with",
            EdgeKind::SimilarTo => "similar_to",
        }
    }

    /// Parse the wire / SQL spelling; `None` for anything off the closed set.
    pub fn parse(s: &str) -> Option<EdgeKind> {
        Some(match s {
            "contains" => EdgeKind::Contains,
            "defined_in" => EdgeKind::DefinedIn,
            "imports" => EdgeKind::Imports,
            "implements" => EdgeKind::Implements,
            "impl_for" => EdgeKind::ImplFor,
            "depends_on" => EdgeKind::DependsOn,
            "gated_by" => EdgeKind::GatedBy,
            "tests" => EdgeKind::Tests,
            "documents" => EdgeKind::Documents,
            "calls" => EdgeKind::Calls,
            "references" => EdgeKind::References,
            "co_changes_with" => EdgeKind::CoChangesWith,
            "similar_to" => EdgeKind::SimilarTo,
            _ => return None,
        })
    }
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The language a key belongs to (`graph_nodes.lang`), derived from the key prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    Rust,
    Go,
    Md,
    Proto,
    Sql,
    Toml,
    /// Structural nodes with no language (`repo`, `file`, `metric`, `span`).
    None,
}

impl Lang {
    /// The wire / SQL spelling; [`Lang::None`] is the empty string.
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::Go => "go",
            Lang::Md => "md",
            Lang::Proto => "proto",
            Lang::Sql => "sql",
            Lang::Toml => "toml",
            Lang::None => "",
        }
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// The key prefix table
// ---------------------------------------------------------------------------

/// `(prefix, kind, lang, remainder_is_path)` for every grammar row whose remainder is a single
/// free field. The remaining rows (`file`, `doc`) set `remainder_is_path = true` so [`parse`]
/// applies [`repo_relative`] to what follows the prefix.
const PREFIXES: &[(&str, NodeKind, Lang, bool)] = &[
    ("repo:", NodeKind::Repo, Lang::None, false),
    ("file:", NodeKind::File, Lang::None, true),
    ("doc:", NodeKind::Doc, Lang::Md, true),
    ("rust:crate:", NodeKind::Crate, Lang::Rust, false),
    ("rust:feature:", NodeKind::Feature, Lang::Rust, false),
    ("rust:mod:", NodeKind::Module, Lang::Rust, false),
    ("rust:fn:", NodeKind::Fn, Lang::Rust, false),
    ("rust:struct:", NodeKind::Struct, Lang::Rust, false),
    ("rust:enum:", NodeKind::Enum, Lang::Rust, false),
    ("rust:trait:", NodeKind::Trait, Lang::Rust, false),
    ("rust:type:", NodeKind::Type, Lang::Rust, false),
    ("rust:const:", NodeKind::Const, Lang::Rust, false),
    ("rust:static:", NodeKind::Static, Lang::Rust, false),
    ("rust:macro:", NodeKind::Macro, Lang::Rust, false),
    ("rust:impl:", NodeKind::Impl, Lang::Rust, false),
    ("rust:method:", NodeKind::Method, Lang::Rust, false),
    ("rust:test:", NodeKind::Test, Lang::Rust, false),
    ("go:package:", NodeKind::Package, Lang::Go, false),
    ("go:func:", NodeKind::Fn, Lang::Go, false),
    ("go:struct:", NodeKind::Struct, Lang::Go, false),
    ("go:interface:", NodeKind::Trait, Lang::Go, false),
    ("go:type:", NodeKind::Type, Lang::Go, false),
    ("go:method:", NodeKind::Method, Lang::Go, false),
    ("go:test:", NodeKind::Test, Lang::Go, false),
    ("proto:service:", NodeKind::ProtoService, Lang::Proto, false),
    ("proto:rpc:", NodeKind::ProtoRpc, Lang::Proto, false),
    ("proto:message:", NodeKind::ProtoMessage, Lang::Proto, false),
    ("sql:table:", NodeKind::Table, Lang::Sql, false),
    ("cfg:", NodeKind::ConfigKey, Lang::Toml, false),
    ("metric:", NodeKind::Metric, Lang::None, false),
    ("span:", NodeKind::Span, Lang::None, false),
];

// ---------------------------------------------------------------------------
// NodeKey
// ---------------------------------------------------------------------------

/// A validated node key. Construct from an untrusted string with [`NodeKey::parse`], or from
/// already-split parts with a typed constructor; both guarantee the invariant that the stored
/// string satisfies the grammar, so [`NodeKey::kind`] / [`NodeKey::lang`] are total.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeKey {
    raw: String,
    kind: NodeKind,
    lang: Lang,
}

impl NodeKey {
    /// Parse and validate an untrusted key. Rejects a non-ASCII byte, whitespace, a control
    /// char, an empty body, a length over [`MAX_NODE_KEY_LEN`], an unknown prefix, a malformed
    /// `@<sha8>` duplicate suffix, and a `file`/`doc` path that is not repo-relative. The error
    /// names the rule, never the input.
    pub fn parse(s: &str) -> RepoGraphResult<NodeKey> {
        if s.is_empty() {
            return Err(RepoGraphError::Invalid("node_key: empty".into()));
        }
        if s.len() > MAX_NODE_KEY_LEN {
            return Err(RepoGraphError::TooLong("node_key".into()));
        }
        if !s.is_ascii() {
            return Err(RepoGraphError::Invalid("node_key: non-ascii".into()));
        }
        if s.bytes().any(|b| b.is_ascii_whitespace()) {
            return Err(RepoGraphError::Invalid("node_key: whitespace".into()));
        }
        if s.bytes().any(|b| b.is_ascii_control()) {
            return Err(RepoGraphError::Invalid("node_key: control char".into()));
        }
        // Split off the optional `@<sha8>` duplicate suffix. `@` is reserved for it: any other
        // use is rejected so a hostile key cannot smuggle one in.
        let base = match s.rfind('@') {
            Some(at) => {
                let suffix = &s[at + 1..];
                let good = suffix.len() == 8
                    && suffix
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
                if !good || s[..at].contains('@') {
                    return Err(RepoGraphError::Invalid("node_key: bad @ suffix".into()));
                }
                &s[..at]
            }
            None => s,
        };
        let &(prefix, kind, lang, is_path) = PREFIXES
            .iter()
            .find(|(p, ..)| base.starts_with(p))
            .ok_or_else(|| RepoGraphError::Invalid("node_key: unknown prefix".into()))?;
        let body = &base[prefix.len()..];
        if body.is_empty() {
            return Err(RepoGraphError::Invalid("node_key: empty body".into()));
        }
        if is_path && !repo_relative(body) {
            return Err(RepoGraphError::Invalid(
                "node_key: file path not repo-relative".into(),
            ));
        }
        Ok(NodeKey {
            raw: s.to_string(),
            kind,
            lang,
        })
    }

    /// The validated key string.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The node kind, derived from the prefix.
    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    /// The language, derived from the prefix.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    // -- typed constructors ------------------------------------------------

    /// `repo:<slug>`; `slug` must be a [`crate::safe_segment`].
    pub fn repo(slug: &str) -> RepoGraphResult<NodeKey> {
        if !crate::safe_segment(slug) {
            return Err(RepoGraphError::Invalid("repo slug".into()));
        }
        Self::parse(&format!("repo:{slug}"))
    }

    /// `file:<path>`; `path` must be repo-relative.
    pub fn file(path: &str) -> RepoGraphResult<NodeKey> {
        if !repo_relative(path) {
            return Err(RepoGraphError::Invalid("file path".into()));
        }
        Self::parse(&format!("file:{path}"))
    }

    /// `doc:<path>`; `path` must be repo-relative.
    pub fn doc(path: &str) -> RepoGraphResult<NodeKey> {
        if !repo_relative(path) {
            return Err(RepoGraphError::Invalid("doc path".into()));
        }
        Self::parse(&format!("doc:{path}"))
    }

    /// `rust:crate:<crate>`.
    pub fn rust_crate(krate: &str) -> RepoGraphResult<NodeKey> {
        check_ident(krate, "crate")?;
        Self::parse(&format!("rust:crate:{krate}"))
    }

    /// `rust:feature:<crate>/<feature>`.
    pub fn rust_feature(krate: &str, feature: &str) -> RepoGraphResult<NodeKey> {
        check_ident(krate, "crate")?;
        check_feature(feature)?;
        Self::parse(&format!("rust:feature:{krate}/{feature}"))
    }

    /// `rust:mod:<crate>::<mod path>`; `mod_path` is a possibly-empty `::` path of idents.
    pub fn rust_mod(krate: &str, mod_path: &str) -> RepoGraphResult<NodeKey> {
        check_ident(krate, "crate")?;
        check_mod_path(mod_path)?;
        Self::parse(&format!("rust:mod:{}", join_path(krate, mod_path, "")))
    }

    /// `rust:<kind>:<crate>::<mod path>::<Name>` for `fn`/`struct`/`enum`/`trait`/`type`/
    /// `const`/`static`/`macro`.
    pub fn rust_item(
        kind: NodeKind,
        krate: &str,
        mod_path: &str,
        name: &str,
    ) -> RepoGraphResult<NodeKey> {
        let prefix = match kind {
            NodeKind::Fn => "fn",
            NodeKind::Struct => "struct",
            NodeKind::Enum => "enum",
            NodeKind::Trait => "trait",
            NodeKind::Type => "type",
            NodeKind::Const => "const",
            NodeKind::Static => "static",
            NodeKind::Macro => "macro",
            _ => return Err(RepoGraphError::Invalid("rust_item: kind".into())),
        };
        check_ident(krate, "crate")?;
        check_mod_path(mod_path)?;
        check_ident(name, "name")?;
        Self::parse(&format!(
            "rust:{prefix}:{}",
            join_path(krate, mod_path, name)
        ))
    }

    /// `rust:impl:<crate>::<mod path>::<SelfType>#<TraitPath|->`.
    pub fn rust_impl(
        krate: &str,
        mod_path: &str,
        self_type: &str,
        trait_path: Option<&str>,
    ) -> RepoGraphResult<NodeKey> {
        check_ident(krate, "crate")?;
        check_mod_path(mod_path)?;
        check_ident(self_type, "self_type")?;
        let tp = check_trait_path(trait_path)?;
        Self::parse(&format!(
            "rust:impl:{}#{tp}",
            join_path(krate, mod_path, self_type)
        ))
    }

    /// `rust:method:<crate>::<mod path>::<SelfType>#<TraitPath|->::<name>`.
    pub fn rust_method(
        krate: &str,
        mod_path: &str,
        self_type: &str,
        trait_path: Option<&str>,
        name: &str,
    ) -> RepoGraphResult<NodeKey> {
        check_ident(krate, "crate")?;
        check_mod_path(mod_path)?;
        check_ident(self_type, "self_type")?;
        let tp = check_trait_path(trait_path)?;
        check_ident(name, "name")?;
        Self::parse(&format!(
            "rust:method:{}#{tp}::{name}",
            join_path(krate, mod_path, self_type)
        ))
    }

    /// `rust:test:<crate>::<mod path>::<fn>[::<case>]`.
    pub fn rust_test(
        krate: &str,
        mod_path: &str,
        func: &str,
        case: Option<&str>,
    ) -> RepoGraphResult<NodeKey> {
        check_ident(krate, "crate")?;
        check_mod_path(mod_path)?;
        check_ident(func, "test_fn")?;
        let base = join_path(krate, mod_path, func);
        let full = match case {
            Some(c) => {
                check_ident(c, "test_case")?;
                format!("rust:test:{base}::{c}")
            }
            None => format!("rust:test:{base}"),
        };
        Self::parse(&full)
    }

    /// `go:package:<import path>`.
    pub fn go_package(import_path: &str) -> RepoGraphResult<NodeKey> {
        check_go_import(import_path)?;
        Self::parse(&format!("go:package:{import_path}"))
    }

    /// `go:<kind>:<import path>.<Name>` for `func`/`struct`/`interface`/`type`. The Go words
    /// `func` and `interface` map to node kinds `fn` and `trait`.
    pub fn go_item(kind: NodeKind, import_path: &str, name: &str) -> RepoGraphResult<NodeKey> {
        let word = match kind {
            NodeKind::Fn => "func",
            NodeKind::Struct => "struct",
            NodeKind::Trait => "interface",
            NodeKind::Type => "type",
            _ => return Err(RepoGraphError::Invalid("go_item: kind".into())),
        };
        check_go_import(import_path)?;
        check_ident(name, "name")?;
        Self::parse(&format!("go:{word}:{import_path}.{name}"))
    }

    /// `go:method:<import path>.<Recv>.<Name>`.
    pub fn go_method(import_path: &str, recv: &str, name: &str) -> RepoGraphResult<NodeKey> {
        check_go_import(import_path)?;
        check_ident(recv, "recv")?;
        check_ident(name, "name")?;
        Self::parse(&format!("go:method:{import_path}.{recv}.{name}"))
    }

    /// `go:test:<import path>.<TestName>`.
    pub fn go_test(import_path: &str, name: &str) -> RepoGraphResult<NodeKey> {
        check_go_import(import_path)?;
        check_ident(name, "name")?;
        Self::parse(&format!("go:test:{import_path}.{name}"))
    }

    /// `proto:service:<pkg>.<Svc>`.
    pub fn proto_service(pkg: &str, svc: &str) -> RepoGraphResult<NodeKey> {
        check_dotted(pkg, "proto_pkg")?;
        check_ident(svc, "proto_service")?;
        Self::parse(&format!("proto:service:{pkg}.{svc}"))
    }

    /// `proto:rpc:<pkg>.<Svc>/<Rpc>`.
    pub fn proto_rpc(pkg: &str, svc: &str, rpc: &str) -> RepoGraphResult<NodeKey> {
        check_dotted(pkg, "proto_pkg")?;
        check_ident(svc, "proto_service")?;
        check_ident(rpc, "proto_rpc")?;
        Self::parse(&format!("proto:rpc:{pkg}.{svc}/{rpc}"))
    }

    /// `proto:message:<pkg>.<Msg>`.
    pub fn proto_message(pkg: &str, msg: &str) -> RepoGraphResult<NodeKey> {
        check_dotted(pkg, "proto_pkg")?;
        check_ident(msg, "proto_message")?;
        Self::parse(&format!("proto:message:{pkg}.{msg}"))
    }

    /// `sql:table:<store>/<name>`.
    pub fn sql_table(store: &str, name: &str) -> RepoGraphResult<NodeKey> {
        check_ident(store, "sql_store")?;
        check_ident(name, "sql_table")?;
        Self::parse(&format!("sql:table:{store}/{name}"))
    }

    /// `cfg:<section>.<key>`.
    pub fn cfg(section: &str, key: &str) -> RepoGraphResult<NodeKey> {
        check_ident(section, "cfg_section")?;
        check_dotted(key, "cfg_key")?;
        Self::parse(&format!("cfg:{section}.{key}"))
    }

    /// `metric:<name>`.
    pub fn metric(name: &str) -> RepoGraphResult<NodeKey> {
        check_ident(name, "metric")?;
        Self::parse(&format!("metric:{name}"))
    }

    /// `span:<name>`.
    pub fn span(name: &str) -> RepoGraphResult<NodeKey> {
        check_ident(name, "span")?;
        Self::parse(&format!("span:{name}"))
    }
}

impl std::fmt::Display for NodeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw)
    }
}

impl Serialize for NodeKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.raw)
    }
}

impl<'de> Deserialize<'de> for NodeKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        NodeKey::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Derivations over a key
// ---------------------------------------------------------------------------

/// The content-addressed node id: the first 8 bytes of `sha256(node_key)`, big-endian, as an
/// `i64` (`01-schema.md`). Deterministic and independent of any store.
pub fn node_id_for(key: &NodeKey) -> NodeId {
    let digest = Sha256::digest(key.as_str().as_bytes());
    let mut first8 = [0u8; 8];
    first8.copy_from_slice(&digest[..8]);
    NodeId(i64::from_be_bytes(first8))
}

/// Whether `path` is a safe repo-relative path: non-empty, at most [`MAX_PATH_LEN`] bytes, not
/// absolute, no backslash / NUL / control char, and no empty / `.` / `..` component. Lexical
/// only — the extractor's file walk additionally `confine`s every path before opening it.
pub fn repo_relative(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return false;
    }
    if path.starts_with('/') {
        return false;
    }
    if path
        .bytes()
        .any(|b| b == b'\\' || b == 0 || b.is_ascii_control())
    {
        return false;
    }
    path.split('/')
        .all(|comp| !comp.is_empty() && comp != "." && comp != "..")
}

/// The lower-cased token split of a symbol name: split on `_`, `-`, `.`, and camel-case
/// boundaries, deduplicated in first-seen order, at most [`MAX_NAME_TOKENS`], each at most
/// [`MAX_NAME_TOKEN_LEN`] bytes. `PgDigests` → `[pg, digests]`; `HTTPServer` →
/// `[http, server]`. Non-ASCII bytes act as separators, so no non-ASCII token survives.
pub fn name_tokens(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    // First split on non-alphanumeric-ASCII (covers `_ - .` and drops non-ASCII), then camel.
    let flush = |word: &mut String, out: &mut Vec<String>| {
        if !word.is_empty() {
            split_camel(word, out);
            word.clear();
        }
    };
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
        }
    }
    flush(&mut word, &mut out);
    // Lower-case, drop over-long, dedup first-seen, cap.
    let mut seen: Vec<String> = Vec::new();
    for tok in out {
        let lower = tok.to_ascii_lowercase();
        if lower.len() > MAX_NAME_TOKEN_LEN || seen.iter().any(|s| s == &lower) {
            continue;
        }
        seen.push(lower);
        if seen.len() == MAX_NAME_TOKENS {
            break;
        }
    }
    seen
}

/// Split one ASCII-alphanumeric word on camel-case boundaries into `out`.
fn split_camel(word: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = word.chars().collect();
    let mut start = 0usize;
    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let cur = chars[i];
        let lower_to_upper =
            (prev.is_ascii_lowercase() || prev.is_ascii_digit()) && cur.is_ascii_uppercase();
        let acronym_end = prev.is_ascii_uppercase()
            && cur.is_ascii_uppercase()
            && i + 1 < chars.len()
            && chars[i + 1].is_ascii_lowercase();
        if lower_to_upper || acronym_end {
            out.push(chars[start..i].iter().collect());
            start = i;
        }
    }
    if start < chars.len() {
        out.push(chars[start..].iter().collect());
    }
}

// ---------------------------------------------------------------------------
// Segment validators (constructor helpers)
// ---------------------------------------------------------------------------

/// A Rust/Go identifier: non-empty, ≤ 128 bytes, first char `[A-Za-z_]`, rest `[A-Za-z0-9_]`.
fn check_ident(s: &str, field: &str) -> RepoGraphResult<()> {
    let ok = !s.is_empty()
        && s.len() <= 128
        && s.bytes().enumerate().all(|(i, b)| {
            if i == 0 {
                b.is_ascii_alphabetic() || b == b'_'
            } else {
                b.is_ascii_alphanumeric() || b == b'_'
            }
        });
    ok.then_some(())
        .ok_or_else(|| RepoGraphError::Invalid(field.to_string()))
}

/// A `::`-separated path of idents, possibly empty (crate root).
fn check_mod_path(s: &str) -> RepoGraphResult<()> {
    if s.is_empty() {
        return Ok(());
    }
    for seg in s.split("::") {
        check_ident(seg, "mod_path")?;
    }
    Ok(())
}

/// A `.`-separated path of idents (proto package, config key), non-empty.
fn check_dotted(s: &str, field: &str) -> RepoGraphResult<()> {
    if s.is_empty() {
        return Err(RepoGraphError::Invalid(field.to_string()));
    }
    for seg in s.split('.') {
        check_ident(seg, field)?;
    }
    Ok(())
}

/// A cargo feature name: non-empty, ≤ 128 bytes, `[A-Za-z0-9_-]`, no leading `-`.
fn check_feature(s: &str) -> RepoGraphResult<()> {
    let ok = !s.is_empty()
        && s.len() <= 128
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then_some(())
        .ok_or_else(|| RepoGraphError::Invalid("feature".into()))
}

/// A Go import path: non-empty, ≤ 256 bytes, `[A-Za-z0-9_./-]`, no `..` component, not absolute.
fn check_go_import(s: &str) -> RepoGraphResult<()> {
    let charset_ok = !s.is_empty()
        && s.len() <= 256
        && !s.starts_with('/')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'));
    let comps_ok = s.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
    (charset_ok && comps_ok)
        .then_some(())
        .ok_or_else(|| RepoGraphError::Invalid("go_import".into()))
}

/// The trait side of an impl / method key: `None` renders as `-`; otherwise a `::` path of
/// idents.
fn check_trait_path(trait_path: Option<&str>) -> RepoGraphResult<String> {
    match trait_path {
        None => Ok("-".to_string()),
        Some(tp) => {
            if tp.is_empty() {
                return Err(RepoGraphError::Invalid("trait_path".into()));
            }
            for seg in tp.split("::") {
                check_ident(seg, "trait_path")?;
            }
            Ok(tp.to_string())
        }
    }
}

/// Join `<crate>[::<mod path>][::<tail>]`, skipping empty parts.
fn join_path(krate: &str, mod_path: &str, tail: &str) -> String {
    let mut s = String::from(krate);
    if !mod_path.is_empty() {
        s.push_str("::");
        s.push_str(mod_path);
    }
    if !tail.is_empty() {
        s.push_str("::");
        s.push_str(tail);
    }
    s
}

// ===========================================================================
// R1 — key grammar, ids, tokens (docs/design/repo-knowledge/08-test-matrix.md)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // -- positive_parse_<form>: every grammar row ---------------------------

    #[rstest]
    #[case::repo("repo:randomizedcoder__agent-seddon", NodeKind::Repo, Lang::None)]
    #[case::file("file:crates/agent-core/src/security.rs", NodeKind::File, Lang::None)]
    #[case::doc("doc:docs/extending.md", NodeKind::Doc, Lang::Md)]
    #[case::rust_crate("rust:crate:agent_core", NodeKind::Crate, Lang::Rust)]
    #[case::rust_feature("rust:feature:agent_digest/postgres", NodeKind::Feature, Lang::Rust)]
    #[case::rust_mod("rust:mod:agent_core::security", NodeKind::Module, Lang::Rust)]
    #[case::rust_fn("rust:fn:agent_core::security::confine", NodeKind::Fn, Lang::Rust)]
    #[case::rust_impl_trait(
        "rust:impl:agent_digest::postgres::PgDigests#DigestStore",
        NodeKind::Impl,
        Lang::Rust
    )]
    #[case::rust_impl_inherent("rust:impl:agent_ast::graph::Graph#-", NodeKind::Impl, Lang::Rust)]
    #[case::rust_method(
        "rust:method:agent_digest::postgres::PgDigests#DigestStore::put",
        NodeKind::Method,
        Lang::Rust
    )]
    #[case::rust_test_case(
        "rust:test:agent_tools::edit::tests::rejects_traversal::adversarial_dotdot",
        NodeKind::Test,
        Lang::Rust
    )]
    #[case::go_package("go:package:example.com/m/pkg", NodeKind::Package, Lang::Go)]
    #[case::go_func("go:func:example.com/m/pkg.Serve", NodeKind::Fn, Lang::Go)]
    #[case::go_method("go:method:example.com/m/pkg.Server.Close", NodeKind::Method, Lang::Go)]
    #[case::go_test("go:test:example.com/m/pkg.TestServe", NodeKind::Test, Lang::Go)]
    #[case::proto_rpc("proto:rpc:agent.v1.Search/Query", NodeKind::ProtoRpc, Lang::Proto)]
    #[case::sql_table("sql:table:digests/digests", NodeKind::Table, Lang::Sql)]
    #[case::cfg("cfg:review.nearby", NodeKind::ConfigKey, Lang::Toml)]
    #[case::metric("metric:agent_repo_graph_index_seconds", NodeKind::Metric, Lang::None)]
    #[case::span("span:agent.review.tick", NodeKind::Span, Lang::None)]
    fn positive_parse_form(#[case] key: &str, #[case] kind: NodeKind, #[case] lang: Lang) {
        let parsed = NodeKey::parse(key).expect("valid key");
        assert_eq!(parsed.as_str(), key);
        assert_eq!(parsed.kind(), kind);
        assert_eq!(parsed.lang(), lang);
    }

    // -- positive_constructor_round_trip -----------------------------------

    #[rstest]
    #[case::repo(NodeKey::repo("agent_seddon"), NodeKind::Repo)]
    #[case::file(NodeKey::file("crates/agent-core/src/lib.rs"), NodeKind::File)]
    #[case::doc(NodeKey::doc("docs/extending.md"), NodeKind::Doc)]
    #[case::rust_crate(NodeKey::rust_crate("agent_core"), NodeKind::Crate)]
    #[case::rust_feature(NodeKey::rust_feature("agent_digest", "postgres"), NodeKind::Feature)]
    #[case::rust_mod(NodeKey::rust_mod("agent_core", "security"), NodeKind::Module)]
    #[case::rust_mod_root(NodeKey::rust_mod("agent_core", ""), NodeKind::Module)]
    #[case::rust_fn(
        NodeKey::rust_item(NodeKind::Fn, "agent_core", "security", "confine"),
        NodeKind::Fn
    )]
    #[case::rust_struct(
        NodeKey::rust_item(NodeKind::Struct, "agent_ast", "graph", "Graph"),
        NodeKind::Struct
    )]
    #[case::rust_impl_trait(
        NodeKey::rust_impl("agent_digest", "postgres", "PgDigests", Some("DigestStore")),
        NodeKind::Impl
    )]
    #[case::rust_impl_inherent(
        NodeKey::rust_impl("agent_ast", "graph", "Graph", None),
        NodeKind::Impl
    )]
    #[case::rust_method(
        NodeKey::rust_method("agent_digest", "postgres", "PgDigests", Some("DigestStore"), "put"),
        NodeKind::Method
    )]
    #[case::rust_test(
        NodeKey::rust_test("agent_tools", "edit::tests", "rejects", Some("adversarial_dotdot")),
        NodeKind::Test
    )]
    #[case::rust_test_no_case(
        NodeKey::rust_test("agent_tools", "edit", "helper", None),
        NodeKind::Test
    )]
    #[case::go_package(NodeKey::go_package("example.com/m/pkg"), NodeKind::Package)]
    #[case::go_func(
        NodeKey::go_item(NodeKind::Fn, "example.com/m/pkg", "Serve"),
        NodeKind::Fn
    )]
    #[case::go_interface(
        NodeKey::go_item(NodeKind::Trait, "example.com/m/pkg", "Store"),
        NodeKind::Trait
    )]
    #[case::go_method(
        NodeKey::go_method("example.com/m/pkg", "Server", "Close"),
        NodeKind::Method
    )]
    #[case::go_test(NodeKey::go_test("example.com/m/pkg", "TestServe"), NodeKind::Test)]
    #[case::proto_service(NodeKey::proto_service("agent.v1", "Search"), NodeKind::ProtoService)]
    #[case::proto_rpc(NodeKey::proto_rpc("agent.v1", "Search", "Query"), NodeKind::ProtoRpc)]
    #[case::proto_message(
        NodeKey::proto_message("agent.v1", "SearchRequest"),
        NodeKind::ProtoMessage
    )]
    #[case::sql_table(NodeKey::sql_table("digests", "digests"), NodeKind::Table)]
    #[case::cfg(NodeKey::cfg("review", "nearby"), NodeKind::ConfigKey)]
    #[case::metric(NodeKey::metric("agent_repo_graph_index_seconds"), NodeKind::Metric)]
    #[case::span(NodeKey::span("tick"), NodeKind::Span)]
    fn positive_constructor_round_trip(
        #[case] built: RepoGraphResult<NodeKey>,
        #[case] kind: NodeKind,
    ) {
        let built = built.expect("constructor accepts valid parts");
        assert_eq!(built.kind(), kind);
        let reparsed = NodeKey::parse(built.as_str()).expect("built key re-parses");
        assert_eq!(reparsed, built);
        assert_eq!(reparsed.kind(), kind);
    }

    #[test]
    fn positive_dup_suffix() {
        let k = NodeKey::parse("rust:fn:a::b::f@0123abcd").expect("dup-suffixed key");
        assert_eq!(k.kind(), NodeKind::Fn);
        assert_eq!(k.as_str(), "rust:fn:a::b::f@0123abcd");
    }

    // -- negatives ---------------------------------------------------------

    #[rstest]
    #[case::unknown_prefix("weird:thing")]
    #[case::unknown_rust_kind("rust:widget:a::b")]
    #[case::empty("")]
    #[case::prefix_only("rust:fn:")]
    #[case::rust_colon_only("rust:")]
    #[case::bare_word("confine")]
    fn negative_reject(#[case] key: &str) {
        assert!(
            NodeKey::parse(key).is_err(),
            "expected rejection for {key:?}"
        );
    }

    // -- corner: Go word → node kind ---------------------------------------

    #[test]
    fn corner_go_interface_is_trait() {
        assert_eq!(
            NodeKey::parse("go:interface:p.I").unwrap().kind(),
            NodeKind::Trait
        );
    }

    #[test]
    fn corner_go_func_is_fn() {
        assert_eq!(NodeKey::parse("go:func:p.F").unwrap().kind(), NodeKind::Fn);
    }

    // -- boundaries --------------------------------------------------------

    #[test]
    fn boundary_key_512() {
        let key = format!("metric:{}", "a".repeat(MAX_NODE_KEY_LEN - "metric:".len()));
        assert_eq!(key.len(), MAX_NODE_KEY_LEN);
        assert!(NodeKey::parse(&key).is_ok());
    }

    #[test]
    fn boundary_key_513() {
        let key = format!(
            "metric:{}",
            "a".repeat(MAX_NODE_KEY_LEN - "metric:".len() + 1)
        );
        assert_eq!(key.len(), MAX_NODE_KEY_LEN + 1);
        assert!(matches!(
            NodeKey::parse(&key),
            Err(RepoGraphError::TooLong(_))
        ));
    }

    // -- adversarial: charset ----------------------------------------------

    #[rstest]
    #[case::whitespace("rust:fn:a::b c")]
    #[case::tab("rust:fn:a\t::b")]
    #[case::control_char("rust:fn:a\u{0001}b")]
    #[case::non_ascii("rust:fn:café")]
    #[case::bidi("rust:fn:a\u{202e}b")]
    #[case::bad_dup_suffix("rust:fn:a::b::f@zz")]
    #[case::double_at("rust:fn:a@0123abcd@0123abcd")]
    #[case::uppercase_dup("rust:fn:a::b::f@0123ABCD")]
    fn adversarial_charset(#[case] key: &str) {
        let err = NodeKey::parse(key).expect_err("must reject");
        // The message names the rule, not the input.
        assert!(!err.to_string().contains("café"));
    }

    // -- adversarial: file / doc paths -------------------------------------

    #[rstest]
    #[case::traversal("file:../x")]
    #[case::absolute("file:/etc/passwd")]
    #[case::backslash("file:a\\b")]
    #[case::dot_component("file:a/./b")]
    #[case::empty_component("file:a//b")]
    #[case::dotdot_inner("doc:a/../b")]
    fn adversarial_file_path(#[case] key: &str) {
        assert!(
            NodeKey::parse(key).is_err(),
            "expected rejection for {key:?}"
        );
    }

    #[rstest]
    #[case::ok("crates/agent-core/src/lib.rs", true)]
    #[case::empty("", false)]
    #[case::absolute("/etc/passwd", false)]
    #[case::traversal("../x", false)]
    #[case::dot("a/./b", false)]
    #[case::empty_comp("a//b", false)]
    #[case::backslash("a\\b", false)]
    #[case::nul("a\0b", false)]
    fn repo_relative_rows(#[case] path: &str, #[case] want: bool) {
        assert_eq!(repo_relative(path), want);
    }

    // -- adversarial: constructor segment validation -----------------------

    #[test]
    fn adversarial_constructor_segment_colon() {
        assert!(NodeKey::rust_crate("a:b").is_err());
    }

    #[test]
    fn adversarial_constructor_segment_whitespace() {
        assert!(NodeKey::rust_item(NodeKind::Fn, "agent_core", "m", "a b").is_err());
    }

    #[test]
    fn adversarial_constructor_crate_dash() {
        // A cargo package name with `-` must be the crate identifier (`_`), not the dashed name.
        assert!(NodeKey::rust_crate("agent-core").is_err());
        assert!(NodeKey::rust_crate("agent_core").is_ok());
    }

    #[test]
    fn adversarial_constructor_rust_item_bad_kind() {
        assert!(NodeKey::rust_item(NodeKind::Impl, "a", "m", "X").is_err());
    }

    // -- ids ---------------------------------------------------------------

    #[test]
    fn positive_id_known_vector() {
        let k = NodeKey::parse("file:a").unwrap();
        assert_eq!(node_id_for(&k), NodeId(-7_438_173_575_100_718_603));
    }

    #[test]
    fn positive_id_deterministic() {
        let k = NodeKey::parse("rust:fn:agent_core::security::confine").unwrap();
        assert_eq!(node_id_for(&k), node_id_for(&k));
    }

    #[test]
    fn corner_id_high_bit_negative() {
        // `file:a`'s sha256 has its top bit set, so the id is negative and still round-trips.
        let k = NodeKey::parse("file:a").unwrap();
        assert!(node_id_for(&k).0 < 0);
    }

    // -- name_tokens -------------------------------------------------------

    #[rstest]
    #[case::snake("find_changed_callers", vec!["find", "changed", "callers"])]
    #[case::camel("findChangedCallers", vec!["find", "changed", "callers"])]
    #[case::acronym("HTTPServer", vec!["http", "server"])]
    #[case::mixed("PgDigests", vec!["pg", "digests"])]
    #[case::dotted("agent.v1.Search", vec!["agent", "v1", "search"])]
    fn positive_tokens(#[case] name: &str, #[case] want: Vec<&str>) {
        assert_eq!(name_tokens(name), want);
    }

    #[test]
    fn corner_tokens_dedup() {
        // Repeated tokens collapse in first-seen order.
        assert_eq!(name_tokens("get_get_value"), vec!["get", "value"]);
    }

    #[test]
    fn boundary_tokens_16() {
        // 17 distinct parts collapse to the first 16.
        let name = (0..17)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join("_");
        assert_eq!(name_tokens(&name).len(), MAX_NAME_TOKENS);
    }

    #[test]
    fn adversarial_tokens_huge_name() {
        // A 10 KiB single word exceeds the per-token cap and is dropped, not stored.
        let name = "a".repeat(10 * 1024);
        assert!(name_tokens(&name).is_empty());
    }

    #[test]
    fn adversarial_tokens_unicode() {
        // Non-ASCII acts as a separator, so no non-ASCII token survives.
        let toks = name_tokens("caféLatte");
        assert!(toks.iter().all(|t| t.is_ascii()));
        assert_eq!(toks, vec!["caf", "latte"]);
    }
}
