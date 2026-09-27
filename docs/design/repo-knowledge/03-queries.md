# 03 — Queries: the store seam, the `PgAst` engine, the `repo_graph` tool, the SQL

Three consumers read the same tables: the `RepoGraphStore` seam (Rust callers), `PgAst` (the
existing `AstBackend` verbs), and the `repo_graph` tool (the model). Every read takes one
`Scope { tenant, repo_id, snapshot_id }`; there is no read helper without it.

## `RepoGraphStore` seam (agent-core, RK-01)

Tenant-implicit like `DigestStore` (`crates/agent-core/src/lib.rs:2096`): the Postgres impl
exposes `with_tenant(&str) -> Self` and the runtime wraps it in `PerTenant`
(`crates/agent-runtime/src/tenant.rs:71`). The fleet builds a view from `row.user`.

| Group | Methods |
|---|---|
| Repos | `repo_put(RepoSpec) -> RepoId`, `repo_get(slug)`, `repos()` |
| Snapshots | `snapshot_begin(repo_id, sha, extractor_version) -> SnapshotId`, `snapshot_write(id, &RepoGraph)`, `snapshot_finish(id, status, report)`, `snapshot_find(repo_id, sha)`, `snapshot_latest(repo_id)`, `snapshots(repo_id, limit)`, `snapshot_delete_older_than(repo_id, keep)`, `snapshot_diff(a, b) -> GraphDiff` |
| Reads | `nodes_by_key(scope, &[key])`, `nodes_by_file(scope, &[path])`, `nodes_by_name(scope, name, kind?, limit)`, `neighbors(scope, ids, kind, dir, hops ≤ 4, cap)`, `blast_radius(scope, files, hops ≤ 3, cap)`, `tests_covering(scope, ids, hops ≤ 3, cap)`, `path_between(scope, a, b, max_hops ≤ 6, max_paths)`, `shape(scope) -> Shape` |
| Inventory | `features_put(repo_id, &[Feature])`, `features(repo_id, kind?)`, `summary_put(Summary)` (validates citations, fail-closed), `summaries(repo_id, kind, subject?)`, `embedding_put`, `embeddings(repo_id, subject_kind, model)` |
| Facts | `facts_put(scope, &[Fact])`, `facts(scope, kind, limit)`, `history_put`, `history(scope, files)` |

`MemRepoGraph` in `agent-testkit` implements the whole trait over `HashMap`s so every consumer
is testable without Postgres.

## `PgAst: AstBackend` (RK-08)

`[ast] backends = ["pg"]` registers a `PgAst` engine behind the existing seam
(`crates/agent-core/src/lib.rs:4909`), routed by `DispatchAst` like the others
(`crates/agent-ast/src/lib.rs:77`). It reads the latest ready snapshot for the configured repo.

| `AstBackend` verb | Store call |
|---|---|
| `find_symbol` | `nodes_by_name` |
| `find_implementations` | `neighbors(implements, in)` then `impl_for` to the self type |
| `find_interface` | `neighbors(implements, out)` |
| `find_callers` / `find_callees` | `neighbors(calls, in / out)`; the capability is advertised only when the snapshot has `calls` edges for that language |
| `blast_radius` | `blast_radius(files)` |
| `find_callchain` | `path_between` |
| `find_dependency_path` | `path_between` over `depends_on` and `imports` |

`Symbol.id: u32` in the seam maps to `node_id` through a per-process intern table that is reset
on `reindex`; the tool output shows keys, never the interned ids.

## The `repo_graph` tool (feature `tool-repo-graph`, RK-08)

One tool with a `question` discriminator so the preamble lists one name and the model reads one
description. Output at most 8 KiB, nodes rendered as `` `key` (file:line) `` so the model can
cite keys back.

| `question` | Args | Answer |
|---|---|---|
| `Shape` | — | crates, files, fn / struct / trait / test counts, seams, tools, snapshot sha, extractors |
| `Seams` | `crate?` | traits with `attrs.is_seam`, each with its impl count and registry keys |
| `Tests` | `crate?` | test counts by class, `#[ignore]` count, crates with zero `adversarial_` cases |
| `Central` | `kind?`, `limit ≤ 50` | top in-degree nodes over `calls` + `references` |
| `DependencyPath` | `from`, `to` | `depends_on` path between crates |
| `TestsCovering` | `keys[] ≤ 20`, `hops ≤ 3` | tests reaching the keys |
| `Dependents` | `crate` | crates that depend on it, and their pub items that import from it |
| `Orphans` | `crate?` | pub items with no in-edges (needs SCIP; says so otherwise) |
| `Node` | `key`, `hops ≤ 2` | the node, its version, and its neighbourhood by edge kind |
| `Similar` | `name`, `limit ≤ 10` | `name_tokens &&` match, then embedding cosine when RK-11 is present |
| `Feature` | `key` | inventory row + evidence + summary (labelled model-written, cited) |
| `EntryPoints` | — | binaries, subcommands, `--serve-*`, listeners with ports (RK-15) |
| `Schemas` | `store?` | tables and protos with columns / rpcs (RK-15) |
| `ConfigKeys` | `section?` | config keys with defaults and docs (RK-15) |
| `Debt` | `crate?` | counts by category with `file:line` (RK-15) |
| `HotSpots` | `crate?` | top churn / lowest bus factor files (RK-16) |
| `InFlight` | — | recent commits by crate, open STATUS rows (RK-16) |
| `Recipe` | `kind` | ordered touch set for that change kind with example commits (RK-16) |
| `Duplicates` | `key` | `similar_to` neighbours with Jaccard (RK-17) |
| `Guards` | — | guard funnels and guard gaps (RK-17) |

