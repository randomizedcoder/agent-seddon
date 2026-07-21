//! Secret detection: a labelled regex set plus a Shannon-entropy heuristic.
//!
//! The regex set is ported from opencode's HTTP-recorder redaction rules and
//! hermes' hardcoded-secret pattern; the entropy pass is ours, to catch novel
//! high-entropy credentials no fixed pattern anticipates. Both report a byte
//! span so a caller can point at (or later redact) the offending substring.

use agent_core::{Finding, ScanKind, Scanner, Severity};
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;

/// Cap on how much content is scanned. Content is attacker-influenced (a fetched
/// page, a model-authored file body), so worst-case runtime must be bounded
/// rather than proportional to whatever was supplied. Mirrors hermes'
/// `MAX_SCAN_CHARS`.
pub const MAX_SCAN_BYTES: usize = 64 * 1024;

/// `(rule id, severity, pattern)`. Ordering is the reporting order.
struct Rule {
    id: &'static str,
    severity: Severity,
    re: Regex,
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let mut rules = Vec::new();
    let mut add = |id: &'static str, severity: Severity, pat: &str| {
        // The patterns are compile-time constants in this file; a bad one is a
        // bug here, not untrusted input.
        rules.push(Rule {
            id,
            severity,
            re: Regex::new(pat).expect("built-in secret pattern compiles"),
        });
    };

    // A private key block is the most severe: it is unambiguous and directly usable.
    add(
        "secret.private_key",
        Severity::Critical,
        r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY-----",
    );
    add(
        "secret.aws_access_key",
        Severity::High,
        r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
    );
    add(
        "secret.github_token",
        Severity::High,
        r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
    );
    add(
        "secret.anthropic_key",
        Severity::High,
        r"\bsk-ant-[A-Za-z0-9_\-]{20,}",
    );
    add(
        "secret.openai_key",
        Severity::High,
        r"\bsk-[A-Za-z0-9]{32,}",
    );
    add(
        "secret.slack_token",
        Severity::High,
        r"\bxox[baprs]-[A-Za-z0-9-]{10,}",
    );
    // A secret-looking NAME assigned a long opaque VALUE (hermes' shape). The
    // value class excludes spaces and quotes so it stops at the literal's end.
    add(
        "secret.assignment",
        Severity::High,
        r#"(?i)\b(?:api[_-]?key|auth[_-]?token|access[_-]?token|bearer|credential|password|passwd|secret|token)\b\s*[=:]\s*["']([A-Za-z0-9+/=_\-]{16,})["']"#,
    );
    rules
});

