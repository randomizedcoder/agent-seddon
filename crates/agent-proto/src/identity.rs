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

/// Metadata key carrying how many agent services a request has already passed
/// through (the sender's own hop count). Absent from a client that is not an agent
/// service. Security-hardening S9 (docs/design/security-hardening/04-service-integration.md).
pub const HOPS_KEY: &str = "x-agent-hops";

/// The most hops a request may already have taken when it arrives. A forwarding loop
/// between seams stops here instead of running until something falls over.
pub const MAX_HOPS: u8 = 4;

/// Why an inbound `x-agent-hops` value was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopsError {
    /// Not a small decimal number.
    Malformed,
    /// More than [`MAX_HOPS`] (a forwarding loop, or a caller claiming one).
    TooMany,
}

/// Parse an inbound `x-agent-hops` value into the sender's hop count. Absent ⇒ `0`
/// (a client, not a forwarding service). Only ASCII digits are accepted — no sign,
/// no whitespace, at most three of them — and a count over [`MAX_HOPS`] is refused.
/// The server's own hop is `count + 1`; a caller that sends a low number can hide
/// hops it made *before* reaching us, never the ones after.
pub fn parse_hops(raw: Option<&str>) -> Result<u8, HopsError> {
    let Some(raw) = raw else {
        return Ok(0);
    };
    if raw.is_empty() || raw.len() > 3 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(HopsError::Malformed);
    }
    let n: u16 = raw.parse().map_err(|_| HopsError::Malformed)?;
    match u8::try_from(n) {
        Ok(n) if n <= MAX_HOPS => Ok(n),
        _ => Err(HopsError::TooMany),
    }
}

/// Stamp this service's hop count on an outgoing request, replacing anything already
/// there. `0` (not inside a served request) sends nothing.
pub fn inject_hops(hops: u8, meta: &mut MetadataMap) {
    meta.remove(HOPS_KEY);
    if hops > 0 {
        meta.insert(HOPS_KEY, MetadataValue::from(u32::from(hops)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // desc: a client (no header) is hop 0.
    #[case::positive_absent_is_zero(None, Ok(0))]
    // desc: a forwarded call.
    #[case::positive_one(Some("1"), Ok(1))]
    // desc: the ceiling is accepted.
    #[case::boundary_max_ok(Some("4"), Ok(4))]
    // desc: one over the ceiling.
    #[case::boundary_over_max(Some("5"), Err(HopsError::TooMany))]
    // desc: leading zeros are still a small number.
    #[case::corner_leading_zero(Some("004"), Ok(4))]
    // desc: zero is a valid (if pointless) claim.
    #[case::corner_zero(Some("0"), Ok(0))]
    // desc: empty value.
    #[case::negative_empty(Some(""), Err(HopsError::Malformed))]
    // desc: a sign would let a caller pretend to be fewer hops.
    #[case::adversarial_negative(Some("-1"), Err(HopsError::Malformed))]
    // desc: plus sign.
    #[case::adversarial_plus(Some("+1"), Err(HopsError::Malformed))]
    // desc: whitespace padding.
    #[case::adversarial_space(Some(" 1"), Err(HopsError::Malformed))]
    // desc: overflow of u8.
    #[case::adversarial_huge(Some("256"), Err(HopsError::TooMany))]
    // desc: overflow of any integer.
    #[case::adversarial_very_long(Some("99999999999999999999"), Err(HopsError::Malformed))]
    // desc: not a number.
    #[case::adversarial_text(Some("two"), Err(HopsError::Malformed))]
    // desc: non-ASCII digits.
    #[case::adversarial_unicode_digit(Some("\u{0663}"), Err(HopsError::Malformed))]
    fn parse_hops_cases(#[case] raw: Option<&str>, #[case] want: Result<u8, HopsError>) {
        assert_eq!(parse_hops(raw), want);
    }

    #[rstest]
    // desc: a forwarding service stamps its count.
    #[case::positive_stamps(2, Some("2"))]
    // desc: outside a served request nothing is sent.
    #[case::boundary_zero_sends_nothing(0, None)]
    fn inject_hops_cases(#[case] hops: u8, #[case] want: Option<&str>) {
        let mut meta = MetadataMap::new();
        inject_hops(hops, &mut meta);
        assert_eq!(meta.get(HOPS_KEY).map(|v| v.to_str().unwrap()), want);
    }

    #[test]
    fn adversarial_inject_replaces_a_forged_value() {
        let mut meta = MetadataMap::new();
        meta.insert(HOPS_KEY, MetadataValue::from_static("0"));
        inject_hops(3, &mut meta);
        assert_eq!(meta.get(HOPS_KEY).unwrap().to_str().unwrap(), "3");
        // ...and a stale value is dropped when this is not a served request.
        inject_hops(0, &mut meta);
        assert!(meta.get(HOPS_KEY).is_none());
    }

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
