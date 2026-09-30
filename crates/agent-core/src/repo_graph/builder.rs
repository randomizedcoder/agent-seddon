//! [`GraphBuilder`] — accumulate nodes and edges, validate every field, resolve duplicate keys,
//! and at [`GraphBuilder::finish`] sort, assign ids, detect id collisions and compute the
//! `graph_hash` (`02-extraction.md`).
//!
//! Everything an extractor emits is untrusted (it is derived from repo content), so each field
//! is checked at insert: a bad field drops the node / edge and is counted, never echoed; a cap
//! truncates; a duplicate key that a `#[cfg]` split does not explain, or a node-id collision,
//! fails the whole build so a corrupt snapshot is never written.

use super::key::node_id_for;
use super::model::{Edge, Node, NodeRecord, NodeVersion, RepoGraph};
use super::{name_tokens, repo_relative, EdgeKind, NodeId, NodeKey};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The maximum length of a node `name`, in bytes (`graph_nodes.name` CHECK upper bound).
pub const MAX_NAME_LEN: usize = 256;
/// The maximum length of a `qualifier`, in bytes.
pub const MAX_QUALIFIER_LEN: usize = 512;
/// The maximum serialized size of a node's `attrs`, in bytes (`graph_node_versions` CHECK).
pub const MAX_ATTRS_BYTES: usize = 4096;
/// The maximum serialized size of an edge's `attrs`, in bytes (`graph_edges` CHECK).
pub const MAX_EDGE_ATTRS_BYTES: usize = 1024;

/// Caps on how large a single graph may grow before inserts are dropped. [`Default`] is the
/// design's numbers (`02-extraction.md`).
#[derive(Debug, Clone, Copy)]
pub struct BuilderCaps {
    pub max_nodes: usize,
    pub max_edges: usize,
}

impl Default for BuilderCaps {
    fn default() -> Self {
        Self {
            max_nodes: 250_000,
            max_edges: 2_000_000,
        }
    }
}

/// An extractor's description of one node to add.
#[derive(Debug, Clone)]
pub struct NodeSpec {
    pub key: NodeKey,
    pub name: String,
    pub qualifier: String,
    pub file: String,
    pub line_start: i32,
    pub line_end: i32,
    pub sig_hash: String,
    pub body_hash: String,
    pub exported: bool,
    /// A JSON object (or null for none); validated to be an object of ≤ [`MAX_ATTRS_BYTES`].
    pub attrs: Value,
    /// The item's `#[cfg]` tokens; when two items share a key with different cfg, the later is
    /// suffixed by their sorted hash.
    pub cfg: Vec<String>,
}

impl NodeSpec {
    /// A spec with `name` and every other field defaulted (empty / zero / no attrs).
    pub fn new(key: NodeKey, name: impl Into<String>) -> Self {
        Self {
            key,
            name: name.into(),
            qualifier: String::new(),
            file: String::new(),
            line_start: 0,
            line_end: 0,
            sig_hash: String::new(),
            body_hash: String::new(),
            exported: false,
            attrs: Value::Object(Map::new()),
            cfg: Vec::new(),
        }
    }

    /// Set the defining file.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }

    /// Set the line span.
    pub fn with_lines(mut self, start: i32, end: i32) -> Self {
        self.line_start = start;
        self.line_end = end;
        self
    }

    /// Set the signature hash.
    pub fn with_sig(mut self, sig_hash: impl Into<String>) -> Self {
        self.sig_hash = sig_hash.into();
        self
    }

    /// Set the body hash.
    pub fn with_body(mut self, body_hash: impl Into<String>) -> Self {
        self.body_hash = body_hash.into();
        self
    }

    /// Set the qualifier (crate::mod path, package path, or dir).
    pub fn with_qualifier(mut self, qualifier: impl Into<String>) -> Self {
        self.qualifier = qualifier.into();
        self
    }

    /// Set the exported flag.
    pub fn with_exported(mut self, exported: bool) -> Self {
        self.exported = exported;
        self
    }

