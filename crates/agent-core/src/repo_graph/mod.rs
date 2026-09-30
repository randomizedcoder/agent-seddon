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

pub mod key;
pub use key::*;

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
