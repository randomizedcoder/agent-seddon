//! Every SQL string the Postgres tier issues (`$n` params only — this module never
//! interpolates a value into SQL) and the **pure, DB-free helpers** that shape a request or a
//! [`RepoGraph`] into bind values: the clamps (`03-queries.md` hop / cap limits), the
//! `u64`↔`i64` binds, and the `UNNEST` array builders. Keeping these pure and here is what lets
//! the P1 unit table (`08-test-matrix.md`) exercise them in the gate with no database.

use agent_core::repo_graph::{
    RepoGraph, MAX_NEIGHBOR_HOPS, MAX_PATH_HOPS, MAX_RADIUS_HOPS, MAX_RESULT, MAX_RETAIN,
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

// ---------------------------------------------------------------------------
// SQL statements — one `const` per statement, `$n` placeholders only. No string
// building anywhere else in the tier (03-queries.md). Every statement binds the tenant
// as `$1`; ids, keys, files and JSON reach the server as bound parameters. Time binds
// epoch ms (`BIGINT`) → `to_timestamp($n/1000.0)` in, `(EXTRACT(EPOCH …)*1000)::BIGINT`
// out. JSONB binds as text (`$n::jsonb`) and reads as `col::text`.
// ---------------------------------------------------------------------------

/// The `repos` projection (single-table; the injectable clock drives `created_at`).
macro_rules! repo_cols {
    () => {
        "repo_id, slug, forge, remote_url, default_branch, profile::text AS profile, \
         (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS created_at_ms"
    };
}

/// The `graph_snapshots` metadata projection.
macro_rules! snapshot_cols {
    () => {
        "snapshot_id, repo_id, commit_sha, extractors::text AS extractors, extractor_version, \
         graph_hash, node_count, edge_count, status, reason, \
         (EXTRACT(EPOCH FROM built_at) * 1000)::BIGINT AS built_at_ms, duration_ms"
    };
}

/// One node row (`graph_nodes` body joined to its `graph_node_versions` row), aliases `n` / `v`.
/// `attrs` and `name_tokens` decode straight (`text` / `text[]`).
macro_rules! node_row_cols {
    () => {
        "n.node_key, n.node_id, n.kind, n.lang, n.name, n.name_tokens, n.qualifier, \
         v.file, v.line_start, v.line_end, v.sig_hash, v.body_hash, v.exported, \
         v.attrs::text AS attrs"
    };
}

/// The scope-scoped join tail every node read shares: `$1` tenant, `$2` repo_id, `$3` snapshot_id.
macro_rules! node_row_from {
    () => {
        " FROM graph_node_versions v \
          JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = v.node_id \
          WHERE v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 "
    };
}

// -- repos ----------------------------------------------------------------------

pub const ENSURE_TENANT: &str =
    "INSERT INTO tenants (tenant) VALUES ($1) ON CONFLICT (tenant) DO NOTHING";

/// Upsert by `(tenant, slug)`; `created_at` is set only on insert (kept on update). `$6` profile
/// JSON text, `$7` now-ms.
pub const REPO_UPSERT: &str = "INSERT INTO repos
     (tenant, slug, forge, remote_url, default_branch, profile, created_at)
     VALUES ($1, $2, $3, $4, $5, $6::jsonb, to_timestamp($7::double precision / 1000.0))
     ON CONFLICT ON CONSTRAINT repos_slug_key DO UPDATE SET
       forge = EXCLUDED.forge, remote_url = EXCLUDED.remote_url,
       default_branch = EXCLUDED.default_branch, profile = EXCLUDED.profile
     RETURNING repo_id";
pub const REPO_GET: &str =
    concat!("SELECT ", repo_cols!(), " FROM repos WHERE tenant = $1 AND slug = $2");
pub const REPOS: &str =
    concat!("SELECT ", repo_cols!(), " FROM repos WHERE tenant = $1 ORDER BY slug");

// -- snapshots: begin / write / finish ------------------------------------------

pub const REPO_EXISTS: &str = "SELECT 1 FROM repos WHERE tenant = $1 AND repo_id = $2";
/// The identity row (if any) under a lock: `$2` repo_id, `$3` sha, `$4` extractor_version.
pub const BEGIN_FIND: &str = "SELECT snapshot_id, status FROM graph_snapshots
     WHERE tenant = $1 AND repo_id = $2 AND commit_sha = $3 AND extractor_version = $4
     FOR UPDATE";
pub const SNAPSHOT_DELETE_ONE: &str =
    "DELETE FROM graph_snapshots WHERE tenant = $1 AND snapshot_id = $2";
/// `$5` extractors JSON text, `$6` now-ms.
pub const BEGIN_INSERT: &str = "INSERT INTO graph_snapshots
     (tenant, repo_id, commit_sha, extractors, extractor_version, status, built_at)
     VALUES ($1, $2, $3, $4::jsonb, $5, 'building', to_timestamp($6::double precision / 1000.0))
     RETURNING snapshot_id";

/// The snapshot's repo + status under a lock (write / finish guards).
pub const SNAPSHOT_LOCK: &str = "SELECT repo_id, status,
     (EXTRACT(EPOCH FROM built_at) * 1000)::BIGINT AS built_at_ms
     FROM graph_snapshots WHERE tenant = $1 AND snapshot_id = $2 FOR UPDATE";

/// Bulk-insert shared bodies. `$3` ids, `$4` keys, `$5` kinds, `$6` langs, `$7` names,
/// `$8` name_tokens (comma-joined per row; empty ⇒ `{}`), `$9` qualifiers. Existing bodies are
/// kept (`DO NOTHING`); the collision check runs next.
pub const NODES_INSERT: &str = "INSERT INTO graph_nodes
     (tenant, repo_id, node_id, node_key, kind, lang, name, name_tokens, qualifier)
     SELECT $1, $2, u.node_id, u.node_key, u.kind, u.lang, u.name,
            CASE WHEN u.toks = '' THEN ARRAY[]::text[] ELSE string_to_array(u.toks, ',') END,
            u.qualifier
     FROM UNNEST($3::bigint[], $4::text[], $5::text[], $6::text[], $7::text[], $8::text[], $9::text[])
       AS u(node_id, node_key, kind, lang, name, toks, qualifier)
     ON CONFLICT (tenant, repo_id, node_id) DO NOTHING";

/// A distinct key mapped onto a stored `node_id` (a collision the content-addressed id forbids):
/// `$3` ids, `$4` keys. `> 0` ⇒ `Conflict`.
pub const COLLISION_CHECK: &str = "SELECT count(*) FROM UNNEST($3::bigint[], $4::text[]) AS b(node_id, node_key)
     JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = b.node_id
     WHERE n.node_key <> b.node_key";

/// Bulk-insert this snapshot's node versions. `$4` ids, `$5` files, `$6` line_start, `$7` line_end,
/// `$8` sig_hash, `$9` body_hash, `$10` exported, `$11` attrs JSON text.
pub const VERSIONS_INSERT: &str = "INSERT INTO graph_node_versions
     (tenant, repo_id, snapshot_id, node_id, file, line_start, line_end, sig_hash, body_hash, exported, attrs)
     SELECT $1, $2, $3, u.node_id, u.file, u.ls, u.le, u.sig, u.body, u.exp, u.attrs::jsonb
     FROM UNNEST($4::bigint[], $5::text[], $6::int[], $7::int[], $8::text[], $9::text[], $10::bool[], $11::text[])
       AS u(node_id, file, ls, le, sig, body, exp, attrs)";

/// Bulk-insert this snapshot's edges. `$4` kinds, `$5` src_ids, `$6` dst_ids, `$7` weights,
/// `$8` attrs JSON text.
pub const EDGES_INSERT: &str = "INSERT INTO graph_edges
     (tenant, repo_id, snapshot_id, kind, src_id, dst_id, weight, attrs)
     SELECT $1, $2, $3, u.kind, u.src_id, u.dst_id, u.weight, u.attrs::jsonb
     FROM UNNEST($4::text[], $5::bigint[], $6::bigint[], $7::real[], $8::text[])
       AS u(kind, src_id, dst_id, weight, attrs)";

/// Record the write's rollup on the snapshot: `$3` graph_hash, `$4` node_count, `$5` edge_count.
pub const WRITE_META: &str = "UPDATE graph_snapshots
     SET graph_hash = $3, node_count = $4, edge_count = $5
     WHERE tenant = $1 AND snapshot_id = $2";

/// Finish a building snapshot: `$3` status, `$4` reason, `$5` duration_ms.
pub const FINISH_UPDATE: &str = "UPDATE graph_snapshots
     SET status = $3, reason = $4, duration_ms = $5
     WHERE tenant = $1 AND snapshot_id = $2";

// -- snapshots: read / list / retention / diff ----------------------------------

/// The newest ready snapshot for `(repo, sha)`: `$2` repo_id, `$3` sha.
pub const SNAPSHOT_FIND: &str = concat!(
    "SELECT ",
    snapshot_cols!(),
    " FROM graph_snapshots
      WHERE tenant = $1 AND repo_id = $2 AND status = 'ready' AND commit_sha = $3
      ORDER BY built_at DESC, snapshot_id DESC LIMIT 1"
);
/// The newest ready snapshot for a repo: `$2` repo_id.
pub const SNAPSHOT_LATEST: &str = concat!(
    "SELECT ",
    snapshot_cols!(),
    " FROM graph_snapshots
      WHERE tenant = $1 AND repo_id = $2 AND status = 'ready'
      ORDER BY built_at DESC, snapshot_id DESC LIMIT 1"
);
/// Snapshots for a repo, newest first, every status: `$2` repo_id, `$3` limit.
pub const SNAPSHOTS: &str = concat!(
    "SELECT ",
    snapshot_cols!(),
    " FROM graph_snapshots WHERE tenant = $1 AND repo_id = $2
      ORDER BY built_at DESC, snapshot_id DESC LIMIT $3"
);
/// One snapshot's metadata under `(repo, snapshot)`: `$2` repo_id, `$3` snapshot_id.
pub const SNAPSHOT_GET: &str = concat!(
    "SELECT ",
    snapshot_cols!(),
    " FROM graph_snapshots WHERE tenant = $1 AND repo_id = $2 AND snapshot_id = $3"
);

/// The `keep` newest ready snapshot ids for a repo (the retained set): `$2` repo_id, `$3` keep.
pub const RETENTION_READY: &str = "SELECT snapshot_id FROM graph_snapshots
     WHERE tenant = $1 AND repo_id = $2 AND status = 'ready'
     ORDER BY built_at DESC, snapshot_id DESC LIMIT $3";
/// Delete every `failed` snapshot and every `ready` one not in the retained set `$3`; leave
/// `building`. CASCADE clears their versions / edges. Returns the deleted ids.
pub const RETENTION_DELETE: &str = "DELETE FROM graph_snapshots
     WHERE tenant = $1 AND repo_id = $2
       AND (status = 'failed' OR (status = 'ready' AND snapshot_id <> ALL($3::bigint[])))
     RETURNING snapshot_id";
/// Sweep bodies of this repo that no surviving version references (the non-CASCADE version→node FK).
pub const SWEEP_BODIES: &str = "DELETE FROM graph_nodes n
     WHERE n.tenant = $1 AND n.repo_id = $2
       AND NOT EXISTS (SELECT 1 FROM graph_node_versions v
                       WHERE v.tenant = $1 AND v.repo_id = $2 AND v.node_id = n.node_id)";

/// The repo of a snapshot (diff existence + same-repo check): `$2` snapshot_id.
pub const SNAPSHOT_REPO: &str =
    "SELECT repo_id FROM graph_snapshots WHERE tenant = $1 AND snapshot_id = $2";
/// `(node_key, sig_hash, body_hash)` for a snapshot's versions: `$2` repo_id, `$3` snapshot_id.
pub const DIFF_VERSIONS: &str = "SELECT n.node_key, v.sig_hash, v.body_hash
     FROM graph_node_versions v
     JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = v.node_id
     WHERE v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3";
/// `(kind, src_key, dst_key)` for a snapshot's edges: `$2` repo_id, `$3` snapshot_id.
pub const DIFF_EDGES: &str = "SELECT e.kind, sn.node_key AS src_key, dn.node_key AS dst_key
     FROM graph_edges e
     JOIN graph_nodes sn ON sn.tenant = $1 AND sn.repo_id = $2 AND sn.node_id = e.src_id
     JOIN graph_nodes dn ON dn.tenant = $1 AND dn.repo_id = $2 AND dn.node_id = e.dst_id
     WHERE e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3";

// -- reads ----------------------------------------------------------------------

/// The scope preflight: a snapshot under `(tenant, repo, snapshot)` exists (else `NotFound`).
pub const SCOPE_EXISTS: &str =
    "SELECT 1 FROM graph_snapshots WHERE tenant = $1 AND repo_id = $2 AND snapshot_id = $3";

/// `$4` keys.
pub const NODES_BY_KEY: &str = concat!(
    "SELECT ",
    node_row_cols!(),
    node_row_from!(),
    " AND n.node_key = ANY($4::text[]) ORDER BY n.node_key"
);
/// `$4` files.
pub const NODES_BY_FILE: &str = concat!(
    "SELECT ",
    node_row_cols!(),
    node_row_from!(),
    " AND v.file = ANY($4::text[]) ORDER BY n.node_key"
);
/// `$4` name, `$5` kind (`NULL` = any), `$6` limit.
pub const NODES_BY_NAME: &str = concat!(
    "SELECT ",
    node_row_cols!(),
    node_row_from!(),
    " AND n.name = $4 AND ($5::text IS NULL OR n.kind = $5) ORDER BY n.node_key LIMIT $6"
);

/// Neighbours reached **inbound** (callers): match `dst_id`, yield `src_id`. `$4` seeds, `$5` kind,
/// `$6` hops, `$7` cap. Seeds are excluded; rows carry their min depth.
pub const NEIGHBORS_IN: &str = concat!(
    "WITH RECURSIVE walk(node_id, depth) AS (
       SELECT id, 0 FROM UNNEST($4::bigint[]) AS s(id)
       UNION
       SELECT e.src_id, w.depth + 1 FROM walk w
         JOIN graph_edges e ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
          AND e.kind = $5 AND e.dst_id = w.node_id
       WHERE w.depth < $6
     ),
     mind AS (
       SELECT node_id, min(depth) AS depth FROM walk
       WHERE node_id <> ALL($4::bigint[]) GROUP BY node_id
     )
     SELECT ",
    node_row_cols!(),
    ", m.depth AS depth
     FROM mind m
     JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = m.node_id
     JOIN graph_node_versions v ON v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 AND v.node_id = m.node_id
     ORDER BY depth, n.node_key LIMIT $7"
);
/// Neighbours reached **outbound**: match `src_id`, yield `dst_id`. Params as [`NEIGHBORS_IN`].
pub const NEIGHBORS_OUT: &str = concat!(
    "WITH RECURSIVE walk(node_id, depth) AS (
       SELECT id, 0 FROM UNNEST($4::bigint[]) AS s(id)
       UNION
       SELECT e.dst_id, w.depth + 1 FROM walk w
         JOIN graph_edges e ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
          AND e.kind = $5 AND e.src_id = w.node_id
       WHERE w.depth < $6
     ),
     mind AS (
       SELECT node_id, min(depth) AS depth FROM walk
       WHERE node_id <> ALL($4::bigint[]) GROUP BY node_id
     )
     SELECT ",
    node_row_cols!(),
    ", m.depth AS depth
     FROM mind m
     JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = m.node_id
     JOIN graph_node_versions v ON v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 AND v.node_id = m.node_id
     ORDER BY depth, n.node_key LIMIT $7"
);

