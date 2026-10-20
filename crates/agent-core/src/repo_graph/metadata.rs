//! Bounded, dual-reading metadata codec. Legacy array rows have unknown completeness.
use super::{
    ExtractReport, RepoGraphError, RepoGraphResult, SnapshotBegin, MAX_DIAGNOSTICS,
    MAX_DIAGNOSTIC_LEN,
};
use serde::{Deserialize, Serialize};

/// Maximum encoded metadata size, before JSON decoding.
pub const MAX_METADATA_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    extractors: Vec<String>,
    report: ExtractReport,
}

fn invalid() -> RepoGraphError {
    RepoGraphError::Backend("snapshot metadata invalid or unsupported".into())
}

/// Validate report bounds and prevent hostile diagnostics from reaching output.
pub fn validate_report(report: &ExtractReport) -> RepoGraphResult<()> {
    if report.diagnostics.len() > MAX_DIAGNOSTICS
        || report.diagnostics.iter().any(|s| {
            s.len() > MAX_DIAGNOSTIC_LEN
                || s.chars().any(char::is_control)
                || crate::scan_for_injection(s).is_some()
        })
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_names(names: &[String]) -> RepoGraphResult<()> {
    SnapshotBegin {
        repo: Default::default(),
        commit_sha: "0".repeat(40),
        extractors: names.to_vec(),
        extractor_version: String::new(),
    }
    .validate()
    .map_err(|_| invalid())
}

/// Encode only after upgraded readers are deployed. Disable indexing before rolling back.
pub fn encode_metadata(names: &[String], report: &ExtractReport) -> RepoGraphResult<String> {
    validate_names(names)?;
    validate_report(report)?;
    let text = serde_json::to_string(&Envelope {
        version: 1,
        extractors: names.to_vec(),
        report: report.clone(),
    })
    .map_err(|_| invalid())?;
    if text.len() > MAX_METADATA_BYTES {
        return Err(invalid());
    }
    Ok(text)
}

/// Return names and an optional report; absence means unknown, never complete.
pub fn decode_metadata(text: &str) -> RepoGraphResult<(Vec<String>, Option<ExtractReport>)> {
    if text.len() > MAX_METADATA_BYTES {
        return Err(invalid());
    }
    let (names, report) = if text.trim_start().starts_with('[') {
        (serde_json::from_str(text).map_err(|_| invalid())?, None)
    } else {
        let envelope: Envelope = serde_json::from_str(text).map_err(|_| invalid())?;
        if envelope.version != 1 {
            return Err(invalid());
        }
        validate_report(&envelope.report)?;
        (envelope.extractors, Some(envelope.report))
    };
    validate_names(&names)?;
    Ok((names, report))
}

/// Recovery is an operator action with a required, bounded, safe audit reason.
pub fn validate_recovery_reason(reason: &str) -> RepoGraphResult<()> {
    if reason.trim().is_empty()
        || reason.len() > super::MAX_REASON_LEN
        || reason.chars().any(char::is_control)
        || crate::scan_for_injection(reason).is_some()
    {
        return Err(RepoGraphError::Invalid("recovery reason".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    #[test]
    fn positive_report_roundtrip() {
        let r = ExtractReport {
            parse_errors: 2,
            truncated: true,
            ..Default::default()
        };
        assert_eq!(
            decode_metadata(&encode_metadata(&["rust-syn".into()], &r).unwrap()).unwrap(),
            (vec!["rust-syn".into()], Some(r))
        );
    }
    #[test]
    fn corner_legacy_metadata() {
        assert_eq!(
            decode_metadata("[\"cargo\"]").unwrap(),
            (vec!["cargo".into()], None)
        );
    }
    #[test]
    fn negative_unknown_report_version() {
        let text = encode_metadata(&[], &ExtractReport::default())
            .unwrap()
            .replace("\"version\":1", "\"version\":999");
        assert!(matches!(
            decode_metadata(&text),
            Err(RepoGraphError::Backend(_))
        ));
    }
    #[rstest]
    #[case::adversarial_malformed("{secret")]
    #[case::adversarial_extractor("[\"../x\"]")]
    #[case::boundary_size(&" ".repeat(MAX_METADATA_BYTES+1))]
    fn negative_metadata(#[case] text: &str) {
        assert!(matches!(
            decode_metadata(text),
            Err(RepoGraphError::Backend(_))
        ));
    }
    #[rstest]
    #[case::adversarial_control("\x1b[31m")]
    #[case::boundary_length(&"x".repeat(MAX_DIAGNOSTIC_LEN+1))]
    fn adversarial_report_payload(#[case] diagnostic: &str) {
        let r = ExtractReport {
            diagnostics: vec![diagnostic.into()],
            ..Default::default()
        };
        assert!(encode_metadata(&[], &r).is_err());
    }
}
