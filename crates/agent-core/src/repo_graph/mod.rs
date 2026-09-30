//! Repo knowledge — the `RepoGraphStore` seam and its pure supporting code.
//!
//! A persisted, deterministic code graph per `(tenant, repo, commit)`
//! (`docs/design/repo-knowledge/`). This module owns everything that has no backend: the
//! node-key grammar ([`key`]), the value types and the pure graph builder (`model`,
//! `builder`), and the `RepoGraphStore` trait itself. The concrete stores (`PgRepoGraph` in
//! RK-02) live in `agent-repo-graph`; the in-memory double `MemRepoGraph` and the conformance
//! suite live in `agent-testkit`.
//!
//! **The graph is built from untrusted input.** Repo content, model-supplied node keys and
//! tenant strings are all attacker-controlled, so every boundary here caps, clamps and fails
//! closed, and a rejection never echoes the offending bytes (`README.md` threat model). The
//! builder lives here — not in the impl crate — because the testkit conformance fixtures build
//! graphs through it and `agent-testkit` cannot depend on the impl crate (the CP-01 campaign
//! precedent; recorded as a deviation from the 06-increments RK-01 row).

use serde::{Deserialize, Serialize};

pub mod builder;
pub mod key;
pub mod model;
pub use builder::*;
pub use key::*;
pub use model::*;

/// The typed error of the seam (`03-queries.md`, README threat model). The caller's response
/// differs per variant, so they are variants, not messages; a payload names a field or a rule
/// and **never** echoes a foreign row's data or a rejected input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoGraphError {
    /// No row for the `(tenant, …)` addressed — including every cross-tenant and cross-repo
    /// access, which are indistinguishable from a genuinely absent row on purpose.
    NotFound,
    /// A compare-and-swap failed: a snapshot in the wrong state, a duplicate identity, or a
    /// node-id collision on write. Names the rule, not the row.
    Conflict(String),
    /// Grammar, caps or request validation failed; names the field.
    Invalid(String),
    /// A field exceeds its cap; names the field.
    TooLong(String),
    /// The store itself failed (connection, unexpected row shape).
    Backend(String),
}

impl std::fmt::Display for RepoGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoGraphError::NotFound => f.write_str("not found"),
            RepoGraphError::Conflict(s) => write!(f, "conflict: {s}"),
            RepoGraphError::Invalid(s) => write!(f, "invalid: {s}"),
            RepoGraphError::TooLong(s) => write!(f, "too long: {s}"),
            RepoGraphError::Backend(s) => write!(f, "backend: {s}"),
        }
    }
}

impl std::error::Error for RepoGraphError {}

impl From<RepoGraphError> for crate::Error {
    fn from(e: RepoGraphError) -> Self {
        crate::Error::RepoGraph(e.to_string())
    }
}

/// The seam's result type.
pub type RepoGraphResult<T> = std::result::Result<T, RepoGraphError>;

macro_rules! id_newtype {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
        )]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_newtype!(
    /// A `repos.repo_id` (identity column, always positive).
    RepoId
);
id_newtype!(
    /// A `graph_snapshots.snapshot_id` (identity column, always positive).
    SnapshotId
);
id_newtype!(
    /// A `graph_nodes.node_id`: the first 8 bytes of `sha256(node_key)`, big-endian, as an
    /// `i64` (so it can be negative). Content-addressed, not an identity column — computed by
    /// [`node_id_for`], never assigned by a store, so a store can never mint a forged id.
    NodeId
);

// ---------------------------------------------------------------------------
// Caps (03-queries.md hop / cap limits, README threat model)
// ---------------------------------------------------------------------------

/// The max hops a `neighbors` walk clamps to.
pub const MAX_NEIGHBOR_HOPS: u32 = 4;
/// The max hops a `blast_radius` walk clamps to.
pub const MAX_RADIUS_HOPS: u32 = 3;
/// The max hops a `path_between` search clamps to.
pub const MAX_PATH_HOPS: u32 = 6;
/// The max number of paths `path_between` returns.
pub const MAX_PATHS: usize = 32;
/// The max rows any read returns.
pub const MAX_RESULT: usize = 500;
/// The max keys / files a batched read accepts.
pub const MAX_KEYS: usize = 64;
/// The max snapshots `snapshots` returns.
pub const MAX_SNAPSHOT_LIST: usize = 200;
/// The max entries a `snapshot_diff` list carries.
pub const MAX_DIFF: usize = 10_000;
/// The max nodes a snapshot may hold.
pub const MAX_SNAPSHOT_NODES: usize = 250_000;
/// The max edges a snapshot may hold.
pub const MAX_SNAPSHOT_EDGES: usize = 2_000_000;
/// The max retained snapshots `snapshot_delete_older_than` will keep.
pub const MAX_RETAIN: usize = 1_000;

