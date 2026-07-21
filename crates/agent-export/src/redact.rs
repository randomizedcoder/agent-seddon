//! Secret redaction applied before a transcript is rendered.
//!
//! A transcript is the worst place for a leaked credential: it is the artifact
//! people paste into bug reports and PR descriptions. Redaction runs on the text
//! *before* any format sees it, so all three renderers inherit it.
//!
//! Uses the [`Scanner`](agent_core::Scanner) seam when one is wired (spec 18 —
//! this is the first consumer of `Finding.span`), and falls back to a built-in
//! matcher otherwise, so export is safe even in a build without the scanner.
//!
//! Redaction must itself be **deterministic**: the same transcript redacts to
//! the same bytes, or byte-stability is lost and the golden tests mean nothing.

use agent_core::{Finding, ScanKind, Scanner};

/// Replace each finding's span with a stable `[redacted:<rule>]` marker.
///
/// Spans are applied **back to front** so earlier offsets stay valid as the text
/// shifts, and overlapping findings are dropped rather than producing
/// interleaved garbage.
pub fn apply(content: &str, mut findings: Vec<Finding>) -> String {
    if findings.is_empty() {
        return content.to_string();
    }
    // Deterministic order regardless of the order rules fired in.
    findings.sort_by(|a, b| a.span.start.cmp(&b.span.start).then(a.rule.cmp(&b.rule)));

    let mut kept: Vec<Finding> = Vec::with_capacity(findings.len());
    let mut last_end = 0usize;
    for f in findings {
        // A span from an untrusted-ish source must be in range and non-inverted.
        if f.span.start >= f.span.end || f.span.end > content.len() {
            continue;
        }
        if f.span.start < last_end {
            continue; // overlaps something already kept
        }
        if !content.is_char_boundary(f.span.start) || !content.is_char_boundary(f.span.end) {
            continue; // never slice mid-char
        }
        last_end = f.span.end;
        kept.push(f);
    }

    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for f in &kept {
        out.push_str(&content[cursor..f.span.start]);
        out.push_str(&format!("[redacted:{}]", f.rule));
        cursor = f.span.end;
    }
    out.push_str(&content[cursor..]);
    out
}

/// Redact `content` using `scanner`.
pub async fn redact_with(scanner: &dyn Scanner, content: &str) -> String {
    let findings = scanner.scan(ScanKind::FileBody, content).await;
    apply(content, findings)
}

/// The built-in fallback matcher, for builds without the scanner seam.
///
/// Deliberately narrow — the high-confidence, unambiguous credential shapes.
/// Export must not mangle ordinary prose, so this errs toward under-redacting
/// and the `Scanner` is the thorough path.
pub fn fallback_findings(content: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut push = |rule: &str, start: usize, end: usize| {
        out.push(Finding {
            rule: rule.to_string(),
            severity: agent_core::Severity::High,
            category: "secret",
            span: start..end,
        });
    };

    // AWS access keys: AKIA/ASIA + 16 uppercase alphanumerics.
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while i + 20 <= bytes.len() {
        let w = &content[i..];
        if (w.starts_with("AKIA") || w.starts_with("ASIA"))
            && w.as_bytes()[4..20]
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            push("secret.aws_access_key", i, i + 20);
            i += 20;
            continue;
        }
        i += 1;
    }
    // Private key blocks: redact the header, which is enough to flag the block.
    let marker = "-----BEGIN";
    let mut from = 0usize;
    while let Some(rel) = content[from..].find(marker) {
        let start = from + rel;
        // Search for the CLOSING dashes after the marker itself — searching from
        // `start` finds the opening `-----`, which terminates the header
        // immediately and makes the rule never fire.
        let after_marker = start + marker.len();
        match content[after_marker..].find("-----") {
            Some(endrel) => {
                let end = (after_marker + endrel + 5).min(content.len());
                if content[start..end].contains("PRIVATE KEY") {
                    push("secret.private_key", start, end);
                }
                from = end;
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::Severity;
    use rstest::rstest;

    fn f(rule: &str, start: usize, end: usize) -> Finding {
        Finding {
            rule: rule.into(),
            severity: Severity::High,
            category: "secret",
            span: start..end,
        }
    }

    #[test]
    fn positive_replaces_span_with_a_stable_marker() {
        let got = apply(
            "key = AKIAIOSFODNN7EXAMPLE done",
            vec![f("secret.aws", 6, 26)],
        );
        assert_eq!(got, "key = [redacted:secret.aws] done");
    }

    /// Multiple findings must all apply, and offsets must stay correct as the
    /// text shifts underneath them.
    #[test]
    fn positive_multiple_spans_all_apply() {
        let got = apply(
            "a SECRET1 b SECRET2 c",
            vec![f("r1", 2, 9), f("r2", 12, 19)],
        );
        assert_eq!(got, "a [redacted:r1] b [redacted:r2] c");
    }

    /// Redaction must be deterministic regardless of the order rules fired.
    #[test]
    fn positive_finding_order_does_not_change_output() {
        let a = apply(
            "a SECRET1 b SECRET2 c",
            vec![f("r1", 2, 9), f("r2", 12, 19)],
        );
        let b = apply(
            "a SECRET1 b SECRET2 c",
            vec![f("r2", 12, 19), f("r1", 2, 9)],
        );
        assert_eq!(a, b);
    }

    /// Spans are data; malformed ones must be skipped, never panic or slice
    /// mid-character.
    #[rstest]
    #[case::adversarial_out_of_range(0, 9_999)]
    #[case::adversarial_inverted(9, 2)]
    #[case::adversarial_empty_span(3, 3)]
    #[case::adversarial_end_past_len(5, 100)]
    fn adversarial_bad_spans_are_skipped(#[case] start: usize, #[case] end: usize) {
        let text = "hello world";
        let got = apply(text, vec![f("r", start, end)]);
        assert_eq!(got, text, "a malformed span must leave the text alone");
    }

    /// A span landing inside a multi-byte character must not panic.
    #[test]
    fn adversarial_multibyte_boundary_is_skipped() {
        let text = "héllo"; // 'é' occupies bytes 1..3
        let got = apply(text, vec![f("r", 2, 4)]);
        assert_eq!(got, text);
    }

    /// Overlapping findings must not interleave into garbage.
    #[test]
    fn corner_overlapping_findings_do_not_interleave() {
        let got = apply("AAAAAAAAAA", vec![f("r1", 0, 6), f("r2", 3, 9)]);
        assert_eq!(got, "[redacted:r1]AAAA");
    }

    #[rstest]
    #[case::positive_aws_key("key = AKIAIOSFODNN7EXAMPLE", true)]
    #[case::positive_private_key("-----BEGIN RSA PRIVATE KEY-----", true)]
    #[case::negative_prose("nothing secret in this sentence at all", false)]
    #[case::negative_lookalike("AKIA is a prefix but this is not a key", false)]
    #[case::boundary_empty("", false)]
    fn fallback_matcher_cases(#[case] input: &str, #[case] expect_hit: bool) {
        assert_eq!(!fallback_findings(input).is_empty(), expect_hit);
    }

    /// The fallback must actually redact when applied.
    #[test]
    fn positive_fallback_redacts_an_aws_key() {
        let text = "export AWS=AKIAIOSFODNN7EXAMPLE";
        let got = apply(text, fallback_findings(text));
        assert!(!got.contains("AKIAIOSFODNN7EXAMPLE"), "got: {got}");
        assert!(got.contains("[redacted:secret.aws_access_key]"));
    }
}
