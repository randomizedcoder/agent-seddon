//! Session/user identity propagation over tonic gRPC metadata.
//!
//! Multi-session identity rides the wire as two ASCII metadata keys, injected once
//! on the client and extracted once on the server — the same choke-points and
//! lifecycle as the W3C trace context in [`crate::trace`], and deliberately **not**
//! part of any `.proto` message (so `buf breaking` never sees it and the change is
//! additive). See docs/design/multi-session/01-identity.md.
//!
//! The values are attacker-controllable (there is no auth layer yet — see
//! docs/design/multi-session/07-security.md), so the extracting side validates them
//! with `agent_core::safe_segment` before using either as a key or a path component.
//! These helpers only move the raw strings; validation is the caller's fail-closed
//! step.

use tonic::metadata::{MetadataMap, MetadataValue};

/// Metadata key carrying the session id (lowercase ASCII, per HTTP/2 header rules).
pub const SESSION_ID_KEY: &str = "x-agent-session-id";
/// Metadata key carrying the user id.
pub const USER_ID_KEY: &str = "x-agent-user-id";

/// Inject a `(user, session)` identity into outgoing request metadata. A value that
/// cannot be encoded as ASCII metadata is skipped rather than panicking — the server
/// then treats it as absent and fails closed on a stateful seam.
pub fn inject_identity(user: &str, session: &str, meta: &mut MetadataMap) {
    if let Ok(v) = MetadataValue::try_from(user) {
        meta.insert(USER_ID_KEY, v);
    }
    if let Ok(v) = MetadataValue::try_from(session) {
        meta.insert(SESSION_ID_KEY, v);
    }
}

/// Extract the raw `(user, session)` identity strings from incoming request
/// metadata, each `None` if absent or non-ASCII. The caller validates
/// (`agent_core::safe_segment`) and decides the fail-closed policy per seam.
pub fn extract_identity(meta: &MetadataMap) -> (Option<String>, Option<String>) {
    let user = meta
        .get(USER_ID_KEY)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let session = meta
        .get(SESSION_ID_KEY)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (user, session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_identity_roundtrips_through_metadata() {
        let mut meta = MetadataMap::new();
        inject_identity("alice", "sess-1", &mut meta);
        assert_eq!(meta.get(USER_ID_KEY).unwrap().to_str().unwrap(), "alice");
        assert_eq!(
            meta.get(SESSION_ID_KEY).unwrap().to_str().unwrap(),
            "sess-1"
        );
        let (u, s) = extract_identity(&meta);
        assert_eq!(u.as_deref(), Some("alice"));
        assert_eq!(s.as_deref(), Some("sess-1"));
    }

    #[test]
    fn boundary_absent_identity_extracts_none() {
        let (u, s) = extract_identity(&MetadataMap::new());
        assert!(u.is_none() && s.is_none());
    }

    #[test]
    fn adversarial_non_ascii_value_is_skipped_not_panicked() {
        // A non-ASCII value cannot be an HTTP/2 header; injection skips it, so the
        // server sees "absent" and fails closed rather than the process panicking.
        let mut meta = MetadataMap::new();
        inject_identity("wíth-ünicode", "sess", &mut meta);
        let (u, s) = extract_identity(&meta);
        assert!(u.is_none());
        assert_eq!(s.as_deref(), Some("sess"));
    }
}