// ---------------------------------------------------------------------------
// Scope, direction
// ---------------------------------------------------------------------------

/// The read address within a tenant: which repo and which snapshot. The tenant is the store
/// handle's own, never a value here, so a read cannot be aimed at another tenant (README D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub repo: RepoId,
    pub snapshot: SnapshotId,
}

impl Scope {
    /// Construct a scope.
    pub fn new(repo: RepoId, snapshot: SnapshotId) -> Self {
        Self { repo, snapshot }
    }
}

/// Which way an edge walk follows edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// From src to dst (out-edges).
    Out,
    /// From dst to src (in-edges).
    In,
}

// ---------------------------------------------------------------------------
// Repos
// ---------------------------------------------------------------------------

/// The maximum length of a `forge` / `default_branch`, in bytes.
pub const MAX_FORGE_LEN: usize = 64;
/// The maximum length of a `remote_url`, in bytes.
pub const MAX_REMOTE_URL_LEN: usize = 512;
/// The maximum serialized size of a repo `profile`, in bytes.
pub const MAX_PROFILE_BYTES: usize = 4096;

/// A request to create or update a repo (upsert by slug).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepoSpec {
    /// The repo slug (`owner__repo`); a [`crate::safe_segment`].
    pub slug: String,
    /// The forge name (`github`, …).
    pub forge: String,
    /// The clone URL.
    pub remote_url: String,
    /// The default branch (`main`).
    pub default_branch: String,
    /// The repo profile object (`seam_crate`, `tool_trait`, …); a JSON object.
    pub profile: serde_json::Value,
}

impl Default for RepoSpec {
    fn default() -> Self {
        Self {
            slug: String::new(),
            forge: String::new(),
            remote_url: String::new(),
            default_branch: "main".to_string(),
            profile: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

impl RepoSpec {
    /// Validate every field fail-closed; the error names the field, never the value.
    pub fn validate(&self) -> RepoGraphResult<()> {
        if !crate::safe_segment(&self.slug) {
            return Err(RepoGraphError::Invalid("slug".into()));
        }
        if !is_forge_token(&self.forge) {
            return Err(RepoGraphError::Invalid("forge".into()));
        }
        if !is_forge_token(&self.default_branch) {
            return Err(RepoGraphError::Invalid("default_branch".into()));
        }
        if self.remote_url.len() > MAX_REMOTE_URL_LEN {
            return Err(RepoGraphError::TooLong("remote_url".into()));
        }
        if self
            .remote_url
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        {
            return Err(RepoGraphError::Invalid("remote_url".into()));
        }
        match &self.profile {
            serde_json::Value::Object(m) => {
                if serde_json::to_vec(m).map(|v| v.len()).unwrap_or(usize::MAX) > MAX_PROFILE_BYTES {
                    return Err(RepoGraphError::TooLong("profile".into()));
                }
            }
            _ => return Err(RepoGraphError::Invalid("profile".into())),
        }
        Ok(())
    }
}

/// A repo row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repo {
    pub id: RepoId,
    pub slug: String,
    pub forge: String,
    pub remote_url: String,
    pub default_branch: String,
    pub profile: serde_json::Value,
    pub created_at_ms: u64,
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

/// The lifecycle state of a snapshot (`graph_snapshots.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotStatus {
    /// Being written; readable only for its own write / finish.
    Building,
    /// Complete and queryable.
    Ready,
    /// Abandoned; replaced by a later `snapshot_begin` of the same identity.
    Failed,
}

impl SnapshotStatus {
    /// The wire / SQL spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            SnapshotStatus::Building => "building",
            SnapshotStatus::Ready => "ready",
            SnapshotStatus::Failed => "failed",
        }
    }
}

