//! `agent-web` — concrete [`WebBackend`] transports behind the seam in
//! `agent-core` (parity spec 11).
//!
//! The default backend, [`LocalWebBackend`], is a `reqwest`-backed read-only HTTP
//! client. It is **transport only**: it enforces the request's size / timeout /
//! redirect caps and returns the *raw* decoded body — the `web_fetch` tool
//! (`agent-tools`) owns MIME-gating and the HTML→`format` conversion so that
//! logic is unit-testable over a `FakeWebBackend` without a socket.
//!
//! The **SSRF destination screen is not here**: it lives in the `Policy` guard
//! (`agent-runtime`), which denies loopback / private / link-local / metadata
//! targets *before* the tool ever calls `fetch`. This backend applies a
//! defence-in-depth scheme check (http/https only) so a mis-wired caller can't
//! reach `file:`/`gopher:` even with the guard off. See
//! `docs/components/web-fetch.md`.

#[cfg(feature = "web-local")]
mod local;
#[cfg(feature = "web-local")]
pub use local::LocalWebBackend;
