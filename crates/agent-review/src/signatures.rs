//! `SignatureCollector` — the **cheap subset** of the AST/call-graph design
//! (increment 6): which top-level function *signatures* a change added, removed, or
//! altered. Deterministic structural fact — "this change touched these APIs" — that
//! a reviewer would otherwise reconstruct from the diff by eye (and get wrong).
//!
//! It reads the full base/head blobs of each changed Go/Rust file (not just the
//! hunks) and extracts function signatures with a small `regex`-anchored scanner —
//! no grammar/tree-sitter dependency, matching the rest of `agent-review`. A real
//! `AstBackend` (Go helper + `syn`) parsing full call-graphs is the deferred follow-up.
//!
//! Fail-soft + bounded: an unreadable/binary/huge blob is skipped, the change list
//! is capped (drop-with-count), signatures are length-bounded, paths are `confine`d.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, is_noisy, lang_of};
use agent_core::{ChangeKind, Revision, SignatureChange, SignatureReport};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

/// Cap on changed signatures surfaced (drop-with-count past it).
const MAX_CHANGES: usize = 100;
/// Cap on files read (a huge diff shouldn't fan out unbounded blob reads).
const MAX_FILES: usize = 100;
/// Skip a blob larger than this — a generated/vendored monster isn't worth scanning.
const MAX_BLOB_BYTES: u64 = 1_000_000;
/// One-line signature length cap.
const MAX_SIG: usize = 200;

pub(crate) struct SignatureCollector;

#[async_trait::async_trait]
impl FactCollector for SignatureCollector {
    fn name(&self) -> &'static str {
        "signatures"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        // Collectors run in parallel (no ChangeSet yet) — recompute the cached diff.
        let diff = match ctx.repo.diff(&ctx.base, &ctx.head, &[]).await {
            Ok(d) => d,
            Err(e) => {
                return CollectorOutput::failed(format!(
                    "diff failed: {}",
                    bound(&e.to_string(), 120)
                ))
            }
        };

        let mut changes: Vec<SignatureChange> = Vec::new();
        let mut files_scanned = 0u32;
        let mut truncated = false;

        for f in diff.files.iter().take(MAX_FILES) {
            // The head path names the file (base path for a delete); pick the lang.
            // Binary blobs are filtered in `read_side` (via `BlobContent.is_binary`).
            let path = f.new_path.as_deref().or(f.old_path.as_deref());
            let Some(path) = path else { continue };
            if is_noisy(path) {
                continue;
            }
            let lang = lang_of(path);
            if lang != "go" && lang != "rust" {
                continue;
            }

            // Pick each side's path. The git backend leaves `old_path` unset for a
            // plain modify (only renames/deletes carry it), so fall back to the
            // surviving path — otherwise every modify reads an empty base and every
            // function looks "added".
            let base_path = match f.change {
                ChangeKind::Added => None,
                _ => f.old_path.as_deref().or(f.new_path.as_deref()),
            };
            let head_path = match f.change {
                ChangeKind::Deleted => None,
                _ => f.new_path.as_deref().or(f.old_path.as_deref()),
            };
            // Read both sides from the object DB (empty for the absent side of an
            // add/delete, or when the blob is binary/oversized/unreadable).
            let base_text = read_side(ctx, &ctx.base, base_path).await;
            let head_text = read_side(ctx, &ctx.head, head_path).await;
            files_scanned += 1;

            let base_sigs = extract(&base_text, &lang);
            let head_sigs = extract(&head_text, &lang);
            let file_disp = path.to_string_lossy().into_owned();
            for mut c in diff_sigs(base_sigs, head_sigs) {
                // Confine the path (untrusted repo content) before it reaches output.
                if agent_core::confine(&ctx.repo_root, &file_disp).is_err() {
                    continue;
                }
                c.file = file_disp.clone();
                c.lang = lang.clone();
                if changes.len() >= MAX_CHANGES {
                    truncated = true;
                    break;
                }
                changes.push(c);
            }
            if truncated {
                break;
            }
        }

        if files_scanned == 0 {
            return CollectorOutput::skipped("no Go/Rust source changes");
        }

        // Keep file-major order (files in diff order, key-sorted within a file) so
        // the render can group by file without a re-sort.
        CollectorOutput::ok(FactFragment::Signatures {
            report: SignatureReport {
                changes,
                files_scanned,
                truncated,
            },
        })
    }
}