    /// Set the attributes object.
    pub fn with_attrs(mut self, attrs: Value) -> Self {
        self.attrs = attrs;
        self
    }

    /// Set the `#[cfg]` tokens.
    pub fn with_cfg(mut self, cfg: Vec<String>) -> Self {
        self.cfg = cfg;
        self
    }
}

/// Why an insert was dropped. `Display` names the field only — it **never** echoes the offending
/// value (repo content is untrusted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// `name` outside `1..=256` bytes.
    Name,
    /// `qualifier` over 512 bytes.
    Qualifier,
    /// `attrs` not a JSON object, or over the size cap.
    Attrs,
    /// `file` non-empty and not a repo-relative path.
    File,
    /// The node cap was reached.
    NodeCap,
    /// The edge cap was reached.
    EdgeCap,
    /// A key + cfg already inserted (an unexplained duplicate); also fails `finish`.
    DuplicateKey,
    /// An edge whose endpoint key is absent from the graph at `finish`.
    Endpoint,
    /// An edge's `attrs` not a JSON object, or over the size cap.
    EdgeAttrs,
}

impl std::fmt::Display for DropReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DropReason::Name => "name length",
            DropReason::Qualifier => "qualifier length",
            DropReason::Attrs => "attrs (object / size)",
            DropReason::File => "file path",
            DropReason::NodeCap => "node cap",
            DropReason::EdgeCap => "edge cap",
            DropReason::DuplicateKey => "duplicate key",
            DropReason::Endpoint => "edge endpoint",
            DropReason::EdgeAttrs => "edge attrs (object / size)",
        };
        f.write_str(s)
    }
}

/// What [`GraphBuilder::node`] did with a spec.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeOutcome {
    /// Inserted under its own key.
    Added(NodeKey),
    /// Inserted under a `@<sha8>` cfg-duplicate suffix.
    Suffixed(NodeKey),
    /// Rejected; the reason names the field.
    Dropped(DropReason),
}

/// What [`GraphBuilder::edge`] did with an edge.
#[derive(Debug, Clone, PartialEq)]
pub enum EdgeOutcome {
    /// Accepted (endpoints resolved at `finish`).
    Added,
    /// Rejected at insert (bad attrs or over the cap).
    Dropped(DropReason),
}

/// A build that failed validation; the snapshot must not be written.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildError {
    /// Two items produced the same key with the same cfg — the extractor is wrong.
    DuplicateKey(NodeKey),
    /// Two distinct keys hashed to one node id.
    IdCollision(NodeId),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::DuplicateKey(k) => write!(f, "duplicate key: {k}"),
            BuildError::IdCollision(id) => write!(f, "node id collision: {id}"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Counts from a build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildReport {
    pub nodes: usize,
    pub edges: usize,
    pub dropped_nodes: usize,
    pub dropped_edges: usize,
    pub suffixed: usize,
    pub truncated: bool,
}

/// The output of [`GraphBuilder::finish`].
#[derive(Debug, Clone)]
pub struct Built {
    pub graph: RepoGraph,
    pub report: BuildReport,
}

struct PendingNode {
    base_key: NodeKey,
    final_key: NodeKey,
    name: String,
    qualifier: String,
    file: String,
    line_start: i32,
    line_end: i32,
    sig_hash: String,
    body_hash: String,
    exported: bool,
    attrs: Map<String, Value>,
}

struct PendingEdge {
    kind: EdgeKind,
    src: NodeKey,
    dst: NodeKey,
    weight: f32,
    attrs: Map<String, Value>,
}

type IdFn = Box<dyn Fn(&NodeKey) -> NodeId + Send + Sync>;

/// Accumulates nodes and edges and validates them into an immutable [`RepoGraph`].
pub struct GraphBuilder {
    caps: BuilderCaps,
    id_fn: IdFn,
    nodes: BTreeMap<String, PendingNode>,
    seen_sigs: HashSet<String>,
    edges: Vec<PendingEdge>,
    duplicate: Option<NodeKey>,
    dropped_nodes: usize,
    dropped_edges: usize,
    suffixed: usize,
    truncated: bool,
}

impl std::fmt::Debug for GraphBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphBuilder")
            .field("caps", &self.caps)
            .field("nodes", &self.nodes.len())
            .field("edges", &self.edges.len())
            .field("truncated", &self.truncated)
            .finish()
    }
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new(BuilderCaps::default())
    }
}