/// The maximum length of a `commit_sha` (a full sha1 hex).
pub const COMMIT_SHA_LEN: usize = 40;
/// The maximum number of extractor names in a snapshot.
pub const MAX_EXTRACTORS: usize = 16;
/// The maximum length of a single extractor name, in bytes.
pub const MAX_EXTRACTOR_NAME_LEN: usize = 32;
/// The maximum length of `extractor_version`, in bytes.
pub const MAX_EXTRACTOR_VERSION_LEN: usize = 256;
/// The maximum length of a snapshot `reason`, in bytes.
pub const MAX_REASON_LEN: usize = 512;

/// A request to begin a snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotBegin {
    pub repo: RepoId,
    /// A full 40-char lowercase-hex commit sha.
    pub commit_sha: String,
    /// The extractor names as run, each `[a-z0-9-]`.
    pub extractors: Vec<String>,
    /// The joined `name@version` list ([`extractor_version`]); part of the snapshot identity.
    pub extractor_version: String,
}

impl SnapshotBegin {
    /// Validate every field fail-closed; the error names the field, never the value.
    pub fn validate(&self) -> RepoGraphResult<()> {
        if self.commit_sha.len() != COMMIT_SHA_LEN
            || !self
                .commit_sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(RepoGraphError::Invalid("commit_sha".into()));
        }
        if self.extractors.len() > MAX_EXTRACTORS {
            return Err(RepoGraphError::TooLong("extractors".into()));
        }
        for name in &self.extractors {
            let ok = !name.is_empty()
                && name.len() <= MAX_EXTRACTOR_NAME_LEN
                && name
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-');
            if !ok {
                return Err(RepoGraphError::Invalid("extractor_name".into()));
            }
        }
        if self.extractor_version.len() > MAX_EXTRACTOR_VERSION_LEN {
            return Err(RepoGraphError::TooLong("extractor_version".into()));
        }
        Ok(())
    }
}

/// A snapshot row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub repo: RepoId,
    pub commit_sha: String,
    pub extractors: Vec<String>,
    pub extractor_version: String,
    pub graph_hash: String,
    pub node_count: usize,
    pub edge_count: usize,
    pub status: SnapshotStatus,
    pub reason: String,
    pub built_at_ms: u64,
    pub duration_ms: u64,
}

/// A reference to an edge by kind and endpoint keys (used in [`GraphDiff`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub kind: EdgeKind,
    pub src: NodeKey,
    pub dst: NodeKey,
}

/// The difference between two snapshots of one repo. Every list is sorted and capped at
/// [`MAX_DIFF`]; `truncated` is set when a cap was hit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphDiff {
    pub added: Vec<NodeKey>,
    pub removed: Vec<NodeKey>,
    pub sig_changed: Vec<NodeKey>,
    pub body_changed: Vec<NodeKey>,
    pub edges_added: Vec<EdgeRef>,
    pub edges_removed: Vec<EdgeRef>,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// Read rows
// ---------------------------------------------------------------------------

/// A node and its version at a snapshot — the row every node read returns.
pub type NodeRow = NodeRecord;

/// A node reached by an edge walk, with its BFS depth from the seed set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Neighbor {
    pub row: NodeRow,
    pub depth: u8,
}

/// A test node that covers a seed, with the edge's `via` provenance (`"name"` / `"scip"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestHit {
    pub row: NodeRow,
    pub via: String,
}

/// A summary of a snapshot's shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub snapshot: Snapshot,
    pub nodes_by_kind: std::collections::BTreeMap<NodeKind, usize>,
    pub edges_by_kind: std::collections::BTreeMap<EdgeKind, usize>,
    pub files: usize,
    pub crates: usize,
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// The persisted repo-graph store (`03-queries.md`). Every read clamps its hops / caps / list
/// lengths and returns an empty result for an unknown or hostile key, never an error that echoes
/// it; a read whose snapshot is not under `scope.repo` (of this tenant) is [`RepoGraphError::NotFound`].
///
/// The store is **tenant-bound**: it is obtained through a `with_tenant` constructor on the
/// concrete type, so a caller cannot address another tenant through any argument here.
#[async_trait::async_trait]
pub trait RepoGraphStore: Send + Sync {
    // -- Repos -------------------------------------------------------------

