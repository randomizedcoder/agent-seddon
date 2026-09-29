//! Who asked for a fleet review, and who approved it (security-hardening S19,
//! docs/design/security-hardening/09-increments.md).
//!
//! A `ReviewNow` is queued and runs later under the fleet's own service identity: a
//! caller's 15-minute bearer would expire in the queue, and the review must not act as
//! the caller. So the caller is recorded, not impersonated. The handler turns the
//! verified principal into a [`requester_label`], the trigger carries it, coalescing
//! merges labels with [`merge_requesters`], and the draft row stores them. Approve
//! records its approver the same way.
//!
//! A label is built from a verified token's tenant and subject, but a subject is only
//! as tidy as the issuer that signed it, so it is stripped of control characters and
//! capped before it reaches a span or a ClickHouse row.

use crate::VerifiedPrincipal;

/// The most requesters one review remembers. Later ones are dropped (and logged by the
/// caller): the list is attribution, not an access-control input.
pub const MAX_REQUESTERS: usize = 8;

/// The longest requester label kept, in bytes. Longer labels are cut on a character
/// boundary.
pub const MAX_REQUESTER_BYTES: usize = 256;

/// The label a draft stores for a verified principal: `tenant/subject`, with control
/// characters removed and the result capped at [`MAX_REQUESTER_BYTES`].
pub fn requester_label(principal: &VerifiedPrincipal) -> String {
    let raw = format!("{}/{}", principal.tenant, principal.subject);
    let mut out = String::with_capacity(raw.len().min(MAX_REQUESTER_BYTES));
    for c in raw.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > MAX_REQUESTER_BYTES {
            break;
        }
        out.push(c);
    }
    out
}

/// Add `more` to `into`, keeping first-seen order, skipping duplicates and empty
/// labels, and stopping at [`MAX_REQUESTERS`]. Returns how many were dropped for the
/// cap, so the caller can log it.
pub fn merge_requesters(into: &mut Vec<String>, more: impl IntoIterator<Item = String>) -> usize {
    let mut dropped = 0;
    for label in more {
        if label.is_empty() || into.contains(&label) {
            continue;
        }
        if into.len() >= MAX_REQUESTERS {
            dropped += 1;
            continue;
        }
        into.push(label);
    }
    dropped
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn principal(tenant: &str, subject: &str) -> VerifiedPrincipal {
        VerifiedPrincipal {
            tenant: tenant.into(),
            subject: subject.into(),
            roles: vec!["operator".into()],
        }
    }

    /// desc: a verified principal becomes a bounded, printable `tenant/subject` label.
    #[rstest]
    #[case::positive_person("acme", "alice@example.com", "acme/alice@example.com")]
    #[case::positive_service("acme", "spiffe://acme/fleet", "acme/spiffe://acme/fleet")]
    #[case::corner_empty_subject("acme", "", "acme/")]
    #[case::adversarial_control_chars_stripped("acme", "ev\nil\u{1b}[2J", "acme/evil[2J")]
    fn requester_label_cases(#[case] tenant: &str, #[case] subject: &str, #[case] want: &str) {
        assert_eq!(requester_label(&principal(tenant, subject)), want);
    }

    /// desc: a hostile, huge subject is capped at the byte limit on a char boundary.
    #[rstest]
    #[case::boundary_exactly_at_cap("a".repeat(MAX_REQUESTER_BYTES - 2), MAX_REQUESTER_BYTES)]
    #[case::adversarial_huge_ascii("a".repeat(1 << 20), MAX_REQUESTER_BYTES)]
    #[case::adversarial_multibyte_not_split("é".repeat(1 << 12), MAX_REQUESTER_BYTES)]
    fn requester_label_is_capped(#[case] subject: String, #[case] want_len: usize) {
        let label = requester_label(&principal("t", &subject));
        assert_eq!(label.len(), want_len);
        assert!(label.len() <= MAX_REQUESTER_BYTES);
    }

    /// desc: merging keeps first-seen order, drops duplicates and blanks, stops at the cap.
    #[rstest]
    #[case::positive_second_requester_added(&["a/x"], &["a/y"], &["a/x", "a/y"], 0)]
    #[case::corner_same_requester_twice(&["a/x"], &["a/x"], &["a/x"], 0)]
    #[case::corner_empty_label_skipped(&[], &[""], &[], 0)]
    #[case::negative_nothing_to_add(&["a/x"], &[], &["a/x"], 0)]
    #[case::boundary_fills_to_cap(
        &["1", "2", "3", "4", "5", "6", "7"], &["8"], &["1", "2", "3", "4", "5", "6", "7", "8"], 0)]
    #[case::boundary_cap_8_drops_ninth(
        &["1", "2", "3", "4", "5", "6", "7", "8"], &["9"], &["1", "2", "3", "4", "5", "6", "7", "8"], 1)]
    fn merge_requesters_cases(
        #[case] start: &[&str],
        #[case] more: &[&str],
        #[case] want: &[&str],
        #[case] want_dropped: usize,
    ) {
        let mut into: Vec<String> = start.iter().map(ToString::to_string).collect();
        let dropped = merge_requesters(&mut into, more.iter().map(ToString::to_string));
        assert_eq!(into, want);
        assert_eq!(dropped, want_dropped);
    }

    /// desc: a flood of distinct requesters cannot grow the list past the cap.
    #[test]
    fn adversarial_requester_flood_stays_bounded() {
        let mut into = Vec::new();
        let dropped = merge_requesters(&mut into, (0..10_000).map(|i| format!("t/{i}")));
        assert_eq!(into.len(), MAX_REQUESTERS);
        assert_eq!(dropped, 10_000 - MAX_REQUESTERS);
    }
}
