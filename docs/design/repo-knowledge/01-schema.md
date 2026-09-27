# 01 — Schema: the Postgres tables, the node-key grammar, the closed kind sets

Crate: `agent-repo-graph`. Migrations: `migrations/0001_repo_graph.sql` (RK-02),
`migrations/0002_inventory.sql` (RK-10), `migrations/0003_repo_facts.sql` (RK-15).

## Migration runner

Copied from `PgDigests` (`crates/agent-digest/src/postgres.rs:44-119`): an embedded
`MIGRATIONS: &[(i64, &str)]` built with `include_str!`, one transaction per migration under
`pg_advisory_xact_lock(<own key>)`, a per-crate ledger `_repo_graph_migrations`, and
`connect(dsn, pool_max, migrate_on_start)` / `connect_lazy` / `from_pool` constructors. sqlx 0.8
without `macros` or `migrate` (`Cargo.toml:144`), so every statement is a plain `sqlx::query`.

`0001` begins with `CREATE TABLE IF NOT EXISTS tenants (tenant TEXT PRIMARY KEY)` so the tier
runs on a database that has never had the config store installed; when both tiers share a
database the statement is a no-op (same shape as
`crates/agent-config-store/migrations/0001_config_store.sql:10`).

## Tables (0001)

```sql
CREATE TABLE IF NOT EXISTS tenants (tenant TEXT PRIMARY KEY);

CREATE TABLE repos (
  tenant          TEXT   NOT NULL REFERENCES tenants(tenant),
  repo_id         BIGINT GENERATED ALWAYS AS IDENTITY,
  slug            TEXT   NOT NULL,            -- safe_segment; equals FleetSession.repo (owner__repo)
  forge           TEXT   NOT NULL DEFAULT '',
  remote_url      TEXT   NOT NULL DEFAULT '',
  default_branch  TEXT   NOT NULL DEFAULT 'main',
  profile         JSONB  NOT NULL DEFAULT '{}'::jsonb,  -- seam_crate, tool_trait, config_root, testkit_crate, recipe_rules
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant, repo_id),
  UNIQUE (tenant, slug),
  CHECK (length(slug) BETWEEN 1 AND 128)
);

CREATE TABLE graph_snapshots (
  tenant            TEXT   NOT NULL,
  repo_id           BIGINT NOT NULL,
  snapshot_id       BIGINT GENERATED ALWAYS AS IDENTITY,
  commit_sha        TEXT   NOT NULL CHECK (commit_sha ~ '^[0-9a-f]{40}$'),
  extractors        JSONB  NOT NULL,            -- ["rust-syn","cargo","docs"] as run
  extractor_version TEXT   NOT NULL,            -- joined "name@ver" list, part of identity
  graph_hash        TEXT   NOT NULL DEFAULT '', -- sha256 over sorted nodes+edges
  node_count        INT    NOT NULL DEFAULT 0,
  edge_count        INT    NOT NULL DEFAULT 0,
  status            TEXT   NOT NULL CHECK (status IN ('building','ready','failed')),
  reason            TEXT   NOT NULL DEFAULT '',
  built_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
  duration_ms       BIGINT NOT NULL DEFAULT 0,
  PRIMARY KEY (tenant, repo_id, snapshot_id),
  FOREIGN KEY (tenant, repo_id) REFERENCES repos(tenant, repo_id) ON DELETE CASCADE,
  UNIQUE (tenant, repo_id, commit_sha, extractor_version)
);
CREATE INDEX graph_snapshots_ready
  ON graph_snapshots (tenant, repo_id, built_at DESC) WHERE status = 'ready';

CREATE TABLE graph_nodes (
  tenant      TEXT   NOT NULL,
  repo_id     BIGINT NOT NULL,
  node_id     BIGINT NOT NULL,                 -- first 8 bytes of sha256(node_key), big-endian, as i64
  node_key    TEXT   NOT NULL CHECK (length(node_key) BETWEEN 1 AND 512),
  kind        TEXT   NOT NULL CHECK (kind IN (
                'repo','crate','package','module','file','fn','method','struct','enum','trait',
                'impl','type','const','static','macro','test','feature','doc',
                'proto_service','proto_rpc','proto_message','table','config_key','metric','span')),
  lang        TEXT   NOT NULL,                 -- 'rust' | 'go' | 'md' | 'proto' | 'sql' | 'toml' | ''
  name        TEXT   NOT NULL CHECK (length(name) BETWEEN 1 AND 256),
  name_tokens TEXT[] NOT NULL DEFAULT '{}',    -- lower-cased snake/camel split of name
  qualifier   TEXT   NOT NULL DEFAULT '',      -- crate::mod path, package path, or dir
  PRIMARY KEY (tenant, repo_id, node_id),
  UNIQUE (tenant, repo_id, node_key),
  FOREIGN KEY (tenant, repo_id) REFERENCES repos(tenant, repo_id) ON DELETE CASCADE
);
CREATE INDEX graph_nodes_name   ON graph_nodes (tenant, repo_id, name);
CREATE INDEX graph_nodes_tokens ON graph_nodes USING GIN (name_tokens);

CREATE TABLE graph_node_versions (
  tenant      TEXT   NOT NULL,
  repo_id     BIGINT NOT NULL,
  snapshot_id BIGINT NOT NULL,
  node_id     BIGINT NOT NULL,
  file        TEXT   NOT NULL,                 -- repo-relative, confined
  line_start  INT    NOT NULL DEFAULT 0,
  line_end    INT    NOT NULL DEFAULT 0,
  sig_hash    TEXT   NOT NULL DEFAULT '',      -- signature tokens, docs/attrs stripped
  body_hash   TEXT   NOT NULL DEFAULT '',
  exported    BOOL   NOT NULL DEFAULT false,
  attrs       JSONB  NOT NULL DEFAULT '{}'::jsonb CHECK (pg_column_size(attrs) <= 4096),
  PRIMARY KEY (tenant, repo_id, snapshot_id, node_id),
  FOREIGN KEY (tenant, repo_id, snapshot_id) REFERENCES graph_snapshots(tenant, repo_id, snapshot_id) ON DELETE CASCADE,
  FOREIGN KEY (tenant, repo_id, node_id)     REFERENCES graph_nodes(tenant, repo_id, node_id)
);
CREATE INDEX graph_node_versions_file ON graph_node_versions (tenant, repo_id, snapshot_id, file);

CREATE TABLE graph_edges (
  tenant      TEXT   NOT NULL,
  repo_id     BIGINT NOT NULL,
  snapshot_id BIGINT NOT NULL,
  kind        TEXT   NOT NULL CHECK (kind IN (
                'contains','defined_in','imports','implements','impl_for','depends_on','gated_by',
                'tests','documents','calls','references','co_changes_with','similar_to')),
  src_id      BIGINT NOT NULL,
  dst_id      BIGINT NOT NULL,
  weight      REAL   NOT NULL DEFAULT 1.0,
  attrs       JSONB  NOT NULL DEFAULT '{}'::jsonb CHECK (pg_column_size(attrs) <= 1024),
  PRIMARY KEY (tenant, repo_id, snapshot_id, kind, src_id, dst_id),
  FOREIGN KEY (tenant, repo_id, snapshot_id) REFERENCES graph_snapshots(tenant, repo_id, snapshot_id) ON DELETE CASCADE
);
CREATE INDEX graph_edges_dst ON graph_edges (tenant, repo_id, snapshot_id, dst_id, kind);
CREATE INDEX graph_edges_src ON graph_edges (tenant, repo_id, snapshot_id, src_id, kind);
```

