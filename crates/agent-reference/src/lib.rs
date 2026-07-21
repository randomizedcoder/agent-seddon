//! `agent-reference` — the `ReferenceResolver` seam impl (parity spec 17).
//!
//! [`LocalResolver`] parses `@file`/`@dir`/`@symbol`/`@url` mentions ([`parse`])
//! and routes each through the seam the runtime wired in: `@file`/`@dir` read the
//! workspace filesystem (confined + sensitive-path-guarded), `@symbol` queries the
//! `SearchBackend`, `@url` fetches through the `WebBackend` (reusing its SSRF
//! guard). Every fetched block is injection-scanned
//! ([`agent_core::scan_for_injection`]), deduped, and size-budgeted; an
//! unresolved/denied/failed reference degrades to a warning, never a hard error.
//! See `docs/components/reference.md`.

pub mod parse;
pub use parse::{bench_parse, parse};

#[cfg(feature = "reference-local")]
mod resolver;
#[cfg(feature = "reference-local")]
pub use resolver::LocalResolver;
