//! Every SQL string the Postgres tier issues (`$n` params only — this module never
//! interpolates a value into SQL) and the **pure, DB-free helpers** that shape a request or a
//! [`RepoGraph`] into bind values: the clamps (`03-queries.md` hop / cap limits), the
//! `u64`↔`i64` binds, and the `UNNEST` array builders. Keeping these pure and here is what lets
//! the P1 unit table (`08-test-matrix.md`) exercise them in the gate with no database.

use agent_core::repo_graph::{
    node_id_for, EdgeKind, RepoGraph, MAX_NEIGHBOR_HOPS, MAX_PATHS, MAX_PATH_HOPS, MAX_RADIUS_HOPS,
    MAX_RESULT, MAX_RETAIN,
};

/// The maximum number of rows bound into a single `UNNEST` insert. A snapshot may hold up to
/// [`agent_core::repo_graph::MAX_SNAPSHOT_EDGES`] edges; binding them as parallel arrays keeps the
/// parameter count to a handful regardless of row count, but a single statement over millions of
/// rows still allocates a large message, so the write loop chunks the arrays at this size.
pub const WRITE_CHUNK: usize = 10_000;

// ---------------------------------------------------------------------------
// Clamps (every read clamps its hops / caps / list lengths in Rust, fail-closed)
// ---------------------------------------------------------------------------

/// Clamp a neighbour/blast/path hop count to `1..=max` (`max` is the per-verb ceiling, e.g.
/// [`MAX_NEIGHBOR_HOPS`]). Zero becomes one; anything over the ceiling is capped.
pub fn clamp_hops(hops: u32, max: u32) -> u32 {
    hops.clamp(1, max)
}

/// Clamp a result cap to `1..=MAX_RESULT`.
pub fn clamp_cap(cap: usize) -> usize {
    cap.clamp(1, MAX_RESULT)
}

/// Clamp a read `limit` to `1..=MAX_RESULT`.
pub fn clamp_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_RESULT)
}

/// Clamp a retention `keep` to `1..=MAX_RETAIN`.
pub fn clamp_keep(keep: usize) -> usize {
    keep.clamp(1, MAX_RETAIN)
}

/// The ceilings the read verbs pass to [`clamp_hops`], re-exported so the impl and the tests name
/// one source.
pub const NEIGHBOR_HOPS: u32 = MAX_NEIGHBOR_HOPS;
/// See [`NEIGHBOR_HOPS`].
pub const RADIUS_HOPS: u32 = MAX_RADIUS_HOPS;
/// See [`NEIGHBOR_HOPS`].
pub const PATH_HOPS: u32 = MAX_PATH_HOPS;
/// The maximum number of paths `path_between` returns.
pub const PATHS: usize = MAX_PATHS;

// ---------------------------------------------------------------------------
// Unsigned <-> signed binds (Postgres has no unsigned integer types)
// ---------------------------------------------------------------------------

