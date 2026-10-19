//! Connection-scoped writes and pinned, repeatable-read query views.
use super::*;

impl PgRepoGraph {
    pub(super) async fn snapshot_begin_tx(
        &self,
        conn: &mut sqlx::PgConnection,
        begin: &SnapshotBegin,
    ) -> RepoGraphResult<SnapshotId> {
        begin.validate()?;
        let now = sql::ms((self.now_ms)());
        let extractors = serde_json::to_string(&begin.extractors)
            .map_err(|e| backend(&format!("extractors: {e}")))?;
        if sqlx::query(sql::REPO_EXISTS)
            .bind(&self.tenant)
            .bind(begin.repo.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(map_db)?
            .is_none()
        {
            return Err(RepoGraphError::NotFound);
        }
        // The identity is unique; at most one row. A live one conflicts, a failed one is replaced.
        if let Some(row) = sqlx::query(sql::BEGIN_FIND)
            .bind(&self.tenant)
            .bind(begin.repo.0)
            .bind(&begin.commit_sha)
            .bind(&begin.extractor_version)
            .fetch_optional(&mut *conn)
            .await
            .map_err(map_db)?
        {
            let status: String = col(&row, "status")?;
            match status.as_str() {
                "building" | "ready" => {
                    return Err(RepoGraphError::Conflict("snapshot identity".to_string()));
                }
                "failed" => {
                    let sid: i64 = col(&row, "snapshot_id")?;
                    sqlx::query(sql::SNAPSHOT_DELETE_ONE)
                        .bind(&self.tenant)
                        .bind(sid)
                        .execute(&mut *conn)
                        .await
                        .map_err(map_db)?;
                }
                _ => return Err(backend("snapshot status: unknown value")),
            }
        }
        let id: i64 = sqlx::query_scalar(sql::BEGIN_INSERT)
            .bind(&self.tenant)
            .bind(begin.repo.0)
            .bind(&begin.commit_sha)
            .bind(&extractors)
            .bind(&begin.extractor_version)
            .bind(now)
            .fetch_one(&mut *conn)
            .await
            .map_err(map_db)?;
        Ok(SnapshotId(id))
    }
    pub(super) async fn snapshot_write_tx(
        &self,
        conn: &mut sqlx::PgConnection,
        id: SnapshotId,
        graph: &RepoGraph,
    ) -> RepoGraphResult<()> {
        if graph.nodes().len() > MAX_SNAPSHOT_NODES {
            return Err(RepoGraphError::TooLong("nodes".to_string()));
        }
        if graph.edges().len() > MAX_SNAPSHOT_EDGES {
            return Err(RepoGraphError::TooLong("edges".to_string()));
        }
        let na = sql::node_arrays(graph);
        let ea = sql::edge_arrays(graph);
        // Per-row `name_tokens` bound as a comma-joined `text[]` (tokens are `[a-z0-9]`, no comma),
        // split back with `string_to_array` — avoids the `text[][]` UNNEST flattening trap.
        let toks: Vec<String> = na.tokens.iter().map(|t| t.join(",")).collect();

        let row = sqlx::query(sql::SNAPSHOT_LOCK)
            .bind(&self.tenant)
            .bind(id.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(map_db)?
            .ok_or(RepoGraphError::NotFound)?;
        let repo_id: i64 = col(&row, "repo_id")?;
        let status: String = col(&row, "status")?;
        if status != "building" {
            return Err(RepoGraphError::Conflict(
                "snapshot not building".to_string(),
            ));
        }

        for r in sql::chunk_ranges(na.ids.len(), sql::WRITE_CHUNK) {
            // Bodies: insert new, keep existing (shared across snapshots).
            sqlx::query(sql::NODES_INSERT)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(&na.ids[r.clone()])
                .bind(&na.keys[r.clone()])
                .bind(&na.kinds[r.clone()])
                .bind(&na.langs[r.clone()])
                .bind(&na.names[r.clone()])
                .bind(&toks[r.clone()])
                .bind(&na.qualifiers[r.clone()])
                .execute(&mut *conn)
                .await
                .map_err(map_db)?;
            // A distinct key mapped onto a stored id is a collision: the tx rolls back untouched.
            let collisions: i64 = sqlx::query_scalar(sql::COLLISION_CHECK)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(&na.ids[r.clone()])
                .bind(&na.keys[r.clone()])
                .fetch_one(&mut *conn)
                .await
                .map_err(map_db)?;
            if collisions > 0 {
                return Err(RepoGraphError::Conflict("node id collision".to_string()));
            }
        }
        for r in sql::chunk_ranges(na.ids.len(), sql::WRITE_CHUNK) {
            sqlx::query(sql::VERSIONS_INSERT)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(id.0)
                .bind(&na.ids[r.clone()])
                .bind(&na.files[r.clone()])
                .bind(&na.line_starts[r.clone()])
                .bind(&na.line_ends[r.clone()])
                .bind(&na.sig_hashes[r.clone()])
                .bind(&na.body_hashes[r.clone()])
                .bind(&na.exported[r.clone()])
                .bind(&na.attrs[r.clone()])
                .execute(&mut *conn)
                .await
                .map_err(map_db)?;
        }
        for r in sql::chunk_ranges(ea.kinds.len(), sql::WRITE_CHUNK) {
            sqlx::query(sql::EDGES_INSERT)
                .bind(&self.tenant)
                .bind(repo_id)
                .bind(id.0)
                .bind(&ea.kinds[r.clone()])
                .bind(&ea.src_ids[r.clone()])
                .bind(&ea.dst_ids[r.clone()])
                .bind(&ea.weights[r.clone()])
                .bind(&ea.attrs[r.clone()])
                .execute(&mut *conn)
                .await
                .map_err(map_db)?;
        }
        sqlx::query(sql::WRITE_META)
            .bind(&self.tenant)
            .bind(id.0)
            .bind(graph.graph_hash())
            .bind(sql::i32_of(graph.nodes().len()))
            .bind(sql::i32_of(graph.edges().len()))
            .execute(&mut *conn)
            .await
            .map_err(map_db)?;
        Ok(())
    }
    pub(super) async fn snapshot_finish_tx(
        &self,
        conn: &mut sqlx::PgConnection,
        id: SnapshotId,
        status: SnapshotStatus,
        reason: &str,
        report: &ExtractReport,
    ) -> RepoGraphResult<()> {
        let now = (self.now_ms)();
        let metadata_row = sqlx::query("SELECT extractors::text AS extractors FROM graph_snapshots WHERE tenant=$1 AND snapshot_id=$2 FOR UPDATE").bind(&self.tenant).bind(id.0).fetch_optional(&mut *conn).await.map_err(map_db)?.ok_or(RepoGraphError::NotFound)?;
        let (names, _) = decode_metadata(&col::<String>(&metadata_row, "extractors")?)?;
        let metadata = encode_metadata(&names, report)?;
        let row = sqlx::query(sql::SNAPSHOT_LOCK)
            .bind(&self.tenant)
            .bind(id.0)
            .fetch_optional(&mut *conn)
            .await
            .map_err(map_db)?
            .ok_or(RepoGraphError::NotFound)?;
        let cur: String = col(&row, "status")?;
        if cur != "building" {
            return Err(RepoGraphError::Conflict(
                "snapshot not building".to_string(),
            ));
        }
        let built_at_ms: i64 = col(&row, "built_at_ms")?;
        let duration = report
            .elapsed_ms
            .max(now.saturating_sub(sql::unsigned(built_at_ms)));
        sqlx::query(sql::FINISH_UPDATE)
            .bind(&self.tenant)
            .bind(id.0)
            .bind(status.as_str())
            .bind(truncate_chars(reason, MAX_REASON_LEN))
            .bind(sql::ms(duration))
            .execute(&mut *conn)
            .await
            .map_err(map_db)?;
        sqlx::query(
            "UPDATE graph_snapshots SET extractors=$3::jsonb WHERE tenant=$1 AND snapshot_id=$2",
        )
        .bind(&self.tenant)
        .bind(id.0)
        .bind(metadata)
        .execute(&mut *conn)
        .await
        .map_err(map_db)?;
        Ok(())
    }
}

pub(super) struct PgReadView {
    pub(super) tenant: String,
    pub(super) scope: Scope,
    pub(super) tx: tokio::sync::Mutex<sqlx::Transaction<'static, Postgres>>,
}
#[async_trait]
impl RepoGraphReadView for PgReadView {
    async fn report(&self) -> RepoGraphResult<Option<ExtractReport>> {
        let mut tx = self.tx.lock().await;
        let row = sqlx::query(sql::SNAPSHOT_GET)
            .bind(&self.tenant)
            .bind(self.scope.repo.0)
            .bind(self.scope.snapshot.0)
            .fetch_one(&mut **tx)
            .await
            .map_err(map_db)?;
        Ok(decode_metadata(&col::<String>(&row, "extractors")?)?.1)
    }

    async fn nodes_by_key(&self, keys: &[NodeKey]) -> RepoGraphResult<Vec<NodeRow>> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;

        let keys: Vec<String> = keys
            .iter()
            .take(MAX_KEYS)
            .map(|k| k.as_str().to_string())
            .collect();
        sqlx::query(sql::NODES_BY_KEY)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&keys)
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_noderow)
            .collect()
    }

    async fn nodes_by_file(&self, files: &[String]) -> RepoGraphResult<Vec<NodeRow>> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;

        let files: Vec<String> = files.iter().take(MAX_KEYS).cloned().collect();
        sqlx::query(sql::NODES_BY_FILE)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&files)
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_noderow)
            .collect()
    }

    async fn nodes_by_name(
        &self,
        name: &str,
        kind: Option<NodeKind>,
        limit: usize,
    ) -> RepoGraphResult<Vec<NodeRow>> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;

        // A name that cannot be a symbol returns empty, not an error, and never reaches SQL.
        if name.len() > 256 || name.bytes().any(|b| b.is_ascii_whitespace()) {
            return Ok(Vec::new());
        }
        let limit = sql::clamp_limit(limit);
        sqlx::query(sql::NODES_BY_NAME)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(name)
            .bind(kind.map(NodeKind::as_str))
            .bind(lim(limit))
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
            .iter()
            .map(row_to_noderow)
            .collect()
    }

    async fn neighbors(
        &self,
        seeds: &[NodeId],
        kind: EdgeKind,
        dir: Direction,
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<Neighbor>> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;

        let hops = sql::clamp_hops(hops, sql::NEIGHBOR_HOPS);
        let cap = sql::clamp_cap(cap);
        let seeds: Vec<i64> = seeds.iter().map(|n| n.0).collect();
        let stmt = match dir {
            Direction::In => sql::NEIGHBORS_IN,
            Direction::Out => sql::NEIGHBORS_OUT,
        };
        sqlx::query(stmt)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&seeds)
            .bind(kind.as_str())
            .bind(hops as i32)
            .bind(lim(cap))
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| {
                let depth: i32 = col(row, "depth")?;
                Ok(Neighbor {
                    row: row_to_noderow(row)?,
                    depth: u8::try_from(depth).unwrap_or(u8::MAX),
                })
            })
            .collect()
    }

    async fn blast_radius(
        &self,
        files: &[String],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<String>> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;

        let hops = sql::clamp_hops(hops, sql::RADIUS_HOPS);
        let cap = sql::clamp_cap(cap);
        let files: Vec<String> = files.iter().take(MAX_KEYS).cloned().collect();
        sqlx::query(sql::BLAST_RADIUS)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&files)
            .bind(hops as i32)
            .bind(lim(cap))
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| col::<String>(row, "file"))
            .collect()
    }

    async fn tests_covering(
        &self,
        seeds: &[NodeId],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<TestHit>> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;

        let hops = sql::clamp_hops(hops, sql::RADIUS_HOPS);
        let cap = sql::clamp_cap(cap);
        let seeds: Vec<i64> = seeds.iter().map(|n| n.0).collect();
        let mut hits: Vec<TestHit> = sqlx::query(sql::TESTS_COVERING)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(&seeds)
            .bind(hops as i32)
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| {
                let via: Option<String> = col(row, "via")?;
                Ok(TestHit {
                    row: row_to_noderow(row)?,
                    via: via.unwrap_or_default(),
                })
            })
            .collect::<RepoGraphResult<Vec<_>>>()?;
        hits.sort_by(|a, b| a.row.node.key.cmp(&b.row.node.key));
        hits.truncate(cap);
        Ok(hits)
    }

    async fn path_between(
        &self,
        src: NodeId,
        dst: NodeId,
        max_hops: u32,
        max_paths: usize,
    ) -> RepoGraphResult<Vec<Vec<NodeKey>>> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;

        let max_hops = sql::clamp_hops(max_hops, sql::PATH_HOPS);
        let max_paths = max_paths.clamp(1, MAX_PATHS);
        let id_paths: Vec<Vec<i64>> = sqlx::query(sql::PATH_BETWEEN)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .bind(src.0)
            .bind(dst.0)
            .bind(max_hops as i32)
            .bind(lim(max_paths))
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
            .iter()
            .map(|row| col::<Vec<i64>>(row, "path"))
            .collect::<RepoGraphResult<Vec<_>>>()?;

        // Map the id paths back to keys. An id whose key does not resolve drops its path.
        let ids: Vec<i64> = id_paths
            .iter()
            .flatten()
            .copied()
            .collect::<BTreeSet<i64>>()
            .into_iter()
            .collect();
        let mut by_id: HashMap<i64, NodeKey> = HashMap::new();
        for row in &sqlx::query(sql::KEYS_FOR_IDS)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(&ids)
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
        {
            let id: i64 = col(row, "node_id")?;
            let key_text: String = col(row, "node_key")?;
            if let Ok(key) = NodeKey::parse(&key_text) {
                by_id.insert(id, key);
            }
        }
        let mut out: Vec<Vec<NodeKey>> = Vec::new();
        for path in id_paths {
            let mut keys = Vec::with_capacity(path.len());
            let mut ok = true;
            for id in path {
                match by_id.get(&id) {
                    Some(key) => keys.push(key.clone()),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                out.push(keys);
            }
        }
        Ok(out)
    }

    async fn shape(&self) -> RepoGraphResult<Shape> {
        let scope = self.scope;
        let mut tx = self.tx.lock().await;
        let meta = sqlx::query(sql::SNAPSHOT_GET)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_db)?
            .ok_or(RepoGraphError::NotFound)?;
        let snapshot = row_to_snapshot(&meta)?;

        let mut nodes_by_kind: BTreeMap<NodeKind, usize> = BTreeMap::new();
        for row in &sqlx::query(sql::SHAPE_NODES_BY_KIND)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
        {
            let kind_text: String = col(row, "kind")?;
            let kind =
                NodeKind::parse(&kind_text).ok_or_else(|| backend("column kind: unknown value"))?;
            let c: i64 = col(row, "c")?;
            nodes_by_kind.insert(kind, usize::try_from(c).unwrap_or(0));
        }
        let mut edges_by_kind: BTreeMap<EdgeKind, usize> = BTreeMap::new();
        for row in &sqlx::query(sql::SHAPE_EDGES_BY_KIND)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_all(&mut **tx)
            .await
            .map_err(map_db)?
        {
            let kind_text: String = col(row, "kind")?;
            let kind =
                EdgeKind::parse(&kind_text).ok_or_else(|| backend("column kind: unknown value"))?;
            let c: i64 = col(row, "c")?;
            edges_by_kind.insert(kind, usize::try_from(c).unwrap_or(0));
        }
        let files: i64 = sqlx::query_scalar(sql::SHAPE_FILES)
            .bind(&self.tenant)
            .bind(scope.repo.0)
            .bind(scope.snapshot.0)
            .fetch_one(&mut **tx)
            .await
            .map_err(map_db)?;
        let crates = nodes_by_kind.get(&NodeKind::Crate).copied().unwrap_or(0);
        Ok(Shape {
            snapshot,
            nodes_by_kind,
            edges_by_kind,
            files: usize::try_from(files).unwrap_or(0),
            crates,
        })
    }
}