`co_changes_with` and `similar_to` are in the CHECK set from 0001 so that 0003 does not have to
alter a constraint; no extractor writes them before RK-16 / RK-17.

## Tables (0002, inventory)

```sql
CREATE TABLE repo_features (
  tenant        TEXT   NOT NULL,
  repo_id       BIGINT NOT NULL,
  feature_key   TEXT   NOT NULL CHECK (length(feature_key) BETWEEN 1 AND 256),
  kind          TEXT   NOT NULL CHECK (kind IN ('crate','seam','tool','cargo_feature','config_section','capability')),
  name          TEXT   NOT NULL,
  parent_key    TEXT   NOT NULL DEFAULT '',
  subgraph_hash TEXT   NOT NULL DEFAULT '',
  last_snapshot BIGINT NOT NULL,
  attrs         JSONB  NOT NULL DEFAULT '{}'::jsonb CHECK (pg_column_size(attrs) <= 4096),
  PRIMARY KEY (tenant, repo_id, feature_key),
  FOREIGN KEY (tenant, repo_id) REFERENCES repos(tenant, repo_id) ON DELETE CASCADE
);

CREATE TABLE repo_feature_evidence (
  tenant      TEXT   NOT NULL,
  repo_id     BIGINT NOT NULL,
  feature_key TEXT   NOT NULL,
  node_id     BIGINT NOT NULL,
  role        TEXT   NOT NULL CHECK (role IN ('defines','implements','tests','gates','documents','exposes')),
  PRIMARY KEY (tenant, repo_id, feature_key, node_id, role),
  FOREIGN KEY (tenant, repo_id, feature_key) REFERENCES repo_features(tenant, repo_id, feature_key) ON DELETE CASCADE
);

CREATE TABLE repo_summaries (
  tenant        TEXT   NOT NULL,
  repo_id       BIGINT NOT NULL,
  kind          TEXT   NOT NULL CHECK (kind IN (
                  'architecture','crate','feature',
                  'conventions','glossary','playbook','pitfalls','overview_l0')),
  subject_key   TEXT   NOT NULL,               -- 'repo', feature_key, or 'playbook:<name>'
  subgraph_hash TEXT   NOT NULL,
  snapshot_id   BIGINT NOT NULL,
  text          TEXT   NOT NULL CHECK (length(text) <= 16384),
  citations     TEXT[] NOT NULL DEFAULT '{}',  -- node_keys verified against snapshot_id
  model         TEXT   NOT NULL DEFAULT '',
  tokens_in     BIGINT NOT NULL DEFAULT 0,
  tokens_out    BIGINT NOT NULL DEFAULT 0,
  ts_ms         BIGINT NOT NULL,
  PRIMARY KEY (tenant, repo_id, kind, subject_key),
  FOREIGN KEY (tenant, repo_id) REFERENCES repos(tenant, repo_id) ON DELETE CASCADE
);

CREATE TABLE repo_embeddings (
  tenant        TEXT   NOT NULL,
  repo_id       BIGINT NOT NULL,
  subject_kind  TEXT   NOT NULL CHECK (subject_kind IN ('summary','symbol')),
  subject_key   TEXT   NOT NULL,
  model         TEXT   NOT NULL,
  dim           INT    NOT NULL CHECK (dim BETWEEN 1 AND 4096),
  vec           REAL[] NOT NULL CHECK (cardinality(vec) = dim),
  content_hash  TEXT   NOT NULL,
  ts_ms         BIGINT NOT NULL,
  PRIMARY KEY (tenant, repo_id, subject_kind, subject_key),
  FOREIGN KEY (tenant, repo_id) REFERENCES repos(tenant, repo_id) ON DELETE CASCADE
);
```