/// Files reachable inbound over `calls`/`imports`/`implements` from the seed files (seed nodes
/// themselves excluded, matching `MemRepoGraph`). `$4` files, `$5` hops, `$6` cap.
pub const BLAST_RADIUS: &str = "WITH RECURSIVE seed AS (
       SELECT node_id FROM graph_node_versions
       WHERE tenant = $1 AND repo_id = $2 AND snapshot_id = $3 AND file = ANY($4::text[])
     ),
     walk(node_id, depth) AS (
       SELECT node_id, 0 FROM seed
       UNION
       SELECT e.src_id, w.depth + 1 FROM walk w
         JOIN graph_edges e ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
          AND e.kind IN ('calls','imports','implements') AND e.dst_id = w.node_id
       WHERE w.depth < $5
     )
     SELECT DISTINCT v.file FROM walk w
     JOIN graph_node_versions v ON v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 AND v.node_id = w.node_id
     WHERE v.file <> '' AND w.node_id NOT IN (SELECT node_id FROM seed)
     ORDER BY v.file LIMIT $6";

/// Tests reaching the seeds inbound over `calls` (incl. the seeds), then `tests` in-edges. `$4`
/// seeds, `$5` hops. Deduped per test node; the caller re-sorts by key and caps.
pub const TESTS_COVERING: &str = "WITH RECURSIVE reach(node_id, depth) AS (
       SELECT id, 0 FROM UNNEST($4::bigint[]) AS s(id)
       UNION
       SELECT e.src_id, r.depth + 1 FROM reach r
         JOIN graph_edges e ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
          AND e.kind = 'calls' AND e.dst_id = r.node_id
       WHERE r.depth < $5
     )
     SELECT DISTINCT ON (n.node_id)
            n.node_key, n.node_id, n.kind, n.lang, n.name, n.name_tokens, n.qualifier,
            v.file, v.line_start, v.line_end, v.sig_hash, v.body_hash, v.exported,
            v.attrs::text AS attrs, te.attrs->>'via' AS via
     FROM reach r
     JOIN graph_edges te ON te.tenant = $1 AND te.repo_id = $2 AND te.snapshot_id = $3
                        AND te.kind = 'tests' AND te.dst_id = r.node_id
     JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = te.src_id AND n.kind = 'test'
     JOIN graph_node_versions v ON v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 AND v.node_id = te.src_id
     ORDER BY n.node_id";