/// Quoted or bare tokens that could plausibly be a credential — the entropy
/// pass's candidate set. Deliberately narrow so prose never reaches the
/// (relatively expensive) entropy computation.
static CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["'`]([A-Za-z0-9+/=_\-]{20,128})["'`]"#).unwrap());

/// Shannon entropy in bits/char.
fn shannon_entropy(s: &str) -> f64 {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0usize; 256];
    for &b in bytes {
        counts[b as usize] += 1;
    }
    let len = bytes.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Entropy at or above this (bits/char) is *necessary* but not sufficient — see
/// [`looks_like_secret`].
const ENTROPY_THRESHOLD: f64 = 3.5;

/// Longest run of consecutive letters a credential-like token may contain.
/// Dictionary-ish identifiers are one long alphabetic run; real credentials
/// interleave digits, so their runs are short.
const MAX_ALPHA_RUN: usize = 12;

/// Whether a candidate token looks like a credential.
///
/// **Entropy alone does not work**, which is worth stating because it is the
/// obvious implementation and it is wrong. Measured over representative tokens:
///
/// ```text
///   3.95  9f8a3b71c04e5d62aa17fe93bc408d51e7f2   (a real hex secret)
///   4.04  AbstractSingletonProxyFactoryBean      (an ordinary class name)
///   4.16  the_quick_brown_fox_jumps_over         (ordinary prose)
/// ```
///
/// Any threshold that catches the hex secret also flags both identifiers. So
/// entropy is combined with structure: a credential mixes letters **and** digits
/// and has no long alphabetic run, whereas identifiers and prose are long
/// alphabetic runs with no digits.
fn looks_like_secret(tok: &str) -> bool {
    let has_digit = tok.bytes().any(|b| b.is_ascii_digit());
    let has_alpha = tok.bytes().any(|b| b.is_ascii_alphabetic());
    if !has_digit || !has_alpha {
        return false;
    }
    let mut run = 0usize;
    let mut longest = 0usize;
    for b in tok.bytes() {
        if b.is_ascii_alphabetic() {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest <= MAX_ALPHA_RUN && shannon_entropy(tok) >= ENTROPY_THRESHOLD
}

/// Detects credentials in content by pattern and by entropy.
pub struct SecretScanner {
    entropy: bool,
}

impl Default for SecretScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretScanner {
    pub fn new() -> Self {
        Self { entropy: true }
    }
    /// Disable the entropy heuristic (patterns only).
    pub fn without_entropy(mut self) -> Self {
        self.entropy = false;
        self
    }
}

#[async_trait]
impl Scanner for SecretScanner {
    fn name(&self) -> &str {
        "secret"
    }

    async fn scan(&self, _kind: ScanKind, content: &str) -> Vec<Finding> {
        scan_secrets(content, self.entropy)
    }
}

/// The pure scan — also the bench entry point (see `benches/scan.rs`).
pub fn scan_secrets(content: &str, entropy: bool) -> Vec<Finding> {
    let scanned = bounded(content);
    let mut out = Vec::new();

    for rule in RULES.iter() {
        for m in rule.re.find_iter(scanned) {
            out.push(Finding {
                rule: rule.id.to_string(),
                severity: rule.severity,
                category: "secret",
                span: m.start()..m.end(),
            });
        }
    }

    if entropy {
        for caps in CANDIDATE.captures_iter(scanned) {
            let tok = caps.get(1).expect("group 1 is in the pattern");
            // Skip anything a named rule already covers, so one secret does not
            // produce two findings at different severities.
            if out
                .iter()
                .any(|f| f.span.start <= tok.start() && tok.end() <= f.span.end)
            {
                continue;
            }
            if looks_like_secret(tok.as_str()) {
                out.push(Finding {
                    rule: "secret.high_entropy".to_string(),
                    severity: Severity::Medium,
                    category: "secret",
                    span: tok.start()..tok.end(),
                });
            }
        }
    }
    out
}

/// Truncate to [`MAX_SCAN_BYTES`] on a char boundary (slicing mid-char panics).
pub(crate) fn bounded(content: &str) -> &str {
    if content.len() <= MAX_SCAN_BYTES {
        return content;
    }
    let mut cut = MAX_SCAN_BYTES;
    while cut > 0 && !content.is_char_boundary(cut) {
        cut -= 1;
    }
    &content[..cut]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn first(findings: &[Finding]) -> Option<(&str, Severity)> {
        findings.first().map(|f| (f.rule.as_str(), f.severity))
    }

    #[rstest]
    #[case::positive_aws_key(
        "aws_secret = \"AKIAIOSFODNN7EXAMPLE\"\n",
        Some(("secret.aws_access_key", Severity::High))
    )]
    #[case::positive_github_token(
        "token: ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        Some(("secret.github_token", Severity::High))
    )]
    #[case::positive_private_key(
        "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n",
        Some(("secret.private_key", Severity::Critical))
    )]
    #[case::positive_anthropic_key(
        "key = \"sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFF\"",
        Some(("secret.anthropic_key", Severity::High))
    )]
    #[case::positive_assignment_secret(
        "api_key = \"s3cr3tValue0123456789abcdef\"",
        Some(("secret.assignment", Severity::High))
    )]
    #[case::corner_high_entropy_token(
        "const k = \"9f8a3b71c04e5d62aa17fe93bc408d51e7f2\";",
        Some(("secret.high_entropy", Severity::Medium))
    )]
    #[case::negative_benign_text("The quick brown fox writes clean, secret-free code.\n", None)]
    #[case::negative_low_entropy_word("password = \"password\"", None)]
    #[case::negative_prose_with_quotes("he said \"this is a perfectly normal sentence\"", None)]
    // Entropy-only would flag these: they score HIGHER than a real hex secret.
    #[case::negative_class_name_higher_entropy_than_a_secret(
        "let t = \"AbstractSingletonProxyFactoryBean\";",
        None
    )]
    #[case::negative_snake_case_prose("let s = \"the_quick_brown_fox_jumps_over\";", None)]
    #[case::negative_uuid_is_not_a_secret("id = \"550e8400-e29b-41d4-a716-446655440000\"", None)]
    #[case::positive_base64_secret(
        "k = \"aGVsbG8td29ybGQtc2VjcmV0LTEyMzQ1Njc4OTA\"",
        Some(("secret.high_entropy", Severity::Medium))
    )]
    #[case::boundary_empty("", None)]
    fn secret_scan_cases(#[case] content: &str, #[case] expected: Option<(&str, Severity)>) {
        let got = scan_secrets(content, true);
        assert_eq!(first(&got), expected, "content: {content:?}");
    }

    /// The span must point at the secret, so a caller can redact exactly it.
    #[test]
    fn positive_span_points_at_the_secret() {
        let content = "aws_secret = \"AKIAIOSFODNN7EXAMPLE\"\n";
        let f = &scan_secrets(content, true)[0];
        assert_eq!(&content[f.span.clone()], "AKIAIOSFODNN7EXAMPLE");
    }

    /// One secret must not be double-reported by both a named rule and entropy.
    #[test]
    fn corner_named_rule_suppresses_entropy_duplicate() {
        let got = scan_secrets("api_key = \"s3cr3tValue0123456789abcdef\"", true);
        assert_eq!(got.len(), 1, "got {got:?}");
        assert_eq!(got[0].rule, "secret.assignment");
    }

    /// Content is attacker-influenced, so scan cost must stay bounded rather
    /// than scaling with whatever was supplied.
    #[rstest]
    #[case::adversarial_huge_input(MAX_SCAN_BYTES * 4)]
    #[case::boundary_exactly_at_cap(MAX_SCAN_BYTES)]
    fn adversarial_scan_is_length_bounded(#[case] n: usize) {
        let content = "a".repeat(n);
        assert!(bounded(&content).len() <= MAX_SCAN_BYTES);
        // Must not panic, and must terminate.
        let _ = scan_secrets(&content, true);
    }

    /// A multi-byte char straddling the cap must not panic the slice.
    #[test]
    fn adversarial_multibyte_at_cap_does_not_panic() {
        let mut content = "é".repeat(MAX_SCAN_BYTES); // 2 bytes each
        content.push_str("AKIAIOSFODNN7EXAMPLE");
        let _ = scan_secrets(&content, true);
    }

    /// A secret past the cap is not reported — an honest limitation worth
    /// pinning, so nobody assumes unbounded coverage.
    #[test]
    fn boundary_secret_beyond_cap_is_not_scanned() {
        let mut content = "x".repeat(MAX_SCAN_BYTES + 10);
        content.push_str("AKIAIOSFODNN7EXAMPLE");
        assert!(scan_secrets(&content, true).is_empty());
    }
}