    /// Upsert a repo by slug; returns its stable id.
    async fn repo_put(&self, spec: &RepoSpec) -> RepoGraphResult<RepoId>;
    /// Fetch a repo by slug.
    async fn repo_get(&self, slug: &str) -> RepoGraphResult<Option<Repo>>;
    /// All repos of this tenant, ordered by slug.
    async fn repos(&self) -> RepoGraphResult<Vec<Repo>>;

    // -- Snapshots ---------------------------------------------------------

    /// Begin a `building` snapshot; `Conflict` if a `building` / `ready` one with the same
    /// `(repo, sha, extractor_version)` exists (a `failed` one is replaced).
    async fn snapshot_begin(&self, begin: &SnapshotBegin) -> RepoGraphResult<SnapshotId>;
    /// Write the graph into a `building` snapshot (all-or-nothing; shared bodies).
    async fn snapshot_write(&self, id: SnapshotId, graph: &RepoGraph) -> RepoGraphResult<()>;
    /// Move a `building` snapshot to `Ready` / `Failed`, recording the report.
    async fn snapshot_finish(
        &self,
        id: SnapshotId,
        status: SnapshotStatus,
        reason: &str,
        report: &ExtractReport,
    ) -> RepoGraphResult<()>;
    /// The newest `ready` snapshot for a `(repo, sha)`.
    async fn snapshot_find(&self, repo: RepoId, commit_sha: &str)
        -> RepoGraphResult<Option<Snapshot>>;
    /// The newest `ready` snapshot for a repo.
    async fn snapshot_latest(&self, repo: RepoId) -> RepoGraphResult<Option<Snapshot>>;
    /// Snapshots for a repo, newest first, every status, `limit` clamped to `1..=MAX_SNAPSHOT_LIST`.
    async fn snapshots(&self, repo: RepoId, limit: usize) -> RepoGraphResult<Vec<Snapshot>>;
    /// Keep the newest `keep` `ready` snapshots, delete the rest and every `failed` one, and
    /// sweep bodies no retained version references. Returns the number deleted.
    async fn snapshot_delete_older_than(&self, repo: RepoId, keep: usize) -> RepoGraphResult<usize>;
    /// The difference between two snapshots of one repo of this tenant.
    async fn snapshot_diff(&self, a: SnapshotId, b: SnapshotId) -> RepoGraphResult<GraphDiff>;

    // -- Reads -------------------------------------------------------------

