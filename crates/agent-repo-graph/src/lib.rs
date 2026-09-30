//! `agent-repo-graph` — repo knowledge: a persisted, deterministic code graph per
//! `(tenant, repo, commit)`.
//!
//! The seam (`RepoGraphStore`), the node-key grammar, the value types and the pure
//! `GraphBuilder` live in [`agent_core::repo_graph`] so that the in-memory double in
//! `agent-testkit` and the Postgres tier here share one source of truth; this crate
//! re-exports them and, increment by increment, adds what only a consumer needs:
//!
//! * `postgres` (feature `repo-graph-postgres`, RK-02) — `PgRepoGraph`, the durable tier
//!   behind migration 0001, tenant-bound, all-or-nothing bulk write with the id-collision
//!   check, every read verb.
//! * extractors (RK-03) — `rust-syn`, `cargo`, `docs`: deterministic, versioned, budgeted
//!   parsers that feed the `GraphBuilder`.
//! * `Indexer` (RK-06) — checkout at a sha, run the extractors, write a snapshot, sweep old
//!   ones, emit the index metrics.
//!
//! Until those land it is a thin re-export so every commit in the workspace builds.
//!
//! Design: `docs/design/repo-knowledge/` (README decisions, `01-schema.md`,
//! `02-extraction.md`, `03-queries.md`, `06-increments.md`).

pub use agent_core::repo_graph;
pub use agent_core::repo_graph::*;