impl GraphBuilder {
    /// A new builder with the given caps.
    pub fn new(caps: BuilderCaps) -> Self {
        Self {
            caps,
            id_fn: Box::new(node_id_for),
            nodes: BTreeMap::new(),
            seen_sigs: HashSet::new(),
            edges: Vec::new(),
            duplicate: None,
            dropped_nodes: 0,
            dropped_edges: 0,
            suffixed: 0,
            truncated: false,
        }
    }

    /// Swap the id function — for tests that need to force a collision. Hidden, not `cfg(test)`,
    /// so the testkit conformance fixtures can reach it.
    #[doc(hidden)]
    pub fn with_id_fn<F>(mut self, f: F) -> Self
    where
        F: Fn(&NodeKey) -> NodeId + Send + Sync + 'static,
    {
        self.id_fn = Box::new(f);
        self
    }

    /// Add a node. Validates every field; returns what it did (added, cfg-suffixed, or dropped
    /// with the field named).
    pub fn node(&mut self, spec: NodeSpec) -> NodeOutcome {
        // -- field validation (drop, count, never echo) --------------------
        if spec.name.is_empty() || spec.name.len() > MAX_NAME_LEN {
            return self.drop_node(DropReason::Name);
        }
        if spec.qualifier.len() > MAX_QUALIFIER_LEN {
            return self.drop_node(DropReason::Qualifier);
        }
        if !spec.file.is_empty() && !repo_relative(&spec.file) {
            return self.drop_node(DropReason::File);
        }
        let mut attrs = match as_object(&spec.attrs) {
            Some(m) if serialized_len(&m) <= MAX_ATTRS_BYTES => m,
            _ => return self.drop_node(DropReason::Attrs),
        };

        // -- cap -----------------------------------------------------------
        if self.nodes.len() >= self.caps.max_nodes {
            self.truncated = true;
            return self.drop_node(DropReason::NodeCap);
        }

        // -- lines clamp ---------------------------------------------------
        let line_start = spec.line_start.max(0);
        let line_end = spec.line_end.max(line_start);

        // -- duplicate / cfg-split -----------------------------------------
        let mut cfg = spec.cfg.clone();
        cfg.sort();
        let base = spec.key.as_str().to_string();
        let sig = format!("{base}\u{1f}{}", cfg.join("\u{1e}"));
        if self.seen_sigs.contains(&sig) {
            // Same key + same cfg: an unexplained duplicate. Fails `finish`.
            self.duplicate.get_or_insert_with(|| spec.key.clone());
            return self.drop_node(DropReason::DuplicateKey);
        }

        let (final_key, suffixed) = if self.nodes.contains_key(&base) {
            // A later item with a different cfg: suffix by the sorted-cfg hash.
            let sha8 = sha8_of(cfg.join("\u{1e}").as_bytes());
            let candidate = format!("{base}@{sha8}");
            match NodeKey::parse(&candidate) {
                Ok(k) if !self.nodes.contains_key(k.as_str()) => (k, true),
                _ => {
                    // A sha8 collision (or an unparseable suffix): treat as an unexplained dup.
                    self.duplicate.get_or_insert_with(|| spec.key.clone());
                    return self.drop_node(DropReason::DuplicateKey);
                }
            }
        } else {
            (spec.key.clone(), false)
        };
        if suffixed {
            attrs.insert("dup".to_string(), Value::Bool(true));
            attrs.insert(
                "cfg".to_string(),
                Value::Array(cfg.iter().map(|c| Value::String(c.clone())).collect()),
            );
            self.suffixed += 1;
        }

        self.seen_sigs.insert(sig);
        let outcome_key = final_key.clone();
        self.nodes.insert(
            final_key.as_str().to_string(),
            PendingNode {
                base_key: spec.key,
                final_key,
                name: spec.name,
                qualifier: spec.qualifier,
                file: spec.file,
                line_start,
                line_end,
                sig_hash: spec.sig_hash,
                body_hash: spec.body_hash,
                exported: spec.exported,
                attrs,
            },
        );
        if suffixed {
            NodeOutcome::Suffixed(outcome_key)
        } else {
            NodeOutcome::Added(outcome_key)
        }
    }