Every argument is model-supplied: keys and names pass `safe_segment`-style validation (ASCII,
no whitespace, ≤ 512 B); `hops`, `limit` are clamped; an unknown key yields an empty answer
that does not echo the key.

## SQL sketches

All run inside a transaction that starts with `SET LOCAL statement_timeout = '3s'`. `$1..$3`
are the `Scope`. `references` is excluded from path queries because it is the densest edge kind.

**Callers, N hops**

```sql
WITH RECURSIVE walk(node_id, depth) AS (
  SELECT unnest($4::bigint[]), 0
  UNION
  SELECT e.src_id, w.depth + 1
  FROM walk w
  JOIN graph_edges e
    ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
   AND e.kind = 'calls' AND e.dst_id = w.node_id
  WHERE w.depth < $5                      -- hops ≤ 4
)
SELECT n.node_key, v.file, v.line_start, min(w.depth) AS depth
FROM walk w
JOIN graph_nodes n         ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = w.node_id
JOIN graph_node_versions v ON v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 AND v.node_id = w.node_id
GROUP BY n.node_key, v.file, v.line_start
ORDER BY depth, n.node_key
LIMIT $6;                                  -- cap
```

**Blast radius from a file seed**

```sql
WITH seed AS (
  SELECT node_id FROM graph_node_versions
  WHERE tenant = $1 AND repo_id = $2 AND snapshot_id = $3 AND file = ANY($4)
),
walk(node_id, depth) AS (
  SELECT node_id, 0 FROM seed
  UNION
  SELECT e.src_id, w.depth + 1
  FROM walk w JOIN graph_edges e
    ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
   AND e.kind IN ('calls','imports','implements') AND e.dst_id = w.node_id
  WHERE w.depth < $5
)
SELECT DISTINCT v.file
FROM walk w JOIN graph_node_versions v
  ON v.tenant = $1 AND v.repo_id = $2 AND v.snapshot_id = $3 AND v.node_id = w.node_id
ORDER BY v.file LIMIT $6;
```

**Tests covering**

```sql
WITH RECURSIVE reach(node_id, depth) AS (
  SELECT unnest($4::bigint[]), 0
  UNION
  SELECT e.src_id, r.depth + 1
  FROM reach r JOIN graph_edges e
    ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
   AND e.kind = 'calls' AND e.dst_id = r.node_id
  WHERE r.depth < $5
)
SELECT DISTINCT t.node_key, te.attrs->>'via' AS via
FROM reach r
JOIN graph_edges te ON te.tenant = $1 AND te.repo_id = $2 AND te.snapshot_id = $3
                   AND te.kind = 'tests' AND te.dst_id = r.node_id
JOIN graph_nodes t  ON t.tenant = $1 AND t.repo_id = $2 AND t.node_id = te.src_id AND t.kind = 'test'
ORDER BY t.node_key LIMIT $6;
```

**Path between two nodes**

```sql
WITH RECURSIVE p(node_id, path, depth) AS (
  SELECT $4::bigint, ARRAY[$4::bigint], 0
  UNION ALL
  SELECT e.dst_id, p.path || e.dst_id, p.depth + 1
  FROM p JOIN graph_edges e
    ON e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
   AND e.kind IN ('calls','imports','depends_on','contains') AND e.src_id = p.node_id
  WHERE p.depth < $6                       -- max_hops ≤ 6
    AND NOT (e.dst_id = ANY(p.path))       -- cycle guard
)
SELECT path FROM p WHERE node_id = $5 ORDER BY depth LIMIT $7;   -- max_paths
```

**Central symbols**

```sql
SELECT n.node_key, count(*) AS in_degree
FROM graph_edges e
JOIN graph_nodes n ON n.tenant = $1 AND n.repo_id = $2 AND n.node_id = e.dst_id
WHERE e.tenant = $1 AND e.repo_id = $2 AND e.snapshot_id = $3
  AND e.kind IN ('calls','references') AND ($4::text IS NULL OR n.kind = $4)
GROUP BY n.node_key ORDER BY in_degree DESC, n.node_key LIMIT $5;
```

**Snapshot diff** (`snapshot_diff`): a full outer join of `graph_node_versions` for two
snapshots on `node_id`, classifying added / removed / `sig_hash` changed / `body_hash` changed,
plus edge set differences by `(kind, src_id, dst_id)`.

## gRPC (RK-13, v2)

`RepoGraphService` mirrors the seam one-to-one, additive in `agent-proto` so `buf breaking`
passes against the committed baseline. It is a `scoped` service in `test/mt-audit/manifest.toml`
(`scoped` = every RPC calls `identity_key` + `run_scoped`, `test/mt-audit/manifest.toml:10`) and
in `identity_policy::class_of` (`crates/agent-grpc/src/server/identity_policy.rs:42`). Port from
`nix/constants.nix`; served by `agent --serve-repo-graph`; dialled by `[repo_graph] store = "grpc"`.

## CLI (RK-06, RK-10, RK-11, RK-12)

```
agent repo add     --tenant <t> --repo <slug> [--forge …] [--remote …] [--profile <json>]
agent repo index   --repo <slug> [--sha <rev>] [--root <path>] [--extractors a,b] [--strict]
agent repo status  --repo <slug>                # snapshots, counts, last error
agent repo diff    --repo <slug> --from <sha> --to <sha>
agent repo shape   --repo <slug>                # the Shape question, for humans
agent repo inventory --repo <slug>              # skeleton rows (RK-10)
agent repo summarize --repo <slug> [--dirty-only] [--kind …]   # LLM step (RK-11)
agent repo brief   --repo <slug> --goal "<text>" [--crates a,b] [--print]   # RK-12
```

`repo add` is required before `index`; nothing creates a repo row from model-supplied input.