The `kind` CHECK on `repo_summaries` already lists the RK-18 kinds so 0002 is not altered later.

## Tables (0003, profile and history)

```sql
CREATE TABLE repo_facts (
  tenant      TEXT   NOT NULL,
  repo_id     BIGINT NOT NULL,
  snapshot_id BIGINT NOT NULL,
  kind        TEXT   NOT NULL CHECK (kind IN (
                'cli','factory','api_summary','hotspot_complexity','debt','gate','command','double',
                'inflight','recipe','chokepoint','guard','guard_gap','orphan')),
  key         TEXT   NOT NULL CHECK (length(key) BETWEEN 1 AND 512),
  attrs       JSONB  NOT NULL DEFAULT '{}'::jsonb CHECK (pg_column_size(attrs) <= 4096),
  PRIMARY KEY (tenant, repo_id, snapshot_id, kind, key),
  FOREIGN KEY (tenant, repo_id, snapshot_id) REFERENCES graph_snapshots(tenant, repo_id, snapshot_id) ON DELETE CASCADE
);

CREATE TABLE repo_history (
  tenant          TEXT   NOT NULL,
  repo_id         BIGINT NOT NULL,
  snapshot_id     BIGINT NOT NULL,
  file            TEXT   NOT NULL,
  commits_90d     INT    NOT NULL DEFAULT 0,
  last_touched_ms BIGINT NOT NULL DEFAULT 0,
  unique_authors  INT    NOT NULL DEFAULT 0,
  bus_factor      INT    NOT NULL DEFAULT 0,   -- authors needed to reach 50 % of commits
  churn_slope     REAL   NOT NULL DEFAULT 0,   -- commits/week trend over the window
  PRIMARY KEY (tenant, repo_id, snapshot_id, file),
  FOREIGN KEY (tenant, repo_id, snapshot_id) REFERENCES graph_snapshots(tenant, repo_id, snapshot_id) ON DELETE CASCADE
);
```

