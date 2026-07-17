//! Load user-provided context files from `<dir>/prepend/` and `<dir>/append/`.
//!
//! Files are named `NNNN_name.md`; the leading number orders them ascending
//! (files without a numeric prefix sort last, by name). Only `*.md` files are
//! read. Best-effort: a missing directory yields an empty list.

use agent_core::ContextBlock;
use std::path::Path;

/// Returns `(prepend, append)` context blocks, each sorted by numeric prefix.
pub fn load(dir: &str) -> (Vec<ContextBlock>, Vec<ContextBlock>) {
    let base = Path::new(dir);
    (
        load_subdir(&base.join("prepend")),
        load_subdir(&base.join("append")),
    )
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

    #[test]
    fn numeric_prefix_parsing() {
        assert_eq!(numeric_prefix("0001_foo.md"), 1);
        assert_eq!(numeric_prefix("0010_bar.md"), 10);
        assert_eq!(numeric_prefix("noprefix.md"), u64::MAX);
    }

    #[test]
    fn missing_dir_is_empty() {
        let (pre, post) = load("/nonexistent/path/xyz");
        assert!(pre.is_empty() && post.is_empty());
    }
}
