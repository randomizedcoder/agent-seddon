//! Deterministic input fixtures shared by benchmarks and the larger unit tests.
//!
//! Benchmarks (iai-callgrind) count instructions, so their inputs must be built
//! the *same way every run* — no clocks, no randomness. These builders are pure
//! functions of their arguments. They also serve the table-driven tests that want
//! a bigger corpus than an inline `#[case]` string.

use agent_core::Message;
use std::path::{Path, PathBuf};

/// Write a set of `(relative_path, contents)` files under `root`, creating parent
/// directories as needed, and return `root` for chaining. Used by the search /
/// read / edit benches to lay down a fixed tree.
///
/// Panics on any I/O error — fixtures are test-only and a failure is a bug.
pub fn write_tree(root: &Path, files: &[(&str, &str)]) -> PathBuf {
    for (rel, contents) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture parent dir");
        }
        std::fs::write(&path, contents).expect("write fixture file");
    }
    root.to_path_buf()
}

/// A synthetic HTML document of `n` article blocks — each a heading, a paragraph
/// with inline emphasis + an entity, and a `script`/`style` pair that the
/// sanitizer must strip. Deterministic input for the `web_fetch`
/// HTML→markdown/text conversion bench (and larger conversion tests).
pub fn html_document(n: usize) -> String {
    let mut s =
        String::from("<html><head><title>t</title><style>.x{color:red}</style></head><body>");
    for i in 0..n {
        s.push_str("<h2>Section ");
        s.push_str(&i.to_string());
        s.push_str("</h2><p>Paragraph ");
        s.push_str(&i.to_string());
        s.push_str(
            " with <strong>bold</strong> and <em>italic</em> &amp; entities.</p>\
             <script>evil()</script>",
        );
    }
    s.push_str("</body></html>");
    s
}

/// A synthetic conversation of `n` messages alternating user/assistant, each body
/// `body_len` bytes of a repeated filler char. Deterministic input for the
/// context-assembly / compaction benches and window tests.
pub fn message_history(n: usize, body_len: usize) -> Vec<Message> {
    (0..n)
        .map(|i| {
            let body = "x".repeat(body_len);
            if i % 2 == 0 {
                Message::user(body)
            } else {
                Message::assistant(body)
            }
        })
        .collect()
}

/// Write `n` markdown "facts" (`fact_00000.md` …) under `dir`, each containing
/// `keyword` plus its index, and return `dir`. Deterministic corpus for the
/// memory keyword-recall benches. Every file matches `keyword`, so recall ranking
/// is exercised over a known population size.
pub fn fact_corpus(dir: &Path, n: usize, keyword: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create fact corpus dir");
    for i in 0..n {
        let body = format!("# fact {i}\n\nThe {keyword} for record {i} is stable.\n");
        std::fs::write(dir.join(format!("fact_{i:05}.md")), body).expect("write fact");
    }
    dir.to_path_buf()
}
