//! The graph value types, the immutable [`RepoGraph`] a snapshot write consumes, and the
//! [`Extractor`] contract (`01-schema.md`, `02-extraction.md`).
//!
//! A [`RepoGraph`] is produced **only** by [`super::builder::GraphBuilder::finish`]: its fields
//! are private and there is no public constructor, so a store can never be handed a graph with a
//! forged id or an unsorted, unhashed body. Consumers read it through the accessors.

use super::{EdgeKind, Lang, NodeId, NodeKey, NodeKind, RepoGraphResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

/// A node body: the identity that is shared across snapshots of a repo (`graph_nodes`). The
/// per-snapshot facts (file, lines, hashes, attrs) live in [`NodeVersion`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// The stable, validated key.
    pub key: NodeKey,
    /// The content-addressed id ([`super::node_id_for`]).
    pub id: NodeId,
    /// The node kind (redundant with `key.kind()`, materialised for the store column).
    pub kind: NodeKind,
    /// The language (redundant with `key.lang()`).
    pub lang: Lang,
    /// The display name (the last path segment of the symbol).
    pub name: String,
    /// The lower-cased token split of `name` ([`super::name_tokens`]).
    pub name_tokens: Vec<String>,
    /// The crate / module / package / directory path the node lives under.
    pub qualifier: String,
}

/// The per-snapshot facts about a node (`graph_node_versions`), joined to its [`Node`] by
/// [`Node::id`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeVersion {
    /// The repo-relative, confined file the node is defined in (may be empty for a node with no
    /// single file, e.g. a crate).
    pub file: String,
    /// The first line of the definition, clamped to `0..=i32::MAX`.
    pub line_start: i32,
    /// The last line of the definition, clamped to `>= line_start`.
    pub line_end: i32,
    /// A hash of the signature tokens (docs / attrs stripped) — changes when the API changes.
    pub sig_hash: String,
    /// A hash of the body — changes when the implementation changes.
    pub body_hash: String,
    /// Whether the item is part of the crate's public API.
    pub exported: bool,
    /// Extractor-supplied attributes (`is_seam`, `tool_name`, the cfg-dup markers, …); a JSON
    /// object of at most 4096 serialized bytes.
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

/// A node together with its version in a built graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeRecord {
    pub node: Node,
    pub version: NodeVersion,
}

/// A directed, kinded edge between two nodes (`graph_edges`). Carries both endpoint keys (for
/// the deterministic sort and diffs) and their ids (for the store).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub kind: EdgeKind,
    pub src: NodeKey,
    pub dst: NodeKey,
    pub src_id: NodeId,
    pub dst_id: NodeId,
    pub weight: f32,
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

/// An immutable, validated, sorted and hashed graph — the unit a snapshot write consumes.
/// Constructible only by [`super::builder::GraphBuilder::finish`].
#[derive(Debug, Clone, PartialEq)]
pub struct RepoGraph {
    pub(super) nodes: Vec<NodeRecord>,
    pub(super) edges: Vec<Edge>,
    pub(super) graph_hash: String,
    pub(super) truncated: bool,
}

impl RepoGraph {
    /// The node records, sorted by node key.
    pub fn nodes(&self) -> &[NodeRecord] {
        &self.nodes
    }

    /// The edges, sorted by `(kind, src_key, dst_key)`.
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// The `sha256` over the sorted `(node_key, kind, sig_hash, body_hash)` and
    /// `(kind, src_key, dst_key)` tuples — the snapshot's content identity.
    pub fn graph_hash(&self) -> &str {
        &self.graph_hash
    }

    /// Whether a cap was hit and later inserts were dropped.
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

// ---------------------------------------------------------------------------
// The extractor contract (RK-03 implements extractors; RK-01 only defines it)
// ---------------------------------------------------------------------------

/// A per-extractor resource budget (`02-extraction.md`). [`Default`] is the design's numbers.
#[derive(Debug, Clone)]
pub struct ExtractBudget {
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    /// A wall-clock deadline; an extractor checks it and returns early (truncated) when passed.
    pub deadline: Instant,
}

impl Default for ExtractBudget {
    fn default() -> Self {
        Self {
            max_files: 50_000,
            max_file_bytes: 2 * 1024 * 1024,
            max_nodes: 250_000,
            max_edges: 2_000_000,
            deadline: Instant::now() + Duration::from_secs(600),
        }
    }
}

/// The maximum number of diagnostics kept in an [`ExtractReport`].
pub const MAX_DIAGNOSTICS: usize = 32;

/// The maximum length of a single diagnostic, in bytes.
pub const MAX_DIAGNOSTIC_LEN: usize = 200;

/// What an extractor observed (`02-extraction.md`). Merged across extractors into the snapshot's
/// [`super::ExtractReport`]-shaped record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractReport {
    pub files: usize,
    pub skipped: usize,
    pub parse_errors: usize,
    pub truncated: bool,
    pub dropped_edges: usize,
    /// Bounded human-readable notes; at most [`MAX_DIAGNOSTICS`], each at most
    /// [`MAX_DIAGNOSTIC_LEN`] bytes.
    pub diagnostics: Vec<String>,
}

impl ExtractReport {
    /// Fold another report into this one: sum the counts, OR the truncation flag, and append the
    /// diagnostics up to the cap (over-long entries truncated on a char boundary).
    pub fn merge(&mut self, other: ExtractReport) {
        self.files += other.files;
        self.skipped += other.skipped;
        self.parse_errors += other.parse_errors;
        self.truncated |= other.truncated;
        self.dropped_edges += other.dropped_edges;
        for mut d in other.diagnostics {
            if self.diagnostics.len() >= MAX_DIAGNOSTICS {
                break;
            }
            if d.len() > MAX_DIAGNOSTIC_LEN {
                let mut cut = MAX_DIAGNOSTIC_LEN;
                while !d.is_char_boundary(cut) {
                    cut -= 1;
                }
                d.truncate(cut);
            }
            self.diagnostics.push(d);
        }
    }
}

/// A deterministic, versioned, budgeted repo extractor (`02-extraction.md`). No LLM (README D5):
/// an extractor is a parser / git reader only. RK-01 defines the contract; RK-03 implements
/// `rust-syn`, `cargo` and `docs`.
pub trait Extractor: Send + Sync {
    /// A stable, short name (`"rust-syn"`); part of the snapshot's extractor list.
    fn name(&self) -> &'static str;
    /// A version bumped whenever the output changes; part of the snapshot identity.
    fn version(&self) -> &'static str;
    /// Walk `root` (each path `confine`d before opening), emit nodes and edges into `out`, and
    /// return what was observed. Must be deterministic given the same tree.
    fn extract(
        &self,
        root: &Path,
        out: &mut super::builder::GraphBuilder,
        budget: &ExtractBudget,
    ) -> RepoGraphResult<ExtractReport>;
}

/// The snapshot's `extractor_version`: the sorted `name@version` list joined with `,`. Part of
/// the snapshot identity, so a bumped extractor forces a re-index.
pub fn extractor_version(extractors: &[&dyn Extractor]) -> String {
    let mut parts: Vec<String> = extractors
        .iter()
        .map(|e| format!("{}@{}", e.name(), e.version()))
        .collect();
    parts.sort();
    parts.join(",")
}