## Node-key grammar

Keys are ASCII, at most 512 bytes, with no whitespace. `<crate>` is the Cargo package name with
`-` replaced by `_` (the crate's Rust identifier). Paths are repo-relative and confined.

| Kind | Key | Example |
|---|---|---|
| repo | `repo:<slug>` | `repo:randomizedcoder__agent-seddon` |
| file | `file:<path>` | `file:crates/agent-core/src/security.rs` |
| doc | `doc:<path>` | `doc:docs/extending.md` |
| crate | `rust:crate:<crate>` | `rust:crate:agent_core` |
| feature | `rust:feature:<crate>/<feature>` | `rust:feature:agent_digest/postgres` |
| module | `rust:mod:<crate>::<mod path>` | `rust:mod:agent_core::security` |
| fn, struct, enum, trait, type, const, static, macro | `rust:<kind>:<crate>::<mod path>::<Name>` | `rust:fn:agent_core::security::confine` |
| impl | `rust:impl:<crate>::<mod path>::<SelfType>#<TraitPath or ->` | `rust:impl:agent_digest::postgres::PgDigests#DigestStore`, `rust:impl:agent_ast::graph::Graph#-` |
| method | `rust:method:<crate>::<mod path>::<SelfType>#<TraitPath or ->::<name>` | `rust:method:agent_digest::postgres::PgDigests#DigestStore::put` |
| test | `rust:test:<crate>::<mod path>::<fn>[::<case>]` | `rust:test:agent_tools::edit::tests::rejects_traversal::adversarial_dotdot` |
| Go package | `go:package:<import path>` | `go:package:example.com/m/pkg` |
| Go func, struct, interface, type | `go:<kind>:<import path>.<Name>` | `go:func:example.com/m/pkg.Serve` |
| Go method | `go:method:<import path>.<Recv>.<Name>` | `go:method:example.com/m/pkg.Server.Close` |
| Go test | `go:test:<import path>.<TestName>` | `go:test:example.com/m/pkg.TestServe` |
| proto service / rpc / message | `proto:service:<pkg>.<Svc>`, `proto:rpc:<pkg>.<Svc>/<Rpc>`, `proto:message:<pkg>.<Msg>` | `proto:rpc:agent.v1.Search/Query` |
| table | `sql:table:<store>/<name>` | `sql:table:digests/digests`, `sql:table:clickhouse/agent_review_collectors` |
| config_key | `cfg:<section>.<key>` | `cfg:review.nearby` |
| metric, span | `metric:<name>`, `span:<name>` | `metric:agent_repo_graph_index_seconds` |

**Duplicate keys.** Two items can produce the same key when `#[cfg]` splits them (this workspace
has 669 `#[cfg(feature)]` attributes). The second and later items get an `@<sha8>` suffix over
their sorted cfg tokens; `attrs.dup = true` and `attrs.cfg` record the split. A collision that
is not cfg-explained (two items, same key, same cfg) is a validation error and fails the snapshot.

**`name_tokens`.** Lower-cased split on `_`, `-`, `.` and camel-case boundaries, deduplicated,
at most 16 tokens. `PgDigests` → `{pg, digests}`; `find_changed_callers` →
`{find, changed, callers}`. Used by `Similar{name}` and by the review collector's
"does this name already exist?" query.

## Node kinds v1 and what is an attribute

Kinds are the closed set in the CHECK. Everything else is an attribute so the kind set stays
small and the model's mental model stays simple:

| Fact | Where |
|---|---|
| A trait is a seam | `attrs.is_seam = true` (trait in `profile.seam_crate` with `#[async_trait]`, or listed in `profile.seams`) |
| A struct is a tool | `attrs.tool_name = "<literal>"` from `fn name(&self) -> &str { "<literal>" }` in its `impl <profile.tool_trait>` |
| A config struct field | kind `config_key`, `attrs.default`, `attrs.doc`, `attrs.section` |
| An item is `pub` at crate root or re-exported | `attrs.api = true` |
| A test's class | `attrs.test_class ∈ {positive, negative, corner, boundary, adversarial, other}`, `attrs.ignored`, `attrs.ignore_reason` |
| A doc comment | `attrs.doc`: first paragraph, ≤ 512 B, absent when `scan_for_injection` fires |
| Registry key | `attrs.registry_key = "<config string>"` on the impl that `register_builtins` maps it to |
| Entry point | `attrs.entry_point ∈ {bin, main, subcommand, serve}` |

## Edge kinds v1

| Kind | src → dst | Source | Notes |
|---|---|---|---|
| contains | crate → module → item; package → item; file → item | syn, cargo, go | structural |
| defined_in | item → file | syn, go, proto, sql | one per node version |
| imports | module → item or module | `use` / Go imports | `attrs.resolved = false` when the target is external or missing |
| implements | impl → trait; Go type → interface; tonic impl → proto_service | syn, go-graph, proto | |
| impl_for | impl → self type | syn | |
| depends_on | crate → crate | Cargo.toml path / workspace deps | `attrs.dev`, `attrs.optional`, `attrs.features` |
| gated_by | item → feature | `#[cfg(feature = …)]`, `all` / `any` flattened | |
| tests | test → target | name heuristic (`attrs.via = "name"`), SCIP (`via = "scip"`) | |
| documents | doc → crate, file, or item | markdown links and backtick paths | |
| calls | fn → fn | go-graph CHA now; SCIP for Rust | |
| references | item → item | SCIP | excluded from path queries (dense) |
| co_changes_with | file ↔ file | git log (RK-16) | `weight` = confidence, `attrs.count` |
| similar_to | fn ↔ fn | winnowing (RK-17) | `weight` = Jaccard |

Dangling edges (an endpoint not in the snapshot) are dropped at validation and counted in
`ExtractReport.dropped_edges`.

## Size estimate for this workspace

| | Nodes | Edges | Bytes per snapshot |
|---|---|---|---|
| syn + cargo + docs | ~17 k (≈ 19.5 k with rstest cases) | ~30 k | ≈ 10 MB |
| + profile (RK-15) | + ~1.5 k | + ~3 k | + 1 MB |
| + SCIP | same | 120–180 k | ≈ 45 MB |

Node bodies are shared across snapshots, so a second snapshot of the same commit range costs
mostly `graph_node_versions` and `graph_edges`. Retention: keep the newest N ready snapshots per
repo (default 10) and delete `graph_nodes` rows no retained version references.

## Deferred: row-level security sketch

Cross-tier, filed under multi-tenancy plane 02 (RK-14). When taken, every table above gets:

```sql
ALTER TABLE graph_nodes ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON graph_nodes
  USING (tenant = current_setting('app.tenant', true));
```

and the store issues `SET LOCAL app.tenant = $1` at the top of each transaction. The app-side
`WHERE tenant = $1` stays; RLS is defence in depth.
