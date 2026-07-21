//! `agent-session` — concrete [`SessionStore`] backends behind the seam in
//! `agent-core` (parity spec 19).
//!
//! [`FileSessionStore`] is a dependency-free, content-addressed history: each
//! checkpoint is an immutable JSON object under `objects/<id>.json` (id = a hash of
//! its messages + parent + label), and each session's branch heads live in
//! `sessions/<session>.json`. `undo`/`branch`/`fork` move heads without rewriting
//! objects; the object store is shared across sessions, so identical content dedups
//! across turns and branches. A `RepoBackend`-backed impl (dedup via real git
//! objects) drops in behind the same trait as a follow-up. See
//! `docs/components/session.md`.

#[cfg(feature = "session-file")]
mod file;
#[cfg(feature = "session-file")]
pub use file::{content_id, FileSessionStore};