/// Read a file's text at a revision, or `""` if the side is absent (add/delete),
/// binary, oversized, or unreadable. Fail-soft — a missing blob is not an error.
async fn read_side(ctx: &CollectCtx, rev: &Revision, path: Option<&Path>) -> String {
    let Some(p) = path else { return String::new() };
    match ctx.repo.read_file(rev, p).await {
        Ok(b) if !b.is_binary && b.bytes_len <= MAX_BLOB_BYTES => b.text,
        _ => String::new(),
    }
}

/// One extracted signature: `key` disambiguates same-named items (Go receiver);
/// `name` is the display name; `sig` is the one-line-normalized signature.
struct Sig {
    key: String,
    name: String,
    sig: String,
}

/// Extract top-level function signatures from `text`. `lang` selects the anchor
/// pattern; the signature is scanned from the decl to the first `{`/`;`, normalized
/// to one bounded line. Best-effort — never panics on malformed input.
fn extract(text: &str, lang: &str) -> Vec<Sig> {
    if text.is_empty() {
        return Vec::new();
    }
    let re = if lang == "go" { go_re() } else { rust_re() };
    let mut out = Vec::new();
    for cap in re.captures_iter(text) {
        let Some(m) = cap.get(0) else { continue };
        let name = cap
            .name("name")
            .map(|x| x.as_str().to_string())
            .unwrap_or_default();
        // Go receiver type disambiguates methods that share a name.
        let key = match cap.name("recv") {
            Some(r) => format!("{}.{name}", recv_type(r.as_str())),
            None => name.clone(),
        };
        let sig = signature_from(&text[m.start()..]);
        if sig.is_empty() {
            continue;
        }
        out.push(Sig { key, name, sig });
    }
    out
}

/// The last whitespace-separated token of a Go receiver group, `*` stripped —
/// `s *Server` → `Server`, `Server` → `Server`.
fn recv_type(recv: &str) -> String {
    recv.split_whitespace()
        .last()
        .unwrap_or("")
        .trim_start_matches('*')
        .to_string()
}

/// Build a one-line signature from the decl start: accumulate up to the first `{`
/// (body) or `;` (bodyless decl), collapse whitespace, bound the length. Guarded
/// against runaway input by a line/char budget.
fn signature_from(s: &str) -> String {
    let mut buf = String::new();
    let mut lines = 0u32;
    for ch in s.chars() {
        match ch {
            '{' | ';' => break,
            '\n' => {
                lines += 1;
                if lines > 6 {
                    break;
                }
                buf.push(' ');
            }
            _ => buf.push(ch),
        }
        if buf.len() > MAX_SIG * 2 {
            break;
        }
    }
    let collapsed = buf.split_whitespace().collect::<Vec<_>>().join(" ");
    bound(collapsed.trim(), MAX_SIG)
}

/// Diff two signature sets into typed changes. Group by `key`; drop exact-unchanged
/// pairs; pair the rest by position as `modified`; leftovers are `added`/`removed`.
fn diff_sigs(base: Vec<Sig>, head: Vec<Sig>) -> Vec<SignatureChange> {
    // key -> (display name, signatures)
    let mut b: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    let mut h: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for s in base {
        let e = b.entry(s.key).or_insert_with(|| (s.name, Vec::new()));
        e.1.push(s.sig);
    }
    for s in head {
        let e = h.entry(s.key).or_insert_with(|| (s.name, Vec::new()));
        e.1.push(s.sig);
    }

    let mut keys: Vec<String> = b.keys().chain(h.keys()).cloned().collect();
    keys.sort();
    keys.dedup();

    let mut out = Vec::new();
    for key in keys {
        let (name, mut bs) = b.remove(&key).unwrap_or_default();
        let (name2, mut hs) = h.remove(&key).unwrap_or_default();
        let name = if name.is_empty() { name2 } else { name };
        // Drop signatures unchanged across base→head (exact multiset match).
        bs.retain(|s| {
            if let Some(pos) = hs.iter().position(|x| x == s) {
                hs.remove(pos);
                false
            } else {
                true
            }
        });
        let paired = bs.len().min(hs.len());
        for i in 0..paired {
            out.push(change("modified", &name, &bs[i], &hs[i]));
        }
        for s in &bs[paired..] {
            out.push(change("removed", &name, s, ""));
        }
        for s in &hs[paired..] {
            out.push(change("added", &name, "", s));
        }
    }
    out
}

