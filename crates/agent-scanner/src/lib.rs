//! Content security scanning behind the `Scanner` seam (parity spec 18).
//!
//! Three sub-scanners, composed by [`DispatchScanner`]:
//!
//! * [`SecretScanner`] — labelled credential patterns + a Shannon-entropy pass.
//! * [`ThreatScanner`] — prompt injection / exfiltration / invisible unicode.
//! * (OSV vulnerability lookup is network-bound and deferred; see
//!   `docs/components/scanner.md`.)
//!
//! The differentiator is not detection but **integration**: findings flow into
//! the `Policy` gate (`crates/agent-runtime/src/policy.rs`), so a secret in a
//! `write_file` body denies the write rather than merely logging.

#[cfg(feature = "scanner-secret")]
pub mod secret;
#[cfg(feature = "scanner-threat")]
pub mod threat;

mod dispatch;

pub use dispatch::DispatchScanner;
#[cfg(feature = "scanner-secret")]
pub use secret::SecretScanner;
#[cfg(feature = "scanner-threat")]
pub use threat::{Scope, ThreatScanner};

/// Bench hook: the secret + entropy scan over a fixed buffer (the CPU hot path).
#[cfg(feature = "scanner-secret")]
#[doc(hidden)]
pub fn bench_scan_secrets(content: &str) -> usize {
    secret::scan_secrets(content, true).len()
}

/// Bench hook: the threat-pattern scan over a fixed buffer.
#[cfg(feature = "scanner-threat")]
#[doc(hidden)]
pub fn bench_scan_threats(content: &str) -> usize {
    threat::scan_threats(content, threat::Scope::Strict).len()
}
