//! Normalization, dedup, and deterministic ranking of provider results.
//!
//! Providers disagree on everything — field names, whether a score is returned,
//! how a URL is spelled. This module flattens that into one stable order, so the
//! same inputs always produce the same output (which is what makes the tool
//! benchable and its tests meaningful).

use agent_core::WebResult;
use std::collections::HashSet;

/// Hard caps on what reaches the model. The payload is attacker-influenced (a
/// provider — or a page a provider indexed — chooses the text), so these are
/// enforced, not advisory.
pub const MAX_RESULTS: usize = 20;
pub const MAX_SNIPPET_CHARS: usize = 1_000;
pub const MAX_TOTAL_CHARS: usize = 20_000;

/// Canonical form of a URL for dedup: lowercase scheme+host, no default port,
/// no trailing slash on an empty path, no fragment, and common tracking params
/// dropped. Deliberately conservative — it only removes things that cannot
/// change what page you land on.
pub fn canonical_url(url: &str) -> String {
    let trimmed = url.trim();
    let (scheme, rest) = match trimmed.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => return trimmed.trim_end_matches('/').to_ascii_lowercase(),
    };
    // Strip the fragment; it never identifies a different document.
    let rest = rest.split('#').next().unwrap_or(rest);
    let (authority, path_and_query) = match rest.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (rest, None),
    };
    let mut authority = authority.to_ascii_lowercase();
    for (s, port) in [("http", ":80"), ("https", ":443")] {
        if scheme == s {
            if let Some(stripped) = authority.strip_suffix(port) {
                authority = stripped.to_string();
            }
        }
    }
    let mut out = format!("{scheme}://{authority}");
    if let Some(pq) = path_and_query {
        let (path, query) = match pq.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (pq, None),
        };
        let path = path.trim_end_matches('/');
        if !path.is_empty() {
            out.push('/');
            out.push_str(path);
        }
        if let Some(q) = query {
            let kept: Vec<&str> = q
                .split('&')
                .filter(|kv| !is_tracking_param(kv.split('=').next().unwrap_or(kv)))
                .filter(|kv| !kv.is_empty())
                .collect();
            if !kept.is_empty() {
                out.push('?');
                out.push_str(&kept.join("&"));
            }
        }
    }
    out
}

fn is_tracking_param(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_") || matches!(k.as_str(), "fbclid" | "gclid" | "msclkid" | "ref_src")
}

/// Dedup by canonical URL (first occurrence wins), sort into a **stable** order,
/// and apply the output caps.
///
/// Ordering is score-descending with the canonical URL as the tie-break, so ties
/// never depend on provider ordering or hash iteration — identical inputs give
/// byte-identical output.
pub fn rank_and_cap(results: Vec<WebResult>, limit: usize) -> Vec<WebResult> {
    // Canonicalize ONCE per result, not per comparison. `canonical_url`
    // allocates, and a comparator that calls it does so O(n log n) times on both
    // sides — which dominated the whole ranking cost before this was hoisted.
    let mut keyed: Vec<(String, WebResult)> = results
        .into_iter()
        .map(|mut r| {
            // Sanitize BEFORE sorting: a non-finite score can arrive from a
            // provider, and `partial_cmp` returns `None` for NaN — which
            // collapses to "equal" and silently scrambles the order rather than
            // sinking the bad entry. Normalizing here makes the compare total.
            if !r.score.is_finite() {
                r.score = 0.0;
            }
            r.score = r.score.clamp(0.0, 1.0);
            (canonical_url(&r.url), r)
        })
        .collect();

    let mut seen: HashSet<&str> = HashSet::with_capacity(keyed.len());
    let mut keep: Vec<bool> = Vec::with_capacity(keyed.len());
    for (key, _) in &keyed {
        keep.push(seen.insert(key.as_str()));
    }
    let mut i = 0;
    keyed.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });

    keyed.sort_by(|(ka, a), (kb, b)| b.score.total_cmp(&a.score).then_with(|| ka.cmp(kb)));
    let mut results: Vec<WebResult> = keyed.into_iter().map(|(_, r)| r).collect();

    let cap = if limit == 0 {
        MAX_RESULTS
    } else {
        limit.min(MAX_RESULTS)
    };
    results.truncate(cap);

    let mut total = 0usize;
    for r in &mut results {
        r.snippet = truncate_chars(&r.snippet, MAX_SNIPPET_CHARS);
        r.title = truncate_chars(&r.title, MAX_SNIPPET_CHARS);
        total += r.snippet.chars().count() + r.title.chars().count();
    }
    // Total-payload cap: drop whole trailing results rather than emitting a
    // half-truncated set, so what the model sees is always coherent.
    if total > MAX_TOTAL_CHARS {
        let mut running = 0usize;
        let mut keep = 0usize;
        for r in &results {
            running += r.snippet.chars().count() + r.title.chars().count();
            if running > MAX_TOTAL_CHARS {
                break;
            }
            keep += 1;
        }
        results.truncate(keep.max(1));
    }
    results
}

/// Truncate on a char boundary with an explicit marker.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…[truncated]")
}

/// Give un-scored backends a deterministic, rank-derived score in `(0,1]` so
/// results from different providers can be ordered on one scale.
pub fn score_from_rank(index: usize, total: usize) -> f32 {
    if total == 0 {
        return 0.0;
    }
    1.0 - (index as f32 / total as f32)
}

