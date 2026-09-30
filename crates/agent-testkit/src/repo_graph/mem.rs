//! [`MemRepoGraph`]: the in-memory `RepoGraphStore`. One `Mutex<MemState>` holds every tenant's
//! rows with global identities (like Postgres `IDENTITY` columns), so a foreign tenant's id is
//! simply absent under its `(tenant, …)` key → `NotFound`. A write is a **clone-mutate-swap**
//! transaction: the closure runs on a clone of the state and the clone replaces the original only
//! on `Ok`, so a snapshot commits all-or-nothing — the same contract `PgRepoGraph` gets from a
//! real transaction. Node bodies are shared per `(tenant, repo)`; per-commit facts and edges are
//! snapshot-scoped. Time is epoch milliseconds from an injectable clock.

use agent_core::repo_graph::{
    node_id_for, Direction, EdgeKind, EdgeRef, ExtractReport, GraphDiff, Neighbor, Node, NodeId,
    NodeKey, NodeKind, NodeRow, NodeVersion, Repo, RepoGraph, RepoGraphError, RepoGraphResult,
    RepoGraphStore, RepoId, RepoSpec, Scope, Shape, Snapshot, SnapshotBegin, SnapshotId,
    SnapshotStatus, TestHit, MAX_DIFF, MAX_KEYS, MAX_NEIGHBOR_HOPS, MAX_PATHS, MAX_PATH_HOPS,
    MAX_RADIUS_HOPS, MAX_REASON_LEN, MAX_RESULT, MAX_RETAIN, MAX_SNAPSHOT_EDGES, MAX_SNAPSHOT_LIST,
    MAX_SNAPSHOT_NODES,
};
use agent_core::safe_segment;
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

/// A snapshot's stored rows: its metadata, its per-node versions and its edges.
#[derive(Debug, Clone)]
struct SnapRec {
    meta: Snapshot,
    /// `node_id` → the node's version at this snapshot.
    versions: BTreeMap<i64, NodeVersion>,
    edges: Vec<agent_core::repo_graph::Edge>,
}

/// Every tenant's rows. Identities are global (one counter per table), as in Postgres.
#[derive(Debug, Clone, Default)]
struct MemState {
    next_repo_id: i64,
    next_snapshot_id: i64,
    /// `(tenant, slug)` → repo.
    repos: BTreeMap<(String, String), Repo>,
    /// `(tenant, snapshot_id)` → snapshot rows.
    snapshots: BTreeMap<(String, i64), SnapRec>,
    /// `(tenant, repo_id, node_id)` → the shared node body.
    bodies: BTreeMap<(String, i64, i64), Node>,
}

/// The in-memory repo-graph store. `Clone` shares the state (a second handle onto the same
/// backend); [`MemRepoGraph::with_tenant`] shares it under another tenant.
#[derive(Clone)]
pub struct MemRepoGraph {
    inner: Arc<Mutex<MemState>>,
    tenant: String,
    now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for MemRepoGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemRepoGraph")
            .field("tenant", &self.tenant)
            .finish_non_exhaustive()
    }
}