    /// Add an edge. Clamps a non-finite / negative `weight` to `1.0`, validates `attrs`, and
    /// enforces the edge cap; a dangling endpoint is dropped at [`GraphBuilder::finish`].
    pub fn edge(
        &mut self,
        kind: EdgeKind,
        src: &NodeKey,
        dst: &NodeKey,
        weight: f32,
        attrs: Value,
    ) -> EdgeOutcome {
        let attrs = match as_object(&attrs) {
            Some(m) if serialized_len(&m) <= MAX_EDGE_ATTRS_BYTES => m,
            _ => {
                self.dropped_edges += 1;
                return EdgeOutcome::Dropped(DropReason::EdgeAttrs);
            }
        };
        if self.edges.len() >= self.caps.max_edges {
            self.truncated = true;
            self.dropped_edges += 1;
            return EdgeOutcome::Dropped(DropReason::EdgeCap);
        }
        let weight = if weight.is_finite() && weight >= 0.0 {
            weight
        } else {
            1.0
        };
        self.edges.push(PendingEdge {
            kind,
            src: src.clone(),
            dst: dst.clone(),
            weight,
            attrs,
        });
        EdgeOutcome::Added
    }

    /// Finish the build: fail on an unexplained duplicate or an id collision, else sort, assign
    /// ids, drop dangling edges, and compute the `graph_hash`.
    pub fn finish(self) -> Result<Built, BuildError> {
        if let Some(k) = self.duplicate {
            return Err(BuildError::DuplicateKey(k));
        }

        // Assign ids and detect collisions.
        let mut key_to_id: HashMap<String, NodeId> = HashMap::with_capacity(self.nodes.len());
        let mut id_to_key: HashMap<NodeId, String> = HashMap::with_capacity(self.nodes.len());
        let mut records: Vec<NodeRecord> = Vec::with_capacity(self.nodes.len());
        for pending in self.nodes.into_values() {
            let id = (self.id_fn)(&pending.final_key);
            if let Some(existing) = id_to_key.get(&id) {
                if existing != pending.final_key.as_str() {
                    return Err(BuildError::IdCollision(id));
                }
            }
            id_to_key.insert(id, pending.final_key.as_str().to_string());
            key_to_id.insert(pending.final_key.as_str().to_string(), id);
            let name_tokens = name_tokens(&pending.name);
            records.push(NodeRecord {
                node: Node {
                    key: pending.final_key.clone(),
                    id,
                    kind: pending.base_key.kind(),
                    lang: pending.base_key.lang(),
                    name: pending.name,
                    name_tokens,
                    qualifier: pending.qualifier,
                },
                version: NodeVersion {
                    file: pending.file,
                    line_start: pending.line_start,
                    line_end: pending.line_end,
                    sig_hash: pending.sig_hash,
                    body_hash: pending.body_hash,
                    exported: pending.exported,
                    attrs: pending.attrs,
                },
            });
        }

        // Resolve edges; drop dangling; dedup exact repeats.
        let mut dropped_edges = self.dropped_edges;
        let mut seen_edges: HashSet<(EdgeKind, NodeId, NodeId)> = HashSet::new();
        let mut edges: Vec<Edge> = Vec::with_capacity(self.edges.len());
        for pe in self.edges {
            let (src_id, dst_id) = match (
                key_to_id.get(pe.src.as_str()),
                key_to_id.get(pe.dst.as_str()),
            ) {
                (Some(&s), Some(&d)) => (s, d),
                _ => {
                    dropped_edges += 1;
                    continue;
                }
            };
            if !seen_edges.insert((pe.kind, src_id, dst_id)) {
                continue;
            }
            edges.push(Edge {
                kind: pe.kind,
                src: pe.src,
                dst: pe.dst,
                src_id,
                dst_id,
                weight: pe.weight,
                attrs: pe.attrs,
            });
        }

        // Deterministic order: nodes by key, edges by (kind, src_key, dst_key).
        records.sort_by(|a, b| a.node.key.cmp(&b.node.key));
        edges.sort_by(|a, b| {
            a.kind
                .as_str()
                .cmp(b.kind.as_str())
                .then_with(|| a.src.cmp(&b.src))
                .then_with(|| a.dst.cmp(&b.dst))
        });

        let graph_hash = compute_graph_hash(&records, &edges);
        let report = BuildReport {
            nodes: records.len(),
            edges: edges.len(),
            dropped_nodes: self.dropped_nodes,
            dropped_edges,
            suffixed: self.suffixed,
            truncated: self.truncated,
        };
        Ok(Built {
            graph: RepoGraph {
                nodes: records,
                edges,
                graph_hash,
                truncated: self.truncated,
            },
            report,
        })
    }