/// Bench hook: the full normalize → dedup → rank → cap path.
#[doc(hidden)]
pub fn bench_rank(results: Vec<WebResult>) -> usize {
    rank_and_cap(results, 0).len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn r(url: &str, score: f32) -> WebResult {
        WebResult {
            url: url.into(),
            title: "t".into(),
            snippet: "s".into(),
            score,
            published_ms: None,
        }
    }

    #[rstest]
    #[case::positive_strips_fragment("https://a.test/p#frag", "https://a.test/p")]
    #[case::positive_strips_default_port("https://a.test:443/p", "https://a.test/p")]
    #[case::positive_lowercases_host("HTTPS://A.TEST/p", "https://a.test/p")]
    #[case::positive_strips_trailing_slash("https://a.test/p/", "https://a.test/p")]
    #[case::positive_drops_utm("https://a.test/p?utm_source=x&q=1", "https://a.test/p?q=1")]
    #[case::corner_root_path("https://a.test/", "https://a.test")]
    #[case::corner_keeps_meaningful_query("https://a.test/p?id=7", "https://a.test/p?id=7")]
    #[case::boundary_no_scheme("a.test/p", "a.test/p")]
    fn canonical_url_cases(#[case] input: &str, #[case] want: &str) {
        assert_eq!(canonical_url(input), want);
    }

    /// Case-differing and fragment-differing URLs are the same page.
    #[test]
    fn positive_dedup_by_canonical_url() {
        let got = rank_and_cap(
            vec![
                r("https://a.test/p", 0.9),
                r("https://A.test/p#x", 0.8),
                r("https://b.test/q", 0.7),
            ],
            0,
        );
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].url, "https://a.test/p");
    }

    /// Identical inputs must give identical output regardless of arrival order —
    /// this is what makes the tool reproducible.
    #[test]
    fn positive_ranking_is_stable_across_input_order() {
        let a = vec![r("https://a.test/1", 0.5), r("https://b.test/2", 0.5)];
        let mut b = a.clone();
        b.reverse();
        let ra: Vec<String> = rank_and_cap(a, 0).into_iter().map(|r| r.url).collect();
        let rb: Vec<String> = rank_and_cap(b, 0).into_iter().map(|r| r.url).collect();
        assert_eq!(ra, rb, "tied scores must break deterministically");
    }

    /// A provider can return NaN; it must not make the sort order undefined.
    #[test]
    fn adversarial_nan_score_does_not_corrupt_order() {
        let got = rank_and_cap(
            vec![
                r("https://a.test/1", f32::NAN),
                r("https://b.test/2", 0.9),
                r("https://c.test/3", 0.1),
            ],
            0,
        );
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].url, "https://b.test/2", "highest real score first");
    }

    #[rstest]
    #[case::boundary_limit_respected(5, 5)]
    #[case::boundary_zero_limit_uses_max(0, MAX_RESULTS)]
    #[case::adversarial_limit_over_max_is_capped(10_000, MAX_RESULTS)]
    fn boundary_result_count_is_capped(#[case] limit: usize, #[case] want: usize) {
        let many: Vec<WebResult> = (0..100)
            .map(|i| r(&format!("https://a.test/{i}"), 1.0 - i as f32 / 100.0))
            .collect();
        assert_eq!(rank_and_cap(many, limit).len(), want);
    }

    /// A hostile provider snippet must not blow the context window.
    #[test]
    fn adversarial_huge_snippet_is_truncated() {
        let mut big = r("https://a.test/1", 1.0);
        big.snippet = "x".repeat(500_000);
        let got = rank_and_cap(vec![big], 0);
        assert!(got[0].snippet.chars().count() <= MAX_SNIPPET_CHARS + 20);
        assert!(got[0].snippet.contains("truncated"));
    }

    /// Many large-but-individually-legal snippets must still be bounded overall.
    #[test]
    fn adversarial_total_payload_is_bounded() {
        let many: Vec<WebResult> = (0..MAX_RESULTS)
            .map(|i| {
                let mut x = r(&format!("https://a.test/{i}"), 1.0);
                x.snippet = "y".repeat(MAX_SNIPPET_CHARS);
                x
            })
            .collect();
        let got = rank_and_cap(many, 0);
        let total: usize = got
            .iter()
            .map(|r| r.snippet.chars().count() + r.title.chars().count())
            .sum();
        assert!(total <= MAX_TOTAL_CHARS, "total {total} exceeds the cap");
        assert!(!got.is_empty(), "must keep at least one result");
    }

    /// A multi-byte snippet must truncate on a char boundary, not panic.
    #[test]
    fn adversarial_multibyte_snippet_truncates_safely() {
        let mut x = r("https://a.test/1", 1.0);
        x.snippet = "é".repeat(MAX_SNIPPET_CHARS * 2);
        let _ = rank_and_cap(vec![x], 0);
    }

    #[rstest]
    #[case::boundary_empty(0, 0, 0.0)]
    #[case::positive_first_is_highest(0, 4, 1.0)]
    #[case::positive_last_is_lowest(3, 4, 0.25)]
    fn score_from_rank_cases(#[case] i: usize, #[case] n: usize, #[case] want: f32) {
        assert!((score_from_rank(i, n) - want).abs() < 1e-6);
    }
}