impl Default for MemRepoGraph {
    fn default() -> Self {
        Self::new()
    }
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

impl MemRepoGraph {
    /// An empty store bound to the `local` tenant, on the wall clock.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MemState {
                next_repo_id: 1,
                next_snapshot_id: 1,
                ..MemState::default()
            })),
            tenant: "local".to_string(),
            now_ms: Arc::new(wall_clock_ms),
        }
    }

    /// The tenant this handle is bound to.
    pub fn tenant(&self) -> &str {
        &self.tenant
    }

    /// The same backend and clock under `tenant`; refuses anything that is not a
    /// [`safe_segment`] (traversal, empty, over-length) without touching the state.
    pub fn with_tenant(&self, tenant: &str) -> RepoGraphResult<Self> {
        if !safe_segment(tenant) {
            return Err(RepoGraphError::Invalid(
                "tenant: must be a non-empty path-safe segment".to_string(),
            ));
        }
        Ok(Self {
            inner: Arc::clone(&self.inner),
            tenant: tenant.to_string(),
            now_ms: Arc::clone(&self.now_ms),
        })
    }

    /// Replace the clock (epoch milliseconds). Tests drive `created_at` / `built_at` with it.
    #[doc(hidden)]
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }

    fn read<T>(&self, f: impl FnOnce(&MemState) -> RepoGraphResult<T>) -> RepoGraphResult<T> {
        let guard = self.inner.lock().expect("repo graph store poisoned");
        f(&guard)
    }

    /// A write as an all-or-nothing transaction: the closure mutates a clone of the state, which
    /// replaces the shared one only on `Ok`.
    fn write<T>(
        &self,
        f: impl FnOnce(&mut MemState, &str, u64) -> RepoGraphResult<T>,
    ) -> RepoGraphResult<T> {
        let mut guard = self.inner.lock().expect("repo graph store poisoned");
        let mut candidate = guard.clone();
        let now = (self.now_ms)();
        let out = f(&mut candidate, &self.tenant, now)?;
        *guard = candidate;
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// State helpers (free functions over a locked MemState + tenant)
// ---------------------------------------------------------------------------

fn find_repo_by_id(st: &MemState, tenant: &str, repo: RepoId) -> Option<Repo> {
    st.repos
        .iter()
        .find(|((t, _), r)| t == tenant && r.id == repo)
        .map(|(_, r)| r.clone())
}

fn snap_for_scope<'a>(
    st: &'a MemState,
    tenant: &str,
    scope: Scope,
) -> RepoGraphResult<&'a SnapRec> {
    let snap = st
        .snapshots
        .get(&(tenant.to_string(), scope.snapshot.0))
        .ok_or(RepoGraphError::NotFound)?;
    if snap.meta.repo != scope.repo {
        return Err(RepoGraphError::NotFound);
    }
    Ok(snap)
}

