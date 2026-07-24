//! `SummaryCollector` — the one **soft** collector (component 08). While the
//! deterministic analyzers run, it fans cheap-LLM jobs over the [`LlmPool`] to
//! summarize the **changed functions** (before/after source from the diff) into
//! short prose. This is the human-legible half of the bundle and the place the
//! cheap-heavy pool economics pay off — we summarize everything the diff touched.
//!
//! The model only ever *summarizes* facts we already grounded; its output is
//! **soft**, clearly labelled, bounded, and never overwrites a hard fact. Fail-soft:
//! no pool / no healthy member / a dead job yields fewer (or zero) summaries and a
//! recorded count, never a blocked bundle. Inputs are capped (a hostile 1 MB
//! "function" can't blow the context); the job count is capped with a recorded
//! `omitted`.

use crate::collector::{CollectCtx, CollectorOutput, FactCollector, FactFragment};
use crate::util::{bound, lang_of};
use agent_core::{CompletionRequest, FunctionSummary, LlmPool, Message, Revision, SummaryReport};
use futures_util::future::join_all;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Most changed functions summarized (top-N; the rest are a recorded `omitted`).
const MAX_JOBS: usize = 20;
/// Per-side source cap sent to a model.
const MAX_SRC: usize = 1500;
/// Summary prose cap (untrusted model output).
const MAX_SUMMARY: usize = 300;

pub(crate) struct SummaryCollector {
    pub pool: Option<Arc<dyn LlmPool>>,
}

/// A unit of summarization work: one changed function's before/after source.
struct Job {
    name: String,
    file: String,
    kind: &'static str,
    before: String,
    after: String,
}

#[async_trait::async_trait]
impl FactCollector for SummaryCollector {
    fn name(&self) -> &'static str {
        "summaries"
    }

    async fn collect(&self, ctx: &CollectCtx) -> CollectorOutput {
        let Some(pool) = self.pool.clone() else {
            return CollectorOutput::skipped("no pool configured");
        };
        // Fail-soft: don't spend requests on a dead pool.
        if !pool.health().await.members.iter().any(|m| m.alive) {
            return CollectorOutput::skipped("no healthy pool member");
        }

        let mut jobs = self.build_jobs(ctx).await;
        if jobs.is_empty() {
            return CollectorOutput::skipped("no changed functions to summarize");
        }
        let requested = jobs.len() as u32;
        let omitted = jobs.len().saturating_sub(MAX_JOBS) as u32;
        jobs.truncate(MAX_JOBS);

        // Fan out concurrently — the pool is parallel; each job is independent.
        let results = join_all(jobs.into_iter().map(|j| summarize(pool.clone(), j))).await;
        let summaries: Vec<FunctionSummary> = results.into_iter().flatten().collect();

        CollectorOutput::ok(FactFragment::Summaries {
            report: SummaryReport {
                produced: summaries.len() as u32,
                requested,
                omitted,
                summaries,
            },
        })
    }
}

impl SummaryCollector {
    /// Build one job per changed function (added or body-modified) across the
    /// changed Go/Rust files, reading before/after source from the base/head blobs.
    async fn build_jobs(&self, ctx: &CollectCtx) -> Vec<Job> {
        let Ok(diff) = ctx.repo.diff(&ctx.base, &ctx.head, &[]).await else {
            return Vec::new();
        };
        let mut jobs = Vec::new();
        for f in diff.files {
            let path = match f.new_path.as_ref().or(f.old_path.as_ref()) {
                Some(p) => p.clone(),
                None => continue,
            };
            let lang = lang_of(&path);
            if lang != "go" && lang != "rust" {
                continue;
            }
            let base = read_bodies(
                ctx,
                &ctx.base,
                f.old_path.as_deref().or(f.new_path.as_deref()),
                &lang,
            )
            .await;
            let head = read_bodies(
                ctx,
                &ctx.head,
                f.new_path.as_deref().or(f.old_path.as_deref()),
                &lang,
            )
            .await;
            let file = path.to_string_lossy().into_owned();
            for (name, after) in &head {
                match base.get(name) {
                    Some(before) if before == after => {} // unchanged — skip
                    Some(before) => jobs.push(Job {
                        name: name.clone(),
                        file: file.clone(),
                        kind: "modified",
                        before: bound(before, MAX_SRC),
                        after: bound(after, MAX_SRC),
                    }),
                    None => jobs.push(Job {
                        name: name.clone(),
                        file: file.clone(),
                        kind: "added",
                        before: String::new(),
                        after: bound(after, MAX_SRC),
                    }),
                }
            }
        }
        jobs
    }
}

