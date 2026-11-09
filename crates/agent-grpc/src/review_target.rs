//! The review target's wire form, shared by the `FactCollectorService` client and
//! server so the two can't drift: `pr:<n>` | `pr:<n>@<oid>` | `branch:<name>` |
//! `worktree` | `revs:<base>..<head>`.

use agent_core::{is_object_id, ReviewTarget};

/// Encode a target for the wire.
pub(crate) fn encode_target(t: &ReviewTarget) -> String {
    match t {
        ReviewTarget::Pr(n) => format!("pr:{n}"),
        ReviewTarget::PrAt { number, head } => format!("pr:{number}@{head}"),
        ReviewTarget::Branch(b) => format!("branch:{b}"),
        ReviewTarget::WorkingTree => "worktree".to_string(),
        ReviewTarget::Revs { base, head } => format!("revs:{base}..{head}"),
    }
}

/// Decode the wire target: `pr:<n>` | `pr:<n>@<oid>` | `branch:<name>` | `worktree` |
/// `revs:<base>..<head>`. The value is
/// attacker-controlled — a bad number or an unknown form is rejected, and an
/// unsafe branch name is caught downstream by the orchestrator's `safe_segment`.
pub(crate) fn decode_target(s: &str) -> Result<ReviewTarget, String> {
    if s == "worktree" {
        return Ok(ReviewTarget::WorkingTree);
    }
    if let Some(n) = s.strip_prefix("pr:") {
        if let Some((n, head)) = n.split_once('@') {
            let number = n
                .parse::<u64>()
                .map_err(|_| format!("invalid PR number `{n}`"))?;
            // Checked here, at the trust boundary, so no collector behind it ever
            // sees a head that is not an object id (the orchestrator checks again).
            if !is_object_id(head) {
                return Err(format!("PR head `{head}` is not a full object id"));
            }
            return Ok(ReviewTarget::PrAt {
                number,
                head: head.to_string(),
            });
        }
        return n
            .parse::<u64>()
            .map(ReviewTarget::Pr)
            .map_err(|_| format!("invalid PR number `{n}`"));
    }
    if let Some(b) = s.strip_prefix("branch:") {
        return Ok(ReviewTarget::Branch(b.to_string()));
    }
    if let Some(range) = s.strip_prefix("revs:") {
        let (base, head) = range
            .split_once("..")
            .ok_or_else(|| format!("malformed revs target `{s}` (want `revs:<base>..<head>`)"))?;
        return Ok(ReviewTarget::Revs {
            base: base.to_string(),
            head: head.to_string(),
        });
    }
    Err(format!("unrecognized review target `{s}`"))
}

#[cfg(test)]
mod tests {
    use super::{decode_target, encode_target};
    use agent_core::ReviewTarget;
    use rstest::rstest;

    #[rstest]
    #[case::worktree("worktree", ReviewTarget::WorkingTree)]
    #[case::pr("pr:42", ReviewTarget::Pr(42))]
    #[case::pr_at(&format!("pr:42@{}", "c".repeat(40)), ReviewTarget::PrAt { number: 42, head: "c".repeat(40) })]
    #[case::branch("branch:feature", ReviewTarget::Branch("feature".into()))]
    #[case::revs("revs:main..HEAD", ReviewTarget::Revs { base: "main".into(), head: "HEAD".into() })]
    fn positive_decodes_known_targets(#[case] wire: &str, #[case] want: ReviewTarget) {
        assert_eq!(decode_target(wire).unwrap(), want);
    }

    #[rstest]
    #[case::empty("")]
    #[case::unknown("nonsense")]
    #[case::pr_not_numeric("pr:abc")]
    #[case::pr_at_not_numeric(&format!("pr:x@{}", "c".repeat(40)))]
    #[case::pr_at_overflow(&format!("pr:99999999999999999999@{}", "c".repeat(40)))]
    #[case::pr_at_empty_head("pr:42@")]
    #[case::pr_at_short_head("pr:42@abc")]
    #[case::pr_at_one_long_head(&format!("pr:42@{}", "c".repeat(41)))]
    #[case::pr_at_ref_head("pr:42@refs/heads/main")]
    #[case::pr_at_option_head(&format!("pr:42@--{}", "c".repeat(38)))]
    #[case::revs_no_range("revs:mainHEAD")]
    fn adversarial_malformed_targets_are_rejected(#[case] wire: &str) {
        assert!(decode_target(wire).is_err(), "must reject `{wire}`");
    }

    /// Every target survives encode → decode (the client and server agree).
    #[rstest]
    #[case::pr(ReviewTarget::Pr(7))]
    #[case::pr_at(ReviewTarget::PrAt { number: 7, head: "c".repeat(40) })]
    #[case::branch(ReviewTarget::Branch("feature".into()))]
    #[case::worktree(ReviewTarget::WorkingTree)]
    #[case::revs(ReviewTarget::Revs { base: "main".into(), head: "HEAD".into() })]
    fn positive_targets_round_trip(#[case] t: ReviewTarget) {
        let wire = encode_target(&t);
        assert_eq!(decode_target(&wire).unwrap(), t);
    }
}