fn node_row(
    st: &MemState,
    tenant: &str,
    repo: RepoId,
    snap: &SnapRec,
    node_id: i64,
) -> Option<NodeRow> {
    let version = snap.versions.get(&node_id)?.clone();
    let node = st
        .bodies
        .get(&(tenant.to_string(), repo.0, node_id))?
        .clone();
    Some(NodeRow { node, version })
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s[..cut].to_string()
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

#[async_trait]
impl RepoGraphStore for MemRepoGraph {
    // -- Repos -------------------------------------------------------------

    async fn repo_put(&self, spec: &RepoSpec) -> RepoGraphResult<RepoId> {
        spec.validate()?;
        self.write(|st, tenant, now| {
            let key = (tenant.to_string(), spec.slug.clone());
            if let Some(existing) = st.repos.get_mut(&key) {
                existing.forge.clone_from(&spec.forge);
                existing.remote_url.clone_from(&spec.remote_url);
                existing.default_branch.clone_from(&spec.default_branch);
                existing.profile.clone_from(&spec.profile);
                return Ok(existing.id);
            }
            let id = RepoId(st.next_repo_id);
            st.next_repo_id += 1;
            st.repos.insert(
                key,
                Repo {
                    id,
                    slug: spec.slug.clone(),
                    forge: spec.forge.clone(),
                    remote_url: spec.remote_url.clone(),
                    default_branch: spec.default_branch.clone(),
                    profile: spec.profile.clone(),
                    created_at_ms: now,
                },
            );
            Ok(id)
        })
    }

    async fn repo_get(&self, slug: &str) -> RepoGraphResult<Option<Repo>> {
        self.read(|st| {
            Ok(st
                .repos
                .get(&(self.tenant.clone(), slug.to_string()))
                .cloned())
        })
    }

    async fn repos(&self) -> RepoGraphResult<Vec<Repo>> {
        self.read(|st| {
            Ok(st
                .repos
                .iter()
                .filter(|((t, _), _)| t == &self.tenant)
                .map(|(_, r)| r.clone())
                .collect())
        })
    }

    // -- Snapshots ---------------------------------------------------------

    async fn snapshot_begin(&self, begin: &SnapshotBegin) -> RepoGraphResult<SnapshotId> {
        begin.validate()?;
        self.write(|st, tenant, now| {
            if find_repo_by_id(st, tenant, begin.repo).is_none() {
                return Err(RepoGraphError::NotFound);
            }
            // The identity is (repo, sha, extractor_version). A live one conflicts; a failed one
            // is replaced.
            let mut replace: Option<i64> = None;
            for ((t, sid), snap) in &st.snapshots {
                if t != tenant
                    || snap.meta.repo != begin.repo
                    || snap.meta.commit_sha != begin.commit_sha
                    || snap.meta.extractor_version != begin.extractor_version
                {
                    continue;
                }
                match snap.meta.status {
                    SnapshotStatus::Building | SnapshotStatus::Ready => {
                        return Err(RepoGraphError::Conflict("snapshot identity".to_string()));
                    }
                    SnapshotStatus::Failed => replace = Some(*sid),
                }
            }
            if let Some(sid) = replace {
                st.snapshots.remove(&(tenant.to_string(), sid));
            }
            let id = SnapshotId(st.next_snapshot_id);
            st.next_snapshot_id += 1;
            st.snapshots.insert(
                (tenant.to_string(), id.0),
                SnapRec {
                    meta: Snapshot {
                        id,
                        repo: begin.repo,
                        commit_sha: begin.commit_sha.clone(),
                        extractors: begin.extractors.clone(),
                        extractor_version: begin.extractor_version.clone(),
                        graph_hash: String::new(),
                        node_count: 0,
                        edge_count: 0,
                        status: SnapshotStatus::Building,
                        reason: String::new(),
                        built_at_ms: now,
                        duration_ms: 0,
                    },
                    versions: BTreeMap::new(),
                    edges: Vec::new(),
                },
            );
            Ok(id)
        })
    }

    async fn snapshot_write(&self, id: SnapshotId, graph: &RepoGraph) -> RepoGraphResult<()> {
        if graph.nodes().len() > MAX_SNAPSHOT_NODES {
            return Err(RepoGraphError::TooLong("nodes".to_string()));
        }
        if graph.edges().len() > MAX_SNAPSHOT_EDGES {
            return Err(RepoGraphError::TooLong("edges".to_string()));
        }
        self.write(|st, tenant, _now| {
            let repo = {
                let snap = st
                    .snapshots
                    .get(&(tenant.to_string(), id.0))
                    .ok_or(RepoGraphError::NotFound)?;
                if snap.meta.status != SnapshotStatus::Building {
                    return Err(RepoGraphError::Conflict(
                        "snapshot not building".to_string(),
                    ));
                }
                snap.meta.repo
            };

            // Validate every body against the shared table first (all-or-nothing): an existing
            // body with the same id but a different key is a collision; nothing is written.
            for rec in graph.nodes() {
                let bkey = (tenant.to_string(), repo.0, rec.node.id.0);
                if let Some(existing) = st.bodies.get(&bkey) {
                    if existing.key != rec.node.key {
                        return Err(RepoGraphError::Conflict("node id collision".to_string()));
                    }
                }
            }

            // Commit: insert new bodies (reuse existing), set versions and edges.
            let mut versions = BTreeMap::new();
            for rec in graph.nodes() {
                let bkey = (tenant.to_string(), repo.0, rec.node.id.0);
                st.bodies.entry(bkey).or_insert_with(|| rec.node.clone());
                versions.insert(rec.node.id.0, rec.version.clone());
            }
            let snap = st
                .snapshots
                .get_mut(&(tenant.to_string(), id.0))
                .ok_or(RepoGraphError::NotFound)?;
            snap.versions = versions;
            snap.edges = graph.edges().to_vec();
            snap.meta.graph_hash = graph.graph_hash().to_string();
            snap.meta.node_count = graph.nodes().len();
            snap.meta.edge_count = graph.edges().len();
            Ok(())
        })
    }

    async fn snapshot_finish(
        &self,
        id: SnapshotId,
        status: SnapshotStatus,
        reason: &str,
        _report: &ExtractReport,
    ) -> RepoGraphResult<()> {
        self.write(|st, tenant, now| {
            let snap = st
                .snapshots
                .get_mut(&(tenant.to_string(), id.0))
                .ok_or(RepoGraphError::NotFound)?;
            if snap.meta.status != SnapshotStatus::Building {
                return Err(RepoGraphError::Conflict(
                    "snapshot not building".to_string(),
                ));
            }
            snap.meta.status = status;
            snap.meta.reason = truncate_chars(reason, MAX_REASON_LEN);
            snap.meta.duration_ms = now.saturating_sub(snap.meta.built_at_ms);
            Ok(())
        })
    }

    async fn snapshot_find(
        &self,
        repo: RepoId,
        commit_sha: &str,
    ) -> RepoGraphResult<Option<Snapshot>> {
        self.read(|st| {
            Ok(newest_ready(st, &self.tenant, repo, |s| {
                s.commit_sha == commit_sha
            }))
        })
    }

    async fn snapshot_latest(&self, repo: RepoId) -> RepoGraphResult<Option<Snapshot>> {
        self.read(|st| Ok(newest_ready(st, &self.tenant, repo, |_| true)))
    }

    async fn snapshots(&self, repo: RepoId, limit: usize) -> RepoGraphResult<Vec<Snapshot>> {
        let limit = limit.clamp(1, MAX_SNAPSHOT_LIST);
        self.read(|st| {
            let mut all: Vec<Snapshot> = st
                .snapshots
                .iter()
                .filter(|((t, _), s)| t == &self.tenant && s.meta.repo == repo)
                .map(|(_, s)| s.meta.clone())
                .collect();
            all.sort_by(|a, b| b.built_at_ms.cmp(&a.built_at_ms).then(b.id.0.cmp(&a.id.0)));
            all.truncate(limit);
            Ok(all)
        })
    }

    async fn snapshot_delete_older_than(
        &self,
        repo: RepoId,
        keep: usize,
    ) -> RepoGraphResult<usize> {
        let keep = keep.clamp(1, MAX_RETAIN);
        self.write(|st, tenant, _now| {
            // Ready snapshots of the repo, newest first.
            let mut ready: Vec<(i64, u64)> = st
                .snapshots
                .iter()
                .filter(|((t, _), s)| {
                    t == tenant && s.meta.repo == repo && s.meta.status == SnapshotStatus::Ready
                })
                .map(|((_, sid), s)| (*sid, s.meta.built_at_ms))
                .collect();
            ready.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
            let retained: HashSet<i64> = ready.iter().take(keep).map(|(sid, _)| *sid).collect();

            let to_delete: Vec<i64> = st
                .snapshots
                .iter()
                .filter(|((t, _), s)| {
                    if t != tenant || s.meta.repo != repo {
                        return false;
                    }
                    match s.meta.status {
                        SnapshotStatus::Ready => !retained.contains(&s.meta.id.0),
                        SnapshotStatus::Failed => true,
                        SnapshotStatus::Building => false,
                    }
                })
                .map(|((_, sid), _)| *sid)
                .collect();

            let deleted = to_delete.len();
            for sid in to_delete {
                st.snapshots.remove(&(tenant.to_string(), sid));
            }

            // Sweep bodies no surviving version of this repo references.
            let mut referenced: HashSet<i64> = HashSet::new();
            for ((t, _), s) in &st.snapshots {
                if t == tenant && s.meta.repo == repo {
                    referenced.extend(s.versions.keys().copied());
                }
            }
            st.bodies.retain(|(t, r, nid), _| {
                !(t == tenant && *r == repo.0 && !referenced.contains(nid))
            });
            Ok(deleted)
        })
    }

    async fn snapshot_diff(&self, a: SnapshotId, b: SnapshotId) -> RepoGraphResult<GraphDiff> {
        self.read(|st| {
            let sa = st
                .snapshots
                .get(&(self.tenant.clone(), a.0))
                .ok_or(RepoGraphError::NotFound)?;
            let sb = st
                .snapshots
                .get(&(self.tenant.clone(), b.0))
                .ok_or(RepoGraphError::NotFound)?;
            if sa.meta.repo != sb.meta.repo {
                return Err(RepoGraphError::NotFound);
            }
            let repo = sa.meta.repo;
            let ha = key_hashes(st, &self.tenant, repo, sa);
            let hb = key_hashes(st, &self.tenant, repo, sb);

            let mut diff = GraphDiff::default();
            for (key, (sig_b, body_b)) in &hb {
                match ha.get(key) {
                    None => diff.added.push(key.clone()),
                    Some((sig_a, body_a)) => {
                        if sig_a != sig_b {
                            diff.sig_changed.push(key.clone());
                        }
                        if body_a != body_b {
                            diff.body_changed.push(key.clone());
                        }
                    }
                }
            }
            for key in ha.keys() {
                if !hb.contains_key(key) {
                    diff.removed.push(key.clone());
                }
            }
            let ea = edge_refs(sa);
            let eb = edge_refs(sb);
            for e in &eb {
                if !ea.contains(e) {
                    diff.edges_added.push(e.clone());
                }
            }
            for e in &ea {
                if !eb.contains(e) {
                    diff.edges_removed.push(e.clone());
                }
            }
            // Sort and cap every list.
            diff.added.sort();
            diff.removed.sort();
            diff.sig_changed.sort();
            diff.body_changed.sort();
            sort_edge_refs(&mut diff.edges_added);
            sort_edge_refs(&mut diff.edges_removed);
            cap_vec(&mut diff.added, &mut diff.truncated);
            cap_vec(&mut diff.removed, &mut diff.truncated);
            cap_vec(&mut diff.sig_changed, &mut diff.truncated);
            cap_vec(&mut diff.body_changed, &mut diff.truncated);
            cap_vec(&mut diff.edges_added, &mut diff.truncated);
            cap_vec(&mut diff.edges_removed, &mut diff.truncated);
            Ok(diff)
        })
    }

    // -- Reads -------------------------------------------------------------

    async fn nodes_by_key(&self, scope: Scope, keys: &[NodeKey]) -> RepoGraphResult<Vec<NodeRow>> {
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            let mut out = Vec::new();
            for key in keys.iter().take(MAX_KEYS) {
                let id = node_id_for(key).0;
                if let Some(row) = node_row(st, &self.tenant, scope.repo, snap, id) {
                    if row.node.key == *key {
                        out.push(row);
                    }
                }
            }
            out.sort_by(|a, b| a.node.key.cmp(&b.node.key));
            Ok(out)
        })
    }

    async fn nodes_by_file(&self, scope: Scope, files: &[String]) -> RepoGraphResult<Vec<NodeRow>> {
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            let want: HashSet<&String> = files.iter().take(MAX_KEYS).collect();
            let mut out: Vec<NodeRow> = snap
                .versions
                .iter()
                .filter(|(_, v)| want.contains(&v.file))
                .filter_map(|(nid, _)| node_row(st, &self.tenant, scope.repo, snap, *nid))
                .collect();
            out.sort_by(|a, b| a.node.key.cmp(&b.node.key));
            Ok(out)
        })
    }

    async fn nodes_by_name(
        &self,
        scope: Scope,
        name: &str,
        kind: Option<NodeKind>,
        limit: usize,
    ) -> RepoGraphResult<Vec<NodeRow>> {
        let limit = limit.clamp(1, MAX_RESULT);
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            if name.len() > 256 || name.bytes().any(|b| b.is_ascii_whitespace()) {
                return Ok(Vec::new());
            }
            let mut out: Vec<NodeRow> = snap
                .versions
                .keys()
                .filter_map(|nid| node_row(st, &self.tenant, scope.repo, snap, *nid))
                .filter(|row| {
                    row.node.name == name && (kind.is_none() || Some(row.node.kind) == kind)
                })
                .collect();
            out.sort_by(|a, b| a.node.key.cmp(&b.node.key));
            out.truncate(limit);
            Ok(out)
        })
    }

    async fn neighbors(
        &self,
        scope: Scope,
        seeds: &[NodeId],
        kind: EdgeKind,
        dir: Direction,
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<Neighbor>> {
        let hops = hops.clamp(1, MAX_NEIGHBOR_HOPS);
        let cap = cap.clamp(1, MAX_RESULT);
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            let seed_set: HashSet<i64> = seeds.iter().map(|n| n.0).collect();
            let mut depth: BTreeMap<i64, u8> = BTreeMap::new();
            let mut frontier: Vec<i64> = seed_set.iter().copied().collect();
            for d in 1..=hops {
                let mut next = Vec::new();
                for &cur in &frontier {
                    for e in &snap.edges {
                        if e.kind != kind {
                            continue;
                        }
                        let nb = match dir {
                            Direction::Out if e.src_id.0 == cur => e.dst_id.0,
                            Direction::In if e.dst_id.0 == cur => e.src_id.0,
                            _ => continue,
                        };
                        if seed_set.contains(&nb) || depth.contains_key(&nb) {
                            continue;
                        }
                        depth.insert(nb, d as u8);
                        next.push(nb);
                    }
                }
                if next.is_empty() {
                    break;
                }
                frontier = next;
            }
            let mut hits: Vec<Neighbor> = depth
                .iter()
                .filter_map(|(&nid, &d)| {
                    node_row(st, &self.tenant, scope.repo, snap, nid)
                        .map(|row| Neighbor { row, depth: d })
                })
                .collect();
            hits.sort_by(|a, b| {
                a.depth
                    .cmp(&b.depth)
                    .then(a.row.node.key.cmp(&b.row.node.key))
            });
            hits.truncate(cap);
            Ok(hits)
        })
    }

    async fn blast_radius(
        &self,
        scope: Scope,
        files: &[String],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<String>> {
        let hops = hops.clamp(1, MAX_RADIUS_HOPS);
        let cap = cap.clamp(1, MAX_RESULT);
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            let want: HashSet<&String> = files.iter().take(MAX_KEYS).collect();
            let seeds: Vec<i64> = snap
                .versions
                .iter()
                .filter(|(_, v)| want.contains(&v.file))
                .map(|(nid, _)| *nid)
                .collect();
            let kinds = [EdgeKind::Calls, EdgeKind::Imports, EdgeKind::Implements];
            let visited = walk(snap, &seeds, &kinds, Direction::In, hops);
            let mut out: BTreeSet<String> = BTreeSet::new();
            for nid in visited {
                if let Some(v) = snap.versions.get(&nid) {
                    if !v.file.is_empty() {
                        out.insert(v.file.clone());
                    }
                }
            }
            Ok(out.into_iter().take(cap).collect())
        })
    }

    async fn tests_covering(
        &self,
        scope: Scope,
        seeds: &[NodeId],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<TestHit>> {
        let hops = hops.clamp(1, MAX_RADIUS_HOPS);
        let cap = cap.clamp(1, MAX_RESULT);
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            let seed_ids: Vec<i64> = seeds.iter().map(|n| n.0).collect();
            let mut reachable = walk(snap, &seed_ids, &[EdgeKind::Calls], Direction::In, hops);
            reachable.extend(seed_ids.iter().copied());
            let mut seen: HashSet<i64> = HashSet::new();
            let mut hits: Vec<TestHit> = Vec::new();
            for e in &snap.edges {
                if e.kind != EdgeKind::Tests || !reachable.contains(&e.dst_id.0) {
                    continue;
                }
                if !seen.insert(e.src_id.0) {
                    continue;
                }
                if let Some(row) = node_row(st, &self.tenant, scope.repo, snap, e.src_id.0) {
                    let via = e
                        .attrs
                        .get("via")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    hits.push(TestHit { row, via });
                }
            }
            hits.sort_by(|a, b| a.row.node.key.cmp(&b.row.node.key));
            hits.truncate(cap);
            Ok(hits)
        })
    }

    async fn path_between(
        &self,
        scope: Scope,
        src: NodeId,
        dst: NodeId,
        max_hops: u32,
        max_paths: usize,
    ) -> RepoGraphResult<Vec<Vec<NodeKey>>> {
        let max_hops = max_hops.clamp(1, MAX_PATH_HOPS);
        let max_paths = max_paths.clamp(1, MAX_PATHS);
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            let kinds = [
                EdgeKind::Calls,
                EdgeKind::Imports,
                EdgeKind::DependsOn,
                EdgeKind::Contains,
            ];
            let id_paths = find_paths(snap, src.0, dst.0, &kinds, max_hops, max_paths);
            let mut out: Vec<Vec<NodeKey>> = Vec::new();
            for path in id_paths {
                let mut keys = Vec::with_capacity(path.len());
                let mut ok = true;
                for nid in path {
                    match st.bodies.get(&(self.tenant.clone(), scope.repo.0, nid)) {
                        Some(node) => keys.push(node.key.clone()),
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
        })
    }

    async fn shape(&self, scope: Scope) -> RepoGraphResult<Shape> {
        self.read(|st| {
            let snap = snap_for_scope(st, &self.tenant, scope)?;
            let mut nodes_by_kind: BTreeMap<NodeKind, usize> = BTreeMap::new();
            let mut files: BTreeSet<String> = BTreeSet::new();
            let mut crates = 0usize;
            for (nid, v) in &snap.versions {
                if let Some(node) = st.bodies.get(&(self.tenant.clone(), scope.repo.0, *nid)) {
                    *nodes_by_kind.entry(node.kind).or_default() += 1;
                    if node.kind == NodeKind::Crate {
                        crates += 1;
                    }
                }
                if !v.file.is_empty() {
                    files.insert(v.file.clone());
                }
            }
            let mut edges_by_kind: BTreeMap<EdgeKind, usize> = BTreeMap::new();
            for e in &snap.edges {
                *edges_by_kind.entry(e.kind).or_default() += 1;
            }
            Ok(Shape {
                snapshot: snap.meta.clone(),
                nodes_by_kind,
                edges_by_kind,
                files: files.len(),
                crates,
            })
        })
    }
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

fn newest_ready(
    st: &MemState,
    tenant: &str,
    repo: RepoId,
    pred: impl Fn(&Snapshot) -> bool,
) -> Option<Snapshot> {
    st.snapshots
        .iter()
        .filter(|((t, _), s)| {
            t == tenant
                && s.meta.repo == repo
                && s.meta.status == SnapshotStatus::Ready
                && pred(&s.meta)
        })
        .map(|(_, s)| s.meta.clone())
        .max_by(|a, b| a.built_at_ms.cmp(&b.built_at_ms).then(a.id.0.cmp(&b.id.0)))
}

fn key_hashes(
    st: &MemState,
    tenant: &str,
    repo: RepoId,
    snap: &SnapRec,
) -> BTreeMap<NodeKey, (String, String)> {
    snap.versions
        .iter()
        .filter_map(|(nid, v)| {
            let node = st.bodies.get(&(tenant.to_string(), repo.0, *nid))?;
            Some((node.key.clone(), (v.sig_hash.clone(), v.body_hash.clone())))
        })
        .collect()
}

fn edge_refs(snap: &SnapRec) -> BTreeSet<EdgeRef> {
    snap.edges
        .iter()
        .map(|e| EdgeRef {
            kind: e.kind,
            src: e.src.clone(),
            dst: e.dst.clone(),
        })
        .collect()
}

fn sort_edge_refs(v: &mut [EdgeRef]) {
    v.sort_by(|a, b| {
        a.kind
            .as_str()
            .cmp(b.kind.as_str())
            .then(a.src.cmp(&b.src))
            .then(a.dst.cmp(&b.dst))
    });
}

fn cap_vec<T>(v: &mut Vec<T>, truncated: &mut bool) {
    if v.len() > MAX_DIFF {
        v.truncate(MAX_DIFF);
        *truncated = true;
    }
}

/// BFS over the given edge kinds in `dir`, up to `hops`; returns the reached ids (excluding the
/// seeds themselves).
fn walk(
    snap: &SnapRec,
    seeds: &[i64],
    kinds: &[EdgeKind],
    dir: Direction,
    hops: u32,
) -> HashSet<i64> {
    let seed_set: HashSet<i64> = seeds.iter().copied().collect();
    let mut visited: HashSet<i64> = HashSet::new();
    let mut frontier: Vec<i64> = seed_set.iter().copied().collect();
    for _ in 0..hops {
        let mut next = Vec::new();
        for &cur in &frontier {
            for e in &snap.edges {
                if !kinds.contains(&e.kind) {
                    continue;
                }
                let nb = match dir {
                    Direction::Out if e.src_id.0 == cur => e.dst_id.0,
                    Direction::In if e.dst_id.0 == cur => e.src_id.0,
                    _ => continue,
                };
                if seed_set.contains(&nb) || !visited.insert(nb) {
                    continue;
                }
                next.push(nb);
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    visited
}

/// Enumerate up to `max_paths` simple paths from `src` to `dst` over the given edge kinds
/// (outbound), shortest first, each using at most `max_hops` edges. The visited-set per path is
/// the cycle guard.
fn find_paths(
    snap: &SnapRec,
    src: i64,
    dst: i64,
    kinds: &[EdgeKind],
    max_hops: u32,
    max_paths: usize,
) -> Vec<Vec<i64>> {
    let mut results: Vec<Vec<i64>> = Vec::new();
    let mut queue: VecDeque<Vec<i64>> = VecDeque::new();
    queue.push_back(vec![src]);
    while let Some(path) = queue.pop_front() {
        if results.len() >= max_paths {
            break;
        }
        let last = *path.last().expect("non-empty path");
        if last == dst && path.len() >= 2 {
            results.push(path);
            continue;
        }
        if (path.len() as u32 - 1) >= max_hops {
            continue;
        }
        let mut nbrs: Vec<i64> = snap
            .edges
            .iter()
            .filter(|e| kinds.contains(&e.kind) && e.src_id.0 == last)
            .map(|e| e.dst_id.0)
            .collect();
        nbrs.sort_unstable();
        nbrs.dedup();
        for nb in nbrs {
            if path.contains(&nb) {
                continue;
            }
            let mut np = path.clone();
            np.push(nb);
            queue.push_back(np);
        }
    }
    results.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    results
}
