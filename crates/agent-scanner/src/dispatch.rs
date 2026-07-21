//! `DispatchScanner` — run every configured sub-scanner and merge findings.
//!
//! Mirrors `DispatchSearch` in `agent-search`: the composition is itself a
//! `Scanner`, so callers (and the gRPC service) see one seam regardless of how
//! many rules are wired.

use agent_core::{Finding, ScanKind, Scanner};
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;

pub struct DispatchScanner {
    scanners: Vec<Arc<dyn Scanner>>,
    /// Rule ids that are detected but **not** reported — the suppression list, so
    /// a known fixture secret or an accepted false positive can be waived without
    /// turning the whole scanner off.
    allowlist: HashSet<String>,
}

impl DispatchScanner {
    pub fn new(scanners: Vec<Arc<dyn Scanner>>) -> Self {
        Self {
            scanners,
            allowlist: HashSet::new(),
        }
    }

    /// Waive these rule ids (config `[scanner] allow_rules`).
    pub fn with_allowlist(mut self, rules: impl IntoIterator<Item = String>) -> Self {
        self.allowlist = rules.into_iter().collect();
        self
    }

    /// `true` when nothing is wired (used to skip work entirely).
    pub fn is_empty(&self) -> bool {
        self.scanners.is_empty()
    }
}

#[async_trait]
impl Scanner for DispatchScanner {
    fn name(&self) -> &str {
        "dispatch"
    }

    async fn scan(&self, kind: ScanKind, content: &str) -> Vec<Finding> {
        let mut out = Vec::new();
        for s in &self.scanners {
            out.extend(s.scan(kind, content).await);
        }
        // Suppression applies after detection so metrics still reflect what the
        // rules saw, and a waived rule can be un-waived without a rescan.
        out.retain(|f| !self.allowlist.contains(&f.rule));
        // Most severe first, so a caller reading `first()` sees the worst.
        out.sort_by_key(|f| std::cmp::Reverse(f.severity));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{max_severity, Severity};
    use rstest::rstest;

    struct Stub(&'static str, Severity);
    #[async_trait]
    impl Scanner for Stub {
        fn name(&self) -> &str {
            "stub"
        }
        async fn scan(&self, _k: ScanKind, _c: &str) -> Vec<Finding> {
            vec![Finding {
                rule: self.0.to_string(),
                severity: self.1,
                category: "test",
                span: 0..1,
            }]
        }
    }

    #[tokio::test]
    async fn positive_merges_every_scanner() {
        let d = DispatchScanner::new(vec![
            Arc::new(Stub("a.rule", Severity::Low)),
            Arc::new(Stub("b.rule", Severity::High)),
        ]);
        let got = d.scan(ScanKind::FileBody, "x").await;
        assert_eq!(got.len(), 2);
        // Sorted most-severe first.
        assert_eq!(got[0].rule, "b.rule");
        assert_eq!(max_severity(&got), Some(Severity::High));
    }

    #[rstest]
    #[case::positive_suppressed_rule(true, 0)]
    #[case::negative_unsuppressed_rule(false, 1)]
    #[tokio::test]
    async fn suppression_waives(#[case] allowlisted: bool, #[case] want: usize) {
        let mut d = DispatchScanner::new(vec![Arc::new(Stub(
            "secret.aws_access_key",
            Severity::High,
        ))]);
        if allowlisted {
            d = d.with_allowlist(["secret.aws_access_key".to_string()]);
        }
        assert_eq!(d.scan(ScanKind::FileBody, "x").await.len(), want);
    }

    #[tokio::test]
    async fn boundary_no_scanners_is_clean() {
        let d = DispatchScanner::new(vec![]);
        assert!(d.is_empty());
        assert!(d.scan(ScanKind::FileBody, "x").await.is_empty());
        assert_eq!(max_severity(&[]), None);
    }
}
