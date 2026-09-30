//! Repo-graph doubles (`docs/design/repo-knowledge/`): [`MemRepoGraph`], an in-memory
//! `RepoGraphStore` with the same all-or-nothing write, shared-body and clamp semantics as the
//! Postgres tier, and the conformance suite ([`conformance`]) that every tier — this double and
//! `PgRepoGraph` (RK-02) — runs through the
//! [`repo_graph_conformance_suite!`](crate::repo_graph_conformance_suite) macro.

pub mod conformance;
mod mem;
pub use mem::MemRepoGraph;

#[cfg(test)]
mod tests;
