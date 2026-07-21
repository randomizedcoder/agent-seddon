//! `agent-lsp` — a live Language Server Protocol backend behind the `LspBackend`
//! seam in `agent-core` (parity spec 13).
//!
//! The stack: a JSON-RPC [`protocol`] codec (`Content-Length` framing) → an
//! [`LspTransport`] (the real [`StdioTransport`] subprocess, or a scripted double
//! in tests) → an [`LspClient`] (handshake, document sync, the six methods,
//! diagnostics store) → an [`LspManager`] that pools one client per language and
//! implements the seam. See `docs/components/lsp.md`.
//!
//! The union differentiator (honestly): agent-seddon offers **both** diagnostics
//! (hermes' half) **and** hover/definition/references/document-symbols (opencode's
//! half) **and** `rename` (neither peer surfaces) behind one swap-by-config seam.

mod client;
mod manager;
mod parse;
pub mod protocol;
pub mod transport;

pub use client::LspClient;
pub use manager::{LspManager, ServerConfig};
pub use transport::{LspTransport, TransportFactory};

#[cfg(feature = "lsp-stdio")]
pub use transport::{StdioFactory, StdioTransport};

#[doc(hidden)]
pub use parse::bench_parse_diagnostics;

#[cfg(test)]
pub(crate) mod testing;