/// Shortest paths `src`→`dst` outbound over `calls`/`imports`/`depends_on`/`contains`, cycle-
/// guarded. `$4` src, `$5` dst, `$6` max_hops, `$7` max_paths. Returns `bigint[]` id paths.
pub const PATH_BETWEEN: &str = "WITH RECURSIVE p(node_id, path, depth) AS (
       SELECT $4::bigint, ARRAY[$4::bigint], 0
       UNION ALL
       SELECT e.dst_id, p.path || e.dst_id, p.depth + 1
       FROM p JOIN graph_edges e ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
          AND e.kind IN ('calls','imports','depends_on','contains') AND e.src_id = p.node_id
       WHERE p.depth < $6 AND NOT (e.dst_id = ANY(p.path))
     )
     SELECT path FROM p WHERE node_id = $5 AND depth >= 1 ORDER BY depth, path LIMIT $7";

/// Map node ids back to keys within a scope: `$3` ids.
pub const KEYS_FOR_IDS: &str = "SELECT node_id, node_key FROM graph_nodes
     WHERE tenant = $1 AND repo_id = $2 AND node_id = ANY($3::bigint[])";

// -- shape ----------------------------------------------------------------------

pub const SHAPE_NODES_BY_KIND: &str = "SELECT n.kind, count(*)::BIGINT AS c
     FROM graph_node_versions v
     JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = v.node_id
     WHERE v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 GROUP BY n.kind";
pub const SHAPE_EDGES_BY_KIND: &str = "SELECT kind, count(*)::BIGINT AS c
     FROM graph_edges WHERE tenant = $1 AND repo_id = $2 AND snapshot_id = $3 GROUP BY kind";
pub const SHAPE_FILES: &str = "SELECT count(DISTINCT file)::BIGINT AS c
     FROM graph_node_versions
     WHERE tenant = $1 AND repo_id = $2 AND snapshot_id = $3 AND file <> ''";

// ===========================================================================
// P1 — in-gate unit tables for the pure helpers (docs/design/repo-knowledge/08-test-matrix.md).
// No database: these run in the normal gate under `--features repo-graph-postgres`.
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::repo_graph::{node_id_for, EdgeKind, MAX_RESULT as CAP};
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
        assert!(na.ids.is_empty());
        assert!(ea.kinds.is_empty());
        assert!(na.attrs.is_empty());
        assert!(ea.attrs.is_empty());
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
