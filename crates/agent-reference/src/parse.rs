//! The `@`-reference grammar — pure, deterministic, and the CPU hot path the iai
//! bench guards. Parses `@file:PATH[:START[-END]]`, `@dir:PATH`, `@symbol:NAME`,
//! `@url:URL` (quoted values allowed) from arbitrary prompt text, order-preserving
//! and deduped. An unknown `@kind:` or a malformed mention is *not a reference*
//! (left verbatim), never an error.

use agent_core::{RefKind, Reference};
use std::collections::HashSet;

/// The dedup key: a reference's kind, target, and optional line range.
type RefKey = (RefKind, String, Option<(u32, u32)>);

/// Parse all `@`-references from `prompt`, in order, deduped by (kind, target, range).
pub fn parse(prompt: &str) -> Vec<Reference> {
    let chars: Vec<char> = prompt.chars().collect();
    let n = chars.len();
    let mut refs = Vec::new();
    let mut seen: HashSet<RefKey> = HashSet::new();
    let mut i = 0;

    while i < n {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        // Word boundary: no match inside `foo@bar` / `foo.bar@x` (email-like).
        if i > 0 {
            let prev = chars[i - 1];
            if prev.is_alphanumeric() || prev == '.' || prev == '_' || prev == '@' {
                i += 1;
                continue;
            }
        }
        // Read the kind (lowercase letters) then require ':'.
        let kstart = i + 1;
        let mut j = kstart;
        while j < n && chars[j].is_ascii_lowercase() {
            j += 1;
        }
        if j >= n || chars[j] != ':' {
            i += 1;
            continue;
        }
        let kind_str: String = chars[kstart..j].iter().collect();
        let Some(kind) = RefKind::parse(&kind_str) else {
            i += 1;
            continue;
        };

        // Read the value: quoted (`"…"`, may contain spaces) or up to whitespace.
        let mut k = j + 1;
        let (value, next) = if k < n && chars[k] == '"' {
            let vstart = k + 1;
            let mut m = vstart;
            while m < n && chars[m] != '"' {
                m += 1;
            }
            let v: String = chars[vstart..m].iter().collect();
            let mut after = if m < n { m + 1 } else { m };
            // Optional `:range` after the closing quote.
            let mut range = String::new();
            if after < n && chars[after] == ':' {
                let rs = after + 1;
                let mut p = rs;
                while p < n && (chars[p].is_ascii_digit() || chars[p] == '-') {
                    p += 1;
                }
                range = chars[rs..p].iter().collect();
                if !range.is_empty() {
                    after = p;
                }
            }
            let joined = if range.is_empty() {
                v
            } else {
                format!("{v}:{range}")
            };
            (joined, after)
        } else {
            let vstart = k;
            let mut m = vstart;
            while m < n && !chars[m].is_whitespace() {
                m += 1;
            }
            let raw: String = chars[vstart..m].iter().collect();
            let trimmed = raw
                .trim_end_matches([',', '.', ';', '!', '?', ')', ']'])
                .to_string();
            (trimmed, m)
        };
        k = next; // (assignment keeps clippy quiet about the unused write above)
        i = k;

        if value.is_empty() {
            continue;
        }
        // Only `@file` carries a `:START[-END]` line range.
        let (target, range) = if kind == RefKind::File {
            split_range(&value)
        } else {
            (value, None)
        };
        if target.is_empty() {
            continue;
        }
        let key = (kind, target.clone(), range);
        if seen.insert(key) {
            refs.push(Reference {
                kind,
                target,
                range,
            });
        }
    }
    refs
}

/// Split a trailing `:N` / `:N-M` line range off a `@file` target.
fn split_range(value: &str) -> (String, Option<(u32, u32)>) {
    if let Some(idx) = value.rfind(':') {
        if let Some(r) = parse_range(&value[idx + 1..]) {
            return (value[..idx].to_string(), Some(r));
        }
    }
    (value.to_string(), None)
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    if s.is_empty() {
        return None;
    }
    match s.split_once('-') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => {
            let n = s.parse().ok()?;
            Some((n, n))
        }
    }
}

/// Bench hook (dependency of `benches/parse.rs`): parse a prompt, report ref count.
#[doc(hidden)]
pub fn bench_parse(prompt: &str) -> usize {
    parse(prompt).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// An inclusive `(start, end)` line range, as the grammar yields it.
    type Range = Option<(u32, u32)>;
    /// A parsed reference flattened for comparison: `(kind, target, range)`.
    type Triple = (&'static str, String, Range);
    /// The same shape as written in a `#[case]` (borrowed target).
    type ExpectedTriple<'a> = (&'a str, &'a str, Range);

    fn triples(refs: &[Reference]) -> Vec<Triple> {
        refs.iter()
            .map(|r| (r.kind.as_str(), r.target.clone(), r.range))
            .collect()
    }

    #[rstest]
    #[case::positive_single_file("explain @file:src/lib.rs please", vec![("file", "src/lib.rs", None)])]
    #[case::positive_file_range("look at @file:src/lib.rs:40-80", vec![("file", "src/lib.rs", Some((40, 80)))])]
    #[case::positive_file_single_line("line @file:a.rs:12 here", vec![("file", "a.rs", Some((12, 12)))])]
    #[case::positive_quoted_path_with_space("@file:\"my dir/a b.rs\"", vec![("file", "my dir/a b.rs", None)])]
    #[case::positive_mixed_kinds(
        "port @symbol:AuthService per @url:https://x.test/rfc into @dir:src/auth",
        vec![("symbol", "AuthService", None), ("url", "https://x.test/rfc", None), ("dir", "src/auth", None)])]
    #[case::corner_trailing_punctuation_trimmed("see @file:a.rs, then @dir:src.", vec![("file", "a.rs", None), ("dir", "src", None)])]
    #[case::negative_email_not_a_ref("mail foo@bar.com about it", vec![])]
    #[case::negative_unknown_kind_passthrough("@wat:thing is not a ref", vec![])]
    #[case::corner_dedup_identical("@file:a.rs and again @file:a.rs", vec![("file", "a.rs", None)])]
    #[case::negative_malformed_missing_target("@file: with no path", vec![])]
    #[case::boundary_no_refs_in_prose("just a normal sentence with an @ sign", vec![])]
    #[case::corner_quoted_with_range("@file:\"a b.rs\":10-20", vec![("file", "a b.rs", Some((10, 20)))])]
    fn parse_reference_cases(#[case] input: &str, #[case] expected: Vec<ExpectedTriple<'_>>) {
        let got = triples(&parse(input));
        let want: Vec<_> = expected
            .into_iter()
            .map(|(k, t, r)| (k, t.to_string(), r))
            .collect();
        assert_eq!(got, want, "input: {input}");
    }
}