fn change(kind: &str, name: &str, before: &str, after: &str) -> SignatureChange {
    SignatureChange {
        file: String::new(), // filled by the caller
        lang: String::new(), // filled by the caller
        kind: kind.into(),
        name: name.into(),
        before: before.into(),
        after: after.into(),
    }
}

fn go_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Top-level Go funcs anchor at column 0; optional `(recv)` receiver group.
        regex::Regex::new(r"(?m)^func\s+(?:\((?P<recv>[^)]*)\)\s*)?(?P<name>[A-Za-z_]\w*)")
            .expect("valid go regex")
    })
}

fn rust_re() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // `fn` items at any indent (methods live inside `impl`); skip the modifiers.
        regex::Regex::new(
            r#"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:async\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+"[^"]*"\s+)?fn\s+(?P<name>[A-Za-z_]\w*)"#,
        )
        .expect("valid rust regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_go_signature_modified() {
        let base = "package p\n\nfunc Foo(a int) error {\n\treturn nil\n}\n";
        let head = "package p\n\nfunc Foo(a, b int) error {\n\treturn nil\n}\n";
        let changes = diff_sigs(extract(base, "go"), extract(head, "go"));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, "modified");
        assert_eq!(changes[0].name, "Foo");
        assert_eq!(changes[0].before, "func Foo(a int) error");
        assert_eq!(changes[0].after, "func Foo(a, b int) error");
    }

    #[test]
    fn positive_go_method_receiver_disambiguates() {
        // Two methods named `Run` on different receivers — must not conflate.
        let base = "package p\nfunc (a *A) Run() {}\nfunc (b *B) Run() {}\n";
        let head = "package p\nfunc (a *A) Run(x int) {}\nfunc (b *B) Run() {}\n";
        let changes = diff_sigs(extract(base, "go"), extract(head, "go"));
        assert_eq!(changes.len(), 1, "only A.Run changed");
        assert_eq!(changes[0].kind, "modified");
        assert_eq!(changes[0].after, "func (a *A) Run(x int)");
    }

    #[test]
    fn positive_rust_added_and_removed() {
        let base = "pub fn keep() {}\nfn gone(x: u32) -> bool { true }\n";
        let head = "pub fn keep() {}\npub async fn fresh(y: &str) -> Result<()> { Ok(()) }\n";
        let mut changes = diff_sigs(extract(base, "rust"), extract(head, "rust"));
        changes.sort_by_key(|c| c.kind.clone());
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].kind, "added");
        assert_eq!(changes[0].name, "fresh");
        assert!(changes[0]
            .after
            .contains("async fn fresh(y: &str) -> Result<()>"));
        assert_eq!(changes[1].kind, "removed");
        assert_eq!(changes[1].name, "gone");
    }

    #[test]
    fn corner_rust_multiline_signature_normalized_to_one_line() {
        let base = "fn f(a: u32) {}\n";
        let head = "fn f(\n    a: u32,\n    b: u32,\n) -> u32 {\n    a\n}\n";
        let changes = diff_sigs(extract(base, "rust"), extract(head, "rust"));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, "modified");
        assert_eq!(changes[0].after, "fn f( a: u32, b: u32, ) -> u32");
        assert!(!changes[0].after.contains('\n'));
    }

    #[test]
    fn corner_identical_files_yield_no_changes() {
        let src = "package p\nfunc A() {}\nfunc B() int { return 1 }\n";
        assert!(diff_sigs(extract(src, "go"), extract(src, "go")).is_empty());
    }

    #[test]
    fn boundary_empty_side_is_all_added_or_removed() {
        let head = "package p\nfunc New() {}\n";
        let added = diff_sigs(extract("", "go"), extract(head, "go"));
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].kind, "added");
        let removed = diff_sigs(extract(head, "go"), extract("", "go"));
        assert_eq!(removed[0].kind, "removed");
    }

    #[test]
    fn adversarial_hostile_long_signature_is_bounded() {
        let big = "a: u32, ".repeat(10_000);
        let head = format!("fn f({big}) {{}}\n");
        let sigs = extract(&head, "rust");
        assert_eq!(sigs.len(), 1);
        assert!(
            sigs[0].sig.chars().count() <= MAX_SIG + 20,
            "signature not bounded"
        );
    }

    #[test]
    fn adversarial_garbage_input_never_panics() {
        // Unbalanced parens, stray keywords, binary-ish bytes — just no signatures.
        for junk in ["func func func", "fn fn ( ) ) )", "\0\0func\0", "))((", ""] {
            let _ = diff_sigs(extract(junk, "go"), extract(junk, "rust"));
        }
    }
}
