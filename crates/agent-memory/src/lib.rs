//! Memory implementations behind the `MemoryStore` seam (see DESIGN.md §3).
//! Each backend is gated by a cargo feature; the registry in `agent-runtime`
//! selects one by the `[memory] backend` config string. See `docs/extending.md`.

#[cfg(feature = "memory-file")]
mod file;
#[cfg(feature = "memory-file")]
pub use file::{file_memory, FileEpisodic, FileSemantic};
