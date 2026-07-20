//! Load user-provided context files from `<dir>/prepend/` and `<dir>/append/`.
//!
//! Files are named `NNNN_name.md`; the leading number orders them ascending
//! (files without a numeric prefix sort last, by name). Only `*.md` files are
//! read. Best-effort: a missing directory yields an empty list.

use agent_core::ContextBlock;
use std::path::Path;

/// Returns `(prepend, append)` context blocks, each sorted by numeric prefix.
pub fn load(dir: &str) -> (Vec<ContextBlock>, Vec<ContextBlock>) {
    let _span = tracing::info_span!("context_files.load", dir).entered();
    let base = Path::new(dir);
    let out = (
        load_subdir(&base.join("prepend")),
        load_subdir(&base.join("append")),
    );
    tracing::debug!(
        prepend = out.0.len(),
        append = out.1.len(),
        "loaded context files"
    );
    out
}

fn load_subdir(dir: &Path) -> Vec<ContextBlock> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(), // no such dir — fine
    };

    let mut files: Vec<(u64, String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("could not read context file `{}`: {e}", path.display());
                continue;
            }
        };
        files.push((numeric_prefix(&name), name, content));
    }

    // Sort by (numeric prefix, then filename) so ordering is deterministic.
    files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    files
        .into_iter()
        .map(|(_, source, content)| ContextBlock {
            source,
            content: content.trim_end().to_string(),
        })
        .collect()
}

/// Parse the leading run of ASCII digits; files without one sort last.
fn numeric_prefix(name: &str) -> u64 {
    let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_testkit::tempdir;
    use rstest::rstest;

    // --- numeric_prefix: leading-digit ordering key ------------------------
    #[rstest]
    #[case::positive_leading_zeros("0001_foo.md", 1)]
    #[case::positive_plain("123_x.md", 123)]
    #[case::positive_ten("0010_bar.md", 10)]
    #[case::positive_all_digits("42", 42)]
    #[case::negative_no_prefix("noprefix.md", u64::MAX)]
    #[case::boundary_empty("", u64::MAX)]
    #[case::corner_overflow_sorts_last("99999999999999999999_x.md", u64::MAX)]
    fn numeric_prefix_cases(#[case] name: &str, #[case] expected: u64) {
        assert_eq!(numeric_prefix(name), expected);
    }

    #[test]
    fn load_subdir_sorts_by_prefix_then_name_and_ignores_non_md() {
        let dir = tempdir();
        std::fs::write(dir.join("0002_b.md"), "B").unwrap();
        std::fs::write(dir.join("0001_a.md"), "A").unwrap();
        std::fs::write(dir.join("zzz.md"), "Z").unwrap(); // no prefix ⇒ sorts last
        std::fs::write(dir.join("ignore.txt"), "no").unwrap(); // non-.md ignored
        let sources: Vec<String> = load_subdir(&dir).into_iter().map(|b| b.source).collect();
        assert_eq!(sources, vec!["0001_a.md", "0002_b.md", "zzz.md"]);
    }

    #[test]
    fn missing_dir_is_empty() {
        let (pre, post) = load("/nonexistent/path/xyz");
        assert!(pre.is_empty() && post.is_empty());
    }

    // Loading emits a `context_files.load` span so the startup path is observable.
    #[test]
    fn load_emits_span() {
        let spans = agent_testkit::observe::captured_spans(|| {
            let _ = load("/nonexistent/path/xyz");
        });
        assert!(
            spans.contains(&"context_files.load".to_string()),
            "{spans:?}"
        );
    }
}
