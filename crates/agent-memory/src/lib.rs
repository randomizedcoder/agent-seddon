//! Memory implementations behind the `MemoryStore` seam (see DESIGN.md §3).
//! Each backend is gated by a cargo feature; the registry in `agent-runtime`
//! selects one by the `[memory] backend` config string. See `docs/extending.md`.

#[cfg(feature = "memory-file")]
mod file;
#[cfg(feature = "memory-file")]
pub use file::{file_memory, FileEpisodic, FileSemantic};

// Per-step dimensional histories (adaptive-cognition 03).
#[cfg(feature = "memory-dimensions")]
mod dimensions;
#[cfg(feature = "memory-dimensions")]
pub use dimensions::{bench_parse_step, file_dimensions, FileDimensions};

// Per-user tenant routing over the file-backed stores
// (docs/design/multi-session/04-tenancy.md).
#[cfg(any(feature = "memory-file", feature = "memory-dimensions"))]
mod tenant;
#[cfg(feature = "memory-dimensions")]
pub use tenant::PerUserDimensions;
#[cfg(feature = "memory-file")]
pub use tenant::PerUserMemory;