    fn drop_node(&mut self, reason: DropReason) -> NodeOutcome {
        self.dropped_nodes += 1;
        NodeOutcome::Dropped(reason)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `Value::Object` → its map, `Value::Null` → an empty map, anything else → `None`.
fn as_object(v: &Value) -> Option<Map<String, Value>> {
    match v {
        Value::Object(m) => Some(m.clone()),
        Value::Null => Some(Map::new()),
        _ => None,
    }
}

/// The serialized byte length of an attrs object (the `pg_column_size` proxy).
fn serialized_len(m: &Map<String, Value>) -> usize {
    serde_json::to_vec(m).map(|v| v.len()).unwrap_or(usize::MAX)
}

/// The first 4 bytes of `sha256(bytes)` as 8 lowercase hex digits — the cfg-duplicate suffix.
fn sha8_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex(&digest[..4])
}

/// Lowercase hex of a byte slice.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// The snapshot `graph_hash`: `sha256` over the sorted `(node_key, kind, sig_hash, body_hash)`
/// tuples then the sorted `(kind, src_key, dst_key)` tuples, each field NUL-separated. Ignores
/// names, qualifiers, lines, attrs, weights and ids — only the content identity.
fn compute_graph_hash(nodes: &[NodeRecord], edges: &[Edge]) -> String {
    let mut hasher = Sha256::new();
    for rec in nodes {
        hasher.update(rec.node.key.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(rec.node.kind.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(rec.version.sig_hash.as_bytes());
        hasher.update([0]);
        hasher.update(rec.version.body_hash.as_bytes());
        hasher.update([0]);
    }
    for e in edges {
        hasher.update(e.kind.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(e.src.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(e.dst.as_str().as_bytes());
        hasher.update([0]);
    }
    hex(&hasher.finalize())
}

// ===========================================================================
// R2 — GraphBuilder + validation (docs/design/repo-knowledge/08-test-matrix.md)
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_graph::{EdgeKind, NodeId, NodeKey};
    use rstest::rstest;
    use serde_json::json;

    /// `sha256("")` — the `graph_hash` of an empty graph.
    const EMPTY_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn fn_key(name: &str) -> NodeKey {
        NodeKey::rust_item(crate::repo_graph::NodeKind::Fn, "c", "m", name).expect("fn key")
    }

    /// A JSON object that serializes to exactly `total` bytes (`{"k":"<pad>"}` = 8 + padding).
    fn attrs_of_size(total: usize) -> serde_json::Value {
        json!({ "k": "a".repeat(total - 8) })
    }

    // -- ordering & hashing -------------------------------------------------

    #[test]
    fn positive_sorted_output() {
        let mut b = GraphBuilder::default();
        for name in ["gamma", "alpha", "beta"] {
            assert!(matches!(
                b.node(NodeSpec::new(fn_key(name), name)),
                NodeOutcome::Added(_)
            ));
        }
        // Edges out of order and of two kinds.
        b.edge(EdgeKind::Calls, &fn_key("alpha"), &fn_key("gamma"), 1.0, json!({}));
        b.edge(EdgeKind::Calls, &fn_key("alpha"), &fn_key("beta"), 1.0, json!({}));
        b.edge(EdgeKind::Contains, &fn_key("beta"), &fn_key("gamma"), 1.0, json!({}));
        let built = b.finish().expect("valid graph");
        let keys: Vec<&str> = built.graph.nodes().iter().map(|r| r.node.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
        let edge_tuples: Vec<(&str, &str, &str)> = built
            .graph
            .edges()
            .iter()
            .map(|e| (e.kind.as_str(), e.src.as_str(), e.dst.as_str()))
            .collect();
        let mut sorted_edges = edge_tuples.clone();
        sorted_edges.sort_unstable();
        assert_eq!(edge_tuples, sorted_edges);
    }

    fn build_two_nodes(order: [&str; 2], body_a: &str, sig_a: &str) -> String {
        let mut b = GraphBuilder::default();
        for name in order {
            let (body, sig) = if name == "a" { (body_a, sig_a) } else { ("bb", "bs") };
            b.node(
                NodeSpec::new(fn_key(name), name)
                    .with_body(body)
                    .with_sig(sig),
            );
        }
        b.finish().unwrap().graph.graph_hash().to_string()
    }

    #[test]
    fn positive_hash_deterministic() {
        assert_eq!(
            build_two_nodes(["a", "b"], "ba", "sa"),
            build_two_nodes(["b", "a"], "ba", "sa")
        );
    }

    #[test]
    fn positive_hash_tracks_body() {
        assert_ne!(
            build_two_nodes(["a", "b"], "ba", "sa"),
            build_two_nodes(["a", "b"], "DIFFERENT", "sa")
        );
    }

    #[test]
    fn positive_hash_tracks_sig() {
        assert_ne!(
            build_two_nodes(["a", "b"], "ba", "sa"),
            build_two_nodes(["a", "b"], "ba", "DIFFERENT")
        );
    }

    #[test]
    fn corner_hash_ignores_attrs_and_lines() {
        let base = {
            let mut b = GraphBuilder::default();
            b.node(NodeSpec::new(fn_key("a"), "a").with_sig("s").with_body("bod"));
            b.finish().unwrap().graph.graph_hash().to_string()
        };
        let varied = {
            let mut b = GraphBuilder::default();
            b.node(
                NodeSpec::new(fn_key("a"), "a")
                    .with_sig("s")
                    .with_body("bod")
                    .with_lines(5, 99)
                    .with_attrs(json!({ "doc": "changed" })),
            );
            b.finish().unwrap().graph.graph_hash().to_string()
        };
        assert_eq!(base, varied);
    }

    // -- cfg duplicates & collisions ---------------------------------------

    #[test]
    fn positive_cfg_dup_suffixed() {
        let mut b = GraphBuilder::default();
        let key = fn_key("f");
        assert!(matches!(
            b.node(NodeSpec::new(key.clone(), "f").with_cfg(vec!["feature=a".into()])),
            NodeOutcome::Added(_)
        ));
        let out = b.node(NodeSpec::new(key.clone(), "f").with_cfg(vec!["feature=b".into()]));
        let suffixed = match out {
            NodeOutcome::Suffixed(k) => k,
            other => panic!("expected Suffixed, got {other:?}"),
        };
        assert!(suffixed.as_str().starts_with(&format!("{}@", key.as_str())));
        let built = b.finish().expect("cfg split is valid");
        assert_eq!(built.report.suffixed, 1);
        let dup = built
            .graph
            .nodes()
            .iter()
            .find(|r| r.node.key == suffixed)
            .expect("suffixed node present");
        assert_eq!(dup.version.attrs.get("dup"), Some(&json!(true)));
        assert_eq!(dup.version.attrs.get("cfg"), Some(&json!(["feature=b"])));
    }

    #[test]
    fn negative_dup_same_cfg_fails() {
        let mut b = GraphBuilder::default();
        let key = fn_key("f");
        b.node(NodeSpec::new(key.clone(), "f").with_cfg(vec!["x".into()]));
        b.node(NodeSpec::new(key.clone(), "f").with_cfg(vec!["x".into()]));
        assert_eq!(b.finish().unwrap_err(), BuildError::DuplicateKey(key));
    }

    #[test]
    fn negative_id_collision_fails() {
        let mut b = GraphBuilder::default().with_id_fn(|_| NodeId(7));
        b.node(NodeSpec::new(fn_key("a"), "a"));
        b.node(NodeSpec::new(fn_key("b"), "b"));
        assert_eq!(b.finish().unwrap_err(), BuildError::IdCollision(NodeId(7)));
    }

    // -- edges --------------------------------------------------------------

    #[test]
    fn negative_dangling_edge_dropped() {
        let mut b = GraphBuilder::default();
        b.node(NodeSpec::new(fn_key("a"), "a"));
        b.edge(EdgeKind::Calls, &fn_key("a"), &fn_key("ghost"), 1.0, json!({}));
        let built = b.finish().unwrap();
        assert!(built.graph.edges().is_empty());
        assert_eq!(built.report.dropped_edges, 1);
    }

    #[test]
    fn corner_edge_before_node_kept() {
        let mut b = GraphBuilder::default();
        b.edge(EdgeKind::Calls, &fn_key("a"), &fn_key("b"), 1.0, json!({}));
        b.node(NodeSpec::new(fn_key("a"), "a"));
        b.node(NodeSpec::new(fn_key("b"), "b"));
        let built = b.finish().unwrap();
        assert_eq!(built.graph.edges().len(), 1);
    }

    #[test]
    fn corner_empty_graph() {
        let built = GraphBuilder::default().finish().unwrap();
        assert_eq!(built.graph.graph_hash(), EMPTY_HASH);
        assert_eq!(built.report, BuildReport::default());
        assert!(!built.graph.truncated());
    }

    // -- caps ---------------------------------------------------------------

    #[test]
    fn boundary_max_nodes() {
        let mut b = GraphBuilder::new(BuilderCaps {
            max_nodes: 3,
            max_edges: 100,
        });
        for i in 0..4 {
            b.node(NodeSpec::new(fn_key(&format!("f{i}")), format!("f{i}")));
        }
        let built = b.finish().unwrap();
        assert_eq!(built.report.nodes, 3);
        assert!(built.report.truncated);
        assert_eq!(built.report.dropped_nodes, 1);
    }

    #[test]
    fn boundary_max_edges() {
        let mut b = GraphBuilder::new(BuilderCaps {
            max_nodes: 100,
            max_edges: 3,
        });
        b.node(NodeSpec::new(fn_key("a"), "a"));
        b.node(NodeSpec::new(fn_key("b"), "b"));
        for kind in [
            EdgeKind::Contains,
            EdgeKind::Calls,
            EdgeKind::Imports,
            EdgeKind::References,
        ] {
            b.edge(kind, &fn_key("a"), &fn_key("b"), 1.0, json!({}));
        }
        let built = b.finish().unwrap();
        assert_eq!(built.report.edges, 3);
        assert!(built.report.truncated);
        assert_eq!(built.report.dropped_edges, 1);
    }

    // -- field boundaries ---------------------------------------------------

    #[rstest]
    #[case::name_256("a".repeat(256), true)]
    #[case::name_257("a".repeat(257), false)]
    fn boundary_name(#[case] name: String, #[case] ok: bool) {
        let mut b = GraphBuilder::default();
        let out = b.node(NodeSpec::new(fn_key("f"), name));
        assert_eq!(matches!(out, NodeOutcome::Added(_)), ok);
    }

    #[rstest]
    #[case::qualifier_512("q".repeat(512), true)]
    #[case::qualifier_513("q".repeat(513), false)]
    fn boundary_qualifier(#[case] q: String, #[case] ok: bool) {
        let mut b = GraphBuilder::default();
        let out = b.node(NodeSpec::new(fn_key("f"), "f").with_qualifier(q));
        assert_eq!(matches!(out, NodeOutcome::Added(_)), ok);
    }

    #[rstest]
    #[case::attrs_4096(4096, true)]
    #[case::attrs_4097(4097, false)]
    fn boundary_attrs(#[case] total: usize, #[case] ok: bool) {
        let mut b = GraphBuilder::default();
        let out = b.node(NodeSpec::new(fn_key("f"), "f").with_attrs(attrs_of_size(total)));
        assert_eq!(matches!(out, NodeOutcome::Added(_)), ok);
    }

    #[rstest]
    #[case::edge_attrs_1024(1024, true)]
    #[case::edge_attrs_1025(1025, false)]
    fn boundary_edge_attrs(#[case] total: usize, #[case] ok: bool) {
        let mut b = GraphBuilder::default();
        let out = b.edge(
            EdgeKind::Calls,
            &fn_key("a"),
            &fn_key("b"),
            1.0,
            attrs_of_size(total),
        );
        assert_eq!(out == EdgeOutcome::Added, ok);
    }

    // -- adversarial --------------------------------------------------------

    #[rstest]
    #[case::traversal("../x")]
    #[case::absolute("/etc/passwd")]
    #[case::control("a\u{0001}b")]
    fn adversarial_file(#[case] file: &str) {
        let mut b = GraphBuilder::default();
        let out = b.node(NodeSpec::new(fn_key("f"), "f").with_file(file));
        assert_eq!(out, NodeOutcome::Dropped(DropReason::File));
    }

    #[rstest]
    #[case::string(json!("nope"))]
    #[case::array(json!([1, 2, 3]))]
    #[case::number(json!(42))]
    fn adversarial_attrs_not_object(#[case] attrs: serde_json::Value) {
        let mut b = GraphBuilder::default();
        let out = b.node(NodeSpec::new(fn_key("f"), "f").with_attrs(attrs));
        assert_eq!(out, NodeOutcome::Dropped(DropReason::Attrs));
    }

    #[rstest]
    #[case::nan(f32::NAN)]
    #[case::neg(-5.0)]
    #[case::inf(f32::INFINITY)]
    fn adversarial_weight_clamped(#[case] weight: f32) {
        let mut b = GraphBuilder::default();
        b.node(NodeSpec::new(fn_key("a"), "a"));
        b.node(NodeSpec::new(fn_key("b"), "b"));
        b.edge(EdgeKind::Calls, &fn_key("a"), &fn_key("b"), weight, json!({}));
        let built = b.finish().unwrap();
        assert_eq!(built.graph.edges()[0].weight, 1.0);
    }

    #[test]
    fn adversarial_lines_reversed() {
        let mut b = GraphBuilder::default();
        b.node(NodeSpec::new(fn_key("f"), "f").with_lines(10, 2));
        let built = b.finish().unwrap();
        let v = &built.graph.nodes()[0].version;
        assert_eq!(v.line_start, 10);
        assert_eq!(v.line_end, 10);
    }

    #[test]
    fn adversarial_drop_reason_never_echoes() {
        let hostile = "Z".repeat(4096);
        let mut b = GraphBuilder::default();
        let out = b.node(NodeSpec::new(fn_key("f"), hostile.clone()));
        let reason = match out {
            NodeOutcome::Dropped(r) => r,
            other => panic!("expected drop, got {other:?}"),
        };
        assert!(!reason.to_string().contains('Z'));
        assert!(!hostile.contains(&reason.to_string()));
    }
}