/// Read a revision's blob and extract each top-level function's full body source,
/// keyed by (receiver-qualified) name. Empty for the absent side / a binary blob.
async fn read_bodies(
    ctx: &CollectCtx,
    rev: &Revision,
    path: Option<&Path>,
    lang: &str,
) -> BTreeMap<String, String> {
    let Some(p) = path else {
        return BTreeMap::new();
    };
    let text = match ctx.repo.read_file(rev, p).await {
        Ok(b) if !b.is_binary => b.text,
        _ => return BTreeMap::new(),
    };
    function_bodies(&text, lang)
}

/// Run one summarization job through the pool. `None` on any failure (fail-soft).
async fn summarize(pool: Arc<dyn LlmPool>, job: Job) -> Option<FunctionSummary> {
    let sys = "You summarize code changes for a reviewer. Reply with ONE short, factual sentence describing what the change does. No preamble, no markdown, no advice, no code.";
    let user = if job.kind == "added" {
        format!(
            "A new function `{}` was added to {}:\n\n{}\n\nWhat does it do, in one sentence?",
            job.name, job.file, job.after
        )
    } else {
        format!(
            "The function `{}` in {} was modified.\n\n--- before ---\n{}\n\n--- after ---\n{}\n\nWhat changed, in one sentence?",
            job.name, job.file, job.before, job.after
        )
    };
    let req = CompletionRequest {
        messages: vec![Message::system(sys), Message::user(user)],
        max_tokens: 200,
        temperature: 0.0,
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let resp = pool.complete(req).await.ok()?;
    let text = resp.message.content_text();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    // `complete` fails over internally, so the specific member isn't exposed — record
    // the pool's name. (Per-member attribution would use `complete_all`.)
    Some(FunctionSummary {
        name: job.name,
        file: job.file,
        kind: job.kind.into(),
        summary: bound(text, MAX_SUMMARY),
        model: bound(pool.name(), 80),
        duration_ms: started.elapsed().as_millis().min(u32::MAX as u128) as u32,
    })
}

/// Extract each top-level function's full source (decl through the brace-balanced
/// body), keyed by name. Go methods are keyed by receiver type (like the signature
/// collector) so same-named methods don't collide. Best-effort, bounded.
fn function_bodies(text: &str, lang: &str) -> BTreeMap<String, String> {
    let re = if lang == "go" {
        crate::signatures::go_re()
    } else {
        crate::signatures::rust_re()
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(cap) = re.captures(lines[i]) {
            let name = cap
                .name("name")
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let key = match cap.name("recv") {
                Some(r) => format!("{}.{name}", recv_type(r.as_str())),
                None => name,
            };
            // Brace-balance from here to the closing brace.
            let (span_end, opened) = brace_span(&lines, i);
            if opened && !key.is_empty() {
                let body = lines[i..=span_end].join("\n");
                out.entry(key).or_insert(body);
                i = span_end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Walk from `start` to the line where the brace depth returns to zero. Returns
/// `(end_line, balanced)`; `balanced == false` when the body never closes (EOF or
/// the runaway guard) — the caller then drops it rather than capturing a fragment.
fn brace_span(lines: &[&str], start: usize) -> (usize, bool) {
    let mut depth = 0i32;
    let mut opened = false;
    let mut j = start;
    while j < lines.len() {
        for ch in lines[j].chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return (j, true);
        }
        j += 1;
        if j - start > 5000 {
            break;
        }
    }
    (start, false) // never balanced
}

fn recv_type(recv: &str) -> String {
    recv.split_whitespace()
        .last()
        .unwrap_or("")
        .trim_start_matches('*')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_extracts_function_bodies_go() {
        let src = "package p\n\nfunc Foo() int {\n\treturn 1\n}\n\nfunc (s *S) Bar() {\n\tx()\n}\n";
        let b = function_bodies(src, "go");
        assert_eq!(b.len(), 2);
        assert!(b.contains_key("Foo"));
        assert!(b.contains_key("S.Bar"), "method keyed by receiver type");
        assert!(b["Foo"].contains("return 1"));
    }

    #[test]
    fn positive_extracts_rust_bodies() {
        let src = "pub fn a() -> u32 {\n    1\n}\nfn b() {}\n";
        let b = function_bodies(src, "rust");
        assert_eq!(b.len(), 2);
        assert!(b["a"].contains("-> u32"));
    }

    #[test]
    fn corner_unbalanced_braces_do_not_hang_or_capture() {
        let src = "func Broken() {\n\tif x {\n"; // never closes
        let b = function_bodies(src, "go");
        assert!(b.is_empty(), "an unterminated body is not captured");
    }

    #[test]
    fn boundary_empty_text_yields_no_bodies() {
        assert!(function_bodies("", "go").is_empty());
        assert!(function_bodies("// just a comment\n", "rust").is_empty());
    }
}