    /// Nodes by exact key (≤ [`MAX_KEYS`]); unknown keys are silently absent.
    async fn nodes_by_key(&self, scope: Scope, keys: &[NodeKey]) -> RepoGraphResult<Vec<NodeRow>>;
    /// Nodes defined in any of the given files (≤ [`MAX_KEYS`]).
    async fn nodes_by_file(&self, scope: Scope, files: &[String]) -> RepoGraphResult<Vec<NodeRow>>;
    /// Nodes with an exact `name` (optionally a kind), `limit` clamped to `1..=MAX_RESULT`.
    async fn nodes_by_name(
        &self,
        scope: Scope,
        name: &str,
        kind: Option<NodeKind>,
        limit: usize,
    ) -> RepoGraphResult<Vec<NodeRow>>;
    /// BFS over one edge kind from the seeds; `hops` clamped `1..=MAX_NEIGHBOR_HOPS`, `cap`
    /// `1..=MAX_RESULT`; rows carry their min depth, ordered by `(depth, key)`.
    async fn neighbors(
        &self,
        scope: Scope,
        seeds: &[NodeId],
        kind: EdgeKind,
        dir: Direction,
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<Neighbor>>;
    /// Files reachable inbound over `calls` / `imports` / `implements` from the seed files;
    /// `hops` clamped `1..=MAX_RADIUS_HOPS`; sorted.
    async fn blast_radius(
        &self,
        scope: Scope,
        files: &[String],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<String>>;
    /// Tests reaching the seeds inbound over `calls` (≤ 3 hops) then `tests` in-edges.
    async fn tests_covering(
        &self,
        scope: Scope,
        seeds: &[NodeId],
        hops: u32,
        cap: usize,
    ) -> RepoGraphResult<Vec<TestHit>>;
    /// Up to `max_paths` shortest paths from `src` to `dst` over
    /// `calls` / `imports` / `depends_on` / `contains`; `max_hops` clamped `1..=MAX_PATH_HOPS`.
    async fn path_between(
        &self,
        scope: Scope,
        src: NodeId,
        dst: NodeId,
        max_hops: u32,
        max_paths: usize,
    ) -> RepoGraphResult<Vec<Vec<NodeKey>>>;
    /// A summary of the snapshot's shape.
    async fn shape(&self, scope: Scope) -> RepoGraphResult<Shape>;
}

/// A `forge` / `default_branch` token: non-empty, ≤ 64 ASCII `[A-Za-z0-9._/-]`.
fn is_forge_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_FORGE_LEN
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn valid_repo() -> RepoSpec {
        RepoSpec {
            slug: "owner__repo".into(),
            forge: "github".into(),
            remote_url: "https://example.com/o/r.git".into(),
            default_branch: "main".into(),
            profile: serde_json::json!({ "seam_crate": "agent-core" }),
        }
    }

    #[test]
    fn positive_repo_spec_valid() {
        assert!(valid_repo().validate().is_ok());
    }

    #[rstest]
    #[case::slug_traversal("../x")]
    #[case::slug_space("a b")]
    #[case::slug_empty("")]
    #[case::slug_leading_dash("-x")]
    fn adversarial_repo_slug(#[case] slug: &str) {
        let mut spec = valid_repo();
        spec.slug = slug.into();
        assert_eq!(spec.validate(), Err(RepoGraphError::Invalid("slug".into())));
    }

    #[test]
    fn boundary_repo_slug_129() {
        let mut spec = valid_repo();
        spec.slug = "a".repeat(129);
        assert!(spec.validate().is_err());
    }

    #[test]
    fn adversarial_repo_remote_control_char() {
        let mut spec = valid_repo();
        spec.remote_url = "https://x\u{0001}.git".into();
        assert_eq!(
            spec.validate(),
            Err(RepoGraphError::Invalid("remote_url".into()))
        );
    }

    #[test]
    fn boundary_repo_remote_513() {
        let mut spec = valid_repo();
        spec.remote_url = "h".repeat(513);
        assert_eq!(
            spec.validate(),
            Err(RepoGraphError::TooLong("remote_url".into()))
        );
    }

    #[test]
    fn adversarial_repo_profile_not_object() {
        let mut spec = valid_repo();
        spec.profile = serde_json::json!("nope");
        assert_eq!(
            spec.validate(),
            Err(RepoGraphError::Invalid("profile".into()))
        );
    }

    #[test]
    fn adversarial_repo_profile_huge() {
        let mut spec = valid_repo();
        spec.profile = serde_json::json!({ "k": "a".repeat(MAX_PROFILE_BYTES) });
        assert_eq!(
            spec.validate(),
            Err(RepoGraphError::TooLong("profile".into()))
        );
    }

    fn valid_begin() -> SnapshotBegin {
        SnapshotBegin {
            repo: RepoId(1),
            commit_sha: "0".repeat(40),
            extractors: vec!["rust-syn".into(), "cargo".into()],
            extractor_version: "rust-syn@1,cargo@1".into(),
        }
    }

    #[test]
    fn positive_begin_valid() {
        assert!(valid_begin().validate().is_ok());
    }

    #[rstest]
    #[case::short("0".repeat(39))]
    #[case::long("0".repeat(41))]
    #[case::uppercase("A".repeat(40))]
    #[case::non_hex("g".repeat(40))]
    fn negative_begin_bad_sha(#[case] sha: String) {
        let mut begin = valid_begin();
        begin.commit_sha = sha;
        assert_eq!(
            begin.validate(),
            Err(RepoGraphError::Invalid("commit_sha".into()))
        );
    }

    #[rstest]
    #[case::traversal("../x")]
    #[case::uppercase("Rust")]
    #[case::too_long("a".repeat(33))]
    fn adversarial_begin_extractor_name(#[case] name: String) {
        let mut begin = valid_begin();
        begin.extractors = vec![name];
        assert!(begin.validate().is_err());
    }

    #[test]
    fn boundary_begin_extractors_17() {
        let mut begin = valid_begin();
        begin.extractors = (0..17).map(|i| format!("e{i}")).collect();
        assert_eq!(
            begin.validate(),
            Err(RepoGraphError::TooLong("extractors".into()))
        );
    }
}
