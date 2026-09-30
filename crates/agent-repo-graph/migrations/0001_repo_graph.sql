-- 0001_repo_graph.sql — the repo-knowledge graph tier (RK-02).
--
-- The DDL from docs/design/repo-knowledge/01-schema.md, applied by the versioned
-- runner in src/postgres.rs (copied from PgDigests): one transaction under a
-- transaction-scoped advisory lock, recorded in `_repo_graph_migrations`. sqlx 0.8
-- without `macros`/`migrate`, so this is plain SQL run via `sqlx::raw_sql`.
--
-- `CREATE TABLE IF NOT EXISTS` throughout so the migration is re-runnable (a DB
-- carrying the schema but no ledger re-runs 0001 inertly). Leads with `tenants`
-- (same shape as the config-store tier) so the tier installs on a database that has
-- never had the config store. Every table carries `tenant TEXT` first, in every
-- PK/FK/index; the app enforces `WHERE tenant = $1` (RLS is deferred to RK-14).
--
-- UNIQUE constraints are NAMED (`repos_slug_key`, `graph_snapshots_identity_key`,
-- `graph_nodes_key_key`) so `map_db` can key a typed error on the rule, not the row.

CREATE TABLE IF NOT EXISTS tenants (tenant TEXT PRIMARY KEY);

CREATE TABLE IF NOT EXISTS repos (
  tenant          TEXT   NOT NULL REFERENCES tenants(tenant),
  repo_id         BIGINT GENERATED ALWAYS AS IDENTITY,
  slug            TEXT   NOT NULL,            -- safe_segment; equals FleetSession.repo (owner__repo)
  forge           TEXT   NOT NULL DEFAULT '',
  remote_url      TEXT   NOT NULL DEFAULT '',
  default_branch  TEXT   NOT NULL DEFAULT 'main',
  profile         JSONB  NOT NULL DEFAULT '{}'::jsonb,  -- seam_crate, tool_trait, config_root, testkit_crate, recipe_rules
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant, repo_id),
  CONSTRAINT repos_slug_key UNIQUE (tenant, slug),
  CHECK (length(slug) BETWEEN 1 AND 128)
);

CREATE TABLE IF NOT EXISTS graph_snapshots (
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
  CONSTRAINT graph_snapshots_identity_key UNIQUE (tenant, repo_id, commit_sha, extractor_version)
);
CREATE INDEX IF NOT EXISTS graph_snapshots_ready
  ON graph_snapshots (tenant, repo_id, built_at DESC) WHERE status = 'ready';

CREATE TABLE IF NOT EXISTS graph_nodes (
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
  CONSTRAINT graph_nodes_key_key UNIQUE (tenant, repo_id, node_key),
  FOREIGN KEY (tenant, repo_id) REFERENCES repos(tenant, repo_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS graph_nodes_name   ON graph_nodes (tenant, repo_id, name);
CREATE INDEX IF NOT EXISTS graph_nodes_tokens ON graph_nodes USING GIN (name_tokens);

CREATE TABLE IF NOT EXISTS graph_node_versions (
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
CREATE INDEX IF NOT EXISTS graph_node_versions_file ON graph_node_versions (tenant, repo_id, snapshot_id, file);

CREATE TABLE IF NOT EXISTS graph_edges (
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
CREATE INDEX IF NOT EXISTS graph_edges_dst ON graph_edges (tenant, repo_id, snapshot_id, dst_id, kind);
CREATE INDEX IF NOT EXISTS graph_edges_src ON graph_edges (tenant, repo_id, snapshot_id, src_id, kind);
