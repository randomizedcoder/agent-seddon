//! `agent-tasks` — concrete [`TaskTracker`] backends behind the seam in
//! `agent-core` (parity spec 21).
//!
//! The default backend, [`MemoryTaskTracker`], holds the plan in a `Mutex<Vec<
//! Todo>>` — the seam is the contract; the backing store is swappable. A
//! `SessionStore`-backed backend (so a plan survives compaction / checkpoint /
//! fork, parity spec 19) drops in behind the same trait as a follow-up. See
//! `docs/components/tasks.md`.

#[cfg(feature = "tasks-memory")]
mod memory;
#[cfg(feature = "tasks-memory")]
pub use memory::MemoryTaskTracker;
