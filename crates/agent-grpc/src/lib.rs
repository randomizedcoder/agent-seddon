//! `agent-grpc` — gRPC transport for the agent-seddon seams.
//!
//! Each seam ([`agent_core::LlmProvider`], [`agent_core::MemoryStore`], …) gets a
//! [`server`] adapter that wraps a locally-built `Arc<dyn Trait>` and a [`client`]
//! that implements the same trait by calling a remote server — so a component can
//! run as its own process/container and the loop is none the wiser. Both TCP and
//! **unix domain sockets** are supported ([`transport`]); UDS is the fast path when
//! components share a host.
//!
//! Default ports and socket paths live in [`constants`], generated from
//! `nix/constants.nix` (the single source of truth). See `docs/grpc.md`.

pub mod client;
pub mod constants;
pub mod server;
pub mod transport;

pub use transport::{Bound, Endpoint};