/// Epoch milliseconds / any `u64` as the `BIGINT` bind, saturating rather than wrapping so a
/// hostile clock or count can never overflow into a negative column.
pub fn ms(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// A stored `BIGINT` back to `u64`, treating a negative (corrupt) column as zero.
pub fn unsigned(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

/// A `usize` count as an `i32` column (line numbers, counts), saturating.
pub fn i32_of(v: usize) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

// ---------------------------------------------------------------------------
// UNNEST array builders (graph -> parallel bind Vecs)
// ---------------------------------------------------------------------------

/// The parallel columns of one `UNNEST` node write, in row order. The body columns
/// (`graph_nodes`) and the version columns (`graph_node_versions`) are built together because both
/// iterate the snapshot's node records in the same order, so `ids[i]` addresses the same node in
/// every column.
#[derive(Debug, Default, PartialEq)]
pub struct NodeArrays {
    // -- graph_nodes (shared bodies) --------------------------------------
    pub ids: Vec<i64>,
    pub keys: Vec<String>,
    pub kinds: Vec<String>,
    pub langs: Vec<String>,
    pub names: Vec<String>,
    pub tokens: Vec<Vec<String>>,
    pub qualifiers: Vec<String>,
    // -- graph_node_versions (per-snapshot) -------------------------------
    pub files: Vec<String>,
    pub line_starts: Vec<i32>,
    pub line_ends: Vec<i32>,
    pub sig_hashes: Vec<String>,
    pub body_hashes: Vec<String>,
    pub exported: Vec<bool>,
    /// Attrs as JSON text, cast per-row `::jsonb` in the `SELECT` (no sqlx `json` feature).
    pub attrs: Vec<String>,
}

impl NodeArrays {
    /// The number of node rows.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the graph had no nodes.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Build the parallel node columns from a built graph. The node id is the record's own
/// content-addressed [`agent_core::repo_graph::NodeId`] (equal to [`node_id_for`] of its key);
/// attrs serialize to a JSON object string.
pub fn node_arrays(graph: &RepoGraph) -> NodeArrays {
    let n = graph.nodes().len();
    let mut a = NodeArrays {
        ids: Vec::with_capacity(n),
        keys: Vec::with_capacity(n),
        kinds: Vec::with_capacity(n),
        langs: Vec::with_capacity(n),
        names: Vec::with_capacity(n),
        tokens: Vec::with_capacity(n),
        qualifiers: Vec::with_capacity(n),
        files: Vec::with_capacity(n),
        line_starts: Vec::with_capacity(n),
        line_ends: Vec::with_capacity(n),
        sig_hashes: Vec::with_capacity(n),
        body_hashes: Vec::with_capacity(n),
        exported: Vec::with_capacity(n),
        attrs: Vec::with_capacity(n),
    };
    for rec in graph.nodes() {
        let node = &rec.node;
        let ver = &rec.version;
        a.ids.push(node.id.0);
        a.keys.push(node.key.as_str().to_string());
        a.kinds.push(node.kind.as_str().to_string());
        a.langs.push(node.lang.as_str().to_string());
        a.names.push(node.name.clone());
        a.tokens.push(node.name_tokens.clone());
        a.qualifiers.push(node.qualifier.clone());
        a.files.push(ver.file.clone());
        a.line_starts.push(ver.line_start);
        a.line_ends.push(ver.line_end);
        a.sig_hashes.push(ver.sig_hash.clone());
        a.body_hashes.push(ver.body_hash.clone());
        a.exported.push(ver.exported);
        a.attrs.push(attrs_json(&ver.attrs));
    }
    a
}

/// The parallel columns of one `UNNEST` edge write, in row order.
#[derive(Debug, Default, PartialEq)]
pub struct EdgeArrays {
    pub kinds: Vec<String>,
    pub src_ids: Vec<i64>,
    pub dst_ids: Vec<i64>,
    pub weights: Vec<f32>,
    /// Attrs as JSON text, cast per-row `::jsonb` in the `SELECT`.
    pub attrs: Vec<String>,
}

impl EdgeArrays {
    /// The number of edge rows.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// Whether the graph had no edges.
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
}

/// Build the parallel edge columns from a built graph, using the edges' pre-resolved endpoint ids.
pub fn edge_arrays(graph: &RepoGraph) -> EdgeArrays {
    let n = graph.edges().len();
    let mut a = EdgeArrays {
        kinds: Vec::with_capacity(n),
        src_ids: Vec::with_capacity(n),
        dst_ids: Vec::with_capacity(n),
        weights: Vec::with_capacity(n),
        attrs: Vec::with_capacity(n),
    };
    for edge in graph.edges() {
        a.kinds.push(edge.kind.as_str().to_string());
        a.src_ids.push(edge.src_id.0);
        a.dst_ids.push(edge.dst_id.0);
        a.weights.push(edge.weight);
        a.attrs.push(attrs_json(&edge.attrs));
    }
    a
}

/// Serialize an attrs map to a JSON object string (the `::jsonb` cast column). A serialize failure
/// (not reachable for a `serde_json::Map`) degrades to the empty object, never a panic.
fn attrs_json(attrs: &serde_json::Map<String, serde_json::Value>) -> String {
    serde_json::to_string(attrs).unwrap_or_else(|_| "{}".to_string())
}

/// The half-open row ranges covering `0..len` in chunks of `chunk` (`chunk` floored at 1), so a
/// large `UNNEST` write splits into batches that together cover every row exactly once.
pub fn chunk_ranges(len: usize, chunk: usize) -> Vec<std::ops::Range<usize>> {
    let chunk = chunk.max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while start < len {
        let end = (start + chunk).min(len);
        out.push(start..end);
        start = end;
    }
    out
}

// ===========================================================================
// P1 — in-gate unit tables for the pure helpers (docs/design/repo-knowledge/08-test-matrix.md).
// No database: these run in the normal gate under `--features repo-graph-postgres`.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::repo_graph::MAX_RESULT as CAP;
    use agent_testkit::repo_graph::conformance::{fixture_v1, key_alpha};
    use rstest::rstest;

    // -- clamps ------------------------------------------------------------

    #[rstest]
    #[case::boundary_hops_zero(0, NEIGHBOR_HOPS, 1)]
    #[case::boundary_hops_over(99, NEIGHBOR_HOPS, 4)]
    #[case::positive_hops_mid(2, NEIGHBOR_HOPS, 2)]
    #[case::boundary_radius_over(99, RADIUS_HOPS, 3)]
    #[case::boundary_path_over(99, PATH_HOPS, 6)]
    fn clamp_hops_rows(#[case] hops: u32, #[case] max: u32, #[case] want: u32) {
        assert_eq!(clamp_hops(hops, max), want);
    }

    #[rstest]
    #[case::boundary_cap_zero(0, 1)]
    #[case::boundary_cap_over(10_000, CAP)]
    #[case::positive_cap_mid(50, 50)]
    fn clamp_cap_rows(#[case] cap: usize, #[case] want: usize) {
        assert_eq!(clamp_cap(cap), want);
        assert_eq!(clamp_limit(cap), want);
    }

    #[rstest]
    #[case::boundary_keep_zero(0, 1)]
    #[case::boundary_keep_over(5000, MAX_RETAIN)]
    #[case::positive_keep_mid(10, 10)]
    fn clamp_keep_rows(#[case] keep: usize, #[case] want: usize) {
        assert_eq!(clamp_keep(keep), want);
    }

    // -- unsigned/signed binds --------------------------------------------

    #[test]
    fn boundary_ms_u64_max() {
        assert_eq!(ms(u64::MAX), i64::MAX, "saturates, never wraps negative");
        assert_eq!(ms(0), 0);
    }

    #[test]
    fn boundary_unsigned_i64_negative() {
        assert_eq!(unsigned(-1), 0, "a corrupt negative column reads as zero");
        assert_eq!(unsigned(7), 7);
    }

    #[test]
    fn boundary_i32_of_saturates() {
        assert_eq!(i32_of(usize::MAX), i32::MAX);
        assert_eq!(i32_of(3), 3);
    }

    // -- node/edge array builders -----------------------------------------

    #[test]
    fn positive_node_arrays_aligned() {
        let g = fixture_v1();
        let a = node_arrays(&g);
        let n = g.nodes().len();
        assert!(n > 0);
        for col in [
            a.ids.len(),
            a.keys.len(),
            a.kinds.len(),
            a.langs.len(),
            a.names.len(),
            a.tokens.len(),
            a.qualifiers.len(),
            a.files.len(),
            a.line_starts.len(),
            a.line_ends.len(),
            a.sig_hashes.len(),
            a.body_hashes.len(),
            a.exported.len(),
            a.attrs.len(),
        ] {
            assert_eq!(col, n, "every column has one entry per node");
        }
        // ids are content-addressed: ids[i] == node_id_for(keys[i]).
        for (i, key) in a.keys.iter().enumerate() {
            let parsed = agent_core::repo_graph::NodeKey::parse(key).expect("stored key re-parses");
            assert_eq!(a.ids[i], node_id_for(&parsed).0, "id matches key hash");
        }
    }

    #[test]
    fn positive_edge_arrays_aligned() {
        let g = fixture_v1();
        let a = edge_arrays(&g);
        let n = g.edges().len();
        assert!(n > 0);
        assert_eq!(a.src_ids.len(), n);
        assert_eq!(a.dst_ids.len(), n);
        assert_eq!(a.weights.len(), n);
        assert_eq!(a.attrs.len(), n);
        // default weight is 1.0; kinds are the enum's wire spelling.
        assert!(a.weights.iter().all(|w| (*w - 1.0).abs() < f32::EPSILON));
        for k in &a.kinds {
            assert!(EdgeKind::parse(k).is_some(), "kind {k:?} is a wire spelling");
        }
    }

    #[test]
    fn positive_attrs_serialize_roundtrip() {
        let g = fixture_v1();
        let a = node_arrays(&g);
        for text in &a.attrs {
            let v: serde_json::Value = serde_json::from_str(text).expect("attrs parse back");
            assert!(v.is_object(), "attrs is a JSON object");
        }
        // The `alpha` node exists in the fixture; its column is a valid object too.
        assert!(a.keys.iter().any(|k| k == key_alpha().as_str()));
    }

    #[test]
    fn corner_empty_graph_arrays() {
        // A graph with no nodes/edges yields empty, aligned columns and never panics.
        let g = empty_graph();
        let na = node_arrays(&g);
        let ea = edge_arrays(&g);
        assert!(na.is_empty());
        assert!(ea.is_empty());
        assert_eq!(na.len(), 0);
        assert_eq!(ea.len(), 0);
    }

    // -- chunking ----------------------------------------------------------

    #[rstest]
    #[case::boundary_write_chunking(25_000, WRITE_CHUNK, 3)]
    #[case::corner_exact_multiple(20_000, WRITE_CHUNK, 2)]
    #[case::corner_under_one_chunk(5, WRITE_CHUNK, 1)]
    #[case::corner_zero_len(0, WRITE_CHUNK, 0)]
    fn chunk_ranges_cover_every_row(
        #[case] len: usize,
        #[case] chunk: usize,
        #[case] want_batches: usize,
    ) {
        let ranges = chunk_ranges(len, chunk);
        assert_eq!(ranges.len(), want_batches);
        // Contiguous, non-overlapping, covering exactly 0..len.
        let mut cursor = 0;
        for r in &ranges {
            assert_eq!(r.start, cursor, "batches are contiguous");
            assert!(r.end > r.start && r.end - r.start <= chunk);
            cursor = r.end;
        }
        assert_eq!(cursor, len, "batches cover every row exactly once");
    }

    #[test]
    fn corner_chunk_zero_size_floored_to_one() {
        // A zero chunk is floored to 1 so the loop always terminates.
        assert_eq!(chunk_ranges(2, 0), vec![0..1, 1..2]);
    }

    /// A built graph with no nodes or edges (the builder accepts an empty graph).
    fn empty_graph() -> RepoGraph {
        agent_core::repo_graph::GraphBuilder::default()
            .finish()
            .expect("empty graph builds")
            .graph
    }
}
