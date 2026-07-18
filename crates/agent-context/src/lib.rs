//! Context assembly / compaction implementations behind the `ContextStrategy`
//! seam. Each strategy is gated by a cargo feature; the registry in
//! `agent-runtime` selects one by config string. See `docs/extending.md`.

#[cfg(feature = "context-sliding-window")]
mod sliding_window;
#[cfg(feature = "context-sliding-window")]
pub use sliding_window::SlidingWindow;
