# Parity spec 49 — fuzzy file-search / `@`-mention completion

Per-feature parity spec for a new **`FileSearch` seam**: a fast, typo-tolerant,
**ranked** filename matcher that turns a partial or misspelled query into a small,
ordered list of candidate paths — the engine behind `@`-mention *completion*.
Where spec [17](17-reference-resolution.md) *resolves* an already-precise
`@file:src/lib.rs` into bytes, this seam is what lets a user type `@lbi` (or
`@serch`) and be offered `src/lib.rs` / `crates/agent-search/…` in the first place,
ranked best-first, before any resolution happens.

> **Status: ⬜ spec written, not started.** Proposed new **`FileSearch` seam**
> (`search(query, opts) -> Vec<FileHit>`, ranked best-first) in `agent-core`, a
> sibling `agent-filesearch` crate behind a `filesearch-walk` feature, a
> `[filesearch]` config key, one `register_builtins` factory line, a `MeteredFileSearch`
> decorator, and a **deterministic iai bench on the ranking hot path**. The
> differentiator is that the fuzzy matcher is an **inspectable, swappable seam** —
> not a TUI-private helper — with an **Ir-ceilinged bench** on the pure
> score-and-sort step and a **metered query/candidate count**, that **routes into
> spec [17](17-reference-resolution.md)** (a `@file` that misses exact resolution
> falls through to a ranked suggestion) and can be **backed by the spec
> [15](15-semantic-search.md) tantivy index** (`SearchBackend::list_files` as the
> candidate source) instead of always re-walking the tree. It reuses the spec
> [05](05-text-search.md) `ignore`-crate gitignore/hidden conventions so what the
> matcher *can't* surface stays a safety property, not just ergonomics.
> **Deferred:** the `filesearch.proto` gRPC `--serve-filesearch` service (the
> `search` shape is already unary-clean for it); an index-backed candidate source
> (`= "index"`) that ranks over `list_files` rather than a live walk; frecency /
> recency weighting (opencode's fff biases by access frequency — a follow-up, not
> a first cut); and streaming/debounced *session* updates as the query grows
> keystroke-by-keystroke (codex's `FileSearchSession` — the batch `search` is the
> right first shape). **Unimplemented** — the `FileSearch` trait, its impl, the
> config/registry wiring, and the bench do not exist yet; this is the design of
> record.

## Feature & why it matters

`@`-mentions are the cheapest pointer a user has (`explain @src/lib.rs:40-80`) —
but only if they can *produce* the path. Spec [17](17-reference-resolution.md)
assumes the path is already (near-)exact: `LocalResolver` parses `@file:<path>`,
`confine()`s it to the workspace, and reads the bytes. It has **no tolerance for a
partial or wrong name** — `@lib` doesn't become `src/lib.rs`, `@serch` finds
nothing, and a user who doesn't remember the exact path is back to running a `find`
tool call (a wasted turn) or guessing.

A fuzzy file-search closes exactly that gap. Given a short, possibly-misspelled
query it returns a **ranked** shortlist of real paths:

- **Typo / subsequence tolerance.** `@atcmplt` should still surface
  `autocomplete.ts`; the query characters need only appear *in order*, not
  contiguously (pi's `fuzzyMatch`, codex's `nucleo` fuzzy atom).
- **Ranking is the whole point.** A flat "these 400 files contain those letters"
  list is useless; the value is the **order** — exact filename first, then a
  prefix hit, then a scattered subsequence, with word-boundary and
  consecutive-run bonuses so `fb` prefers `foo-bar` over `afbx` (pi's word-boundary
  scoring; codex's nucleo score). The top ~20 are all the completion UI shows.
- **Bounded and gitignore-aware.** The candidate set is the tracked tree — `.git/`,
  `node_modules/`, and `.gitignore`d paths are skipped (the spec [05](05-text-search.md)
  `ignore`-crate walk), both so the shortlist is relevant and because a hidden
  cache dir is a prompt-injection surface (hermes' `#1558`). Results are capped;
  a query can never return the whole tree.

The unit of work is **query → ranked candidates**, a pure and cheap operation once
the candidate list exists — which is precisely why it belongs behind a seam with a
**deterministic CPU bench** on the score-and-sort step, and why it slots *in front
of* spec 17 rather than duplicating it.

## agent-seddon today

**No fuzzy, ranked filename matcher exists.** The pieces it would compose with are
all real and wired; the matcher itself is the missing middle.

- **`@`-resolution assumes an exact path.**
  [`crates/agent-reference/src/resolver.rs`](../../crates/agent-reference/src/resolver.rs)
  `LocalResolver` parses `@file`/`@dir`/`@symbol`/`@url`, `confined()`s each target
  through [`agent_core::confine`](../../crates/agent-core/src/security.rs) (canonicalizing —
  symlink-escape and `..` both refused), applies a sensitive-path guard
  (`is_sensitive`) and `scan_for_injection`, routes `@symbol` → `SearchBackend` and
  `@url` → `WebBackend`. But `confined("lib")` on a non-existent `lib` just
  *fails* — there is no fuzzy fallback that says "did you mean `src/lib.rs`?".
  Spec [17](17-reference-resolution.md).
- **Exact/regex tree tools, no ranking.** `grep`/`find`/`ls`
  ([`crates/agent-tools/src/search.rs`](../../crates/agent-tools/src/search.rs))
  walk the tree with ripgrep's [`ignore`](https://docs.rs/ignore) crate — `find` is
  **regex-over-relative-path** and returns matches in **walk order**, unranked and
  uncapped-by-relevance (only `MAX_HITS`). It answers "which paths match this
  regex", never "which paths *best* match this fuzzy query". Spec [05](05-text-search.md).
- **A ready candidate source exists.** The tantivy `SearchBackend`
  ([`crates/agent-search`](../../crates/agent-search), spec [15](15-semantic-search.md))
  exposes [`list_files(&globs)`](../../crates/agent-core/src/lib.rs) — "the
  index-backed alternative to walking the tree", sorted and de-duplicated. A
  FileSearch backend could rank over *that* enumeration instead of re-walking,
  reusing the already-maintained index.
- **Reusable seam scaffolding.** The plugin registry
  ([`register_builtins`](../../crates/agent-runtime/src/registry.rs), which already
  wires `SearchBackend` factories like `"tantivy"`), the `Metered*` decorator
  pattern ([`metered.rs`](../../crates/agent-runtime/src/metered.rs)), and
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs) counters/histograms are
  all directly reusable.

Honest gap: there is **no scorer, no ranking, no fuzzy/subsequence match, no
candidate-source abstraction, and no completion entry point into the resolver**.
`find`'s regex-over-path is the closest thing and it is neither fuzzy nor ranked.
The whole matcher is greenfield.

## Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| codex | `codex-rs/file-search/src/lib.rs` (`run`/`create_session`, `nucleo`-backed fuzzy match over an `ignore` walk; `FileMatch{score,path,match_type,indices}`, `cmp_by_score_desc_then_path_asc`, `FileSearchOptions{limit,exclude,respect_gitignore}`), `.../src/cli.rs` | `codex-rs/file-search/src/lib.rs` `#[cfg(test)] mod tests` (~18 `#[test]`, `tempfile` + `pretty_assertions`) | cargo `#[test]` |
| opencode | `packages/core/src/filesystem/fff.{node,bun}.ts` (native "fast file finder": `fileSearch`/`directorySearch`/`mixedSearch`, frecency + fuzzy ranked, `scores.total`, `totalMatched`), consumed by TUI `packages/tui/src/component/prompt/autocomplete.tsx` (trusts fff order for `@`-files; `fuzzysort` for non-file items) | — (the ranked fuzzy path is a native module + the `fuzzysort` library; `packages/core/test/filesystem/search.test.ts` covers ripgrep/glob (spec 05), **not** fff ranking) | bun:test |
| pi | `packages/tui/src/fuzzy.ts` (`fuzzyMatch`/`fuzzyFilter`: in-order subsequence + word-boundary/consecutive/gap scoring, lower = better), `packages/tui/src/autocomplete.ts` (`scoreEntry` exact>prefix>substring>path-substring, `getFuzzyFileSuggestions` via `fd`, `.slice(0,20)`) | `packages/tui/test/fuzzy.test.ts`, `packages/tui/test/autocomplete.test.ts` (`fd @ file suggestions`) | node:test |
| hermes | — (no fuzzy filename matcher / `@`-mention completion; `@file:` refs resolve **near-exact** paths in `agent/context_references.py`, spec 17. "fuzzy" in hermes is `tools/fuzzy_match.py`, an edit/patch *text-replacement* matcher — unrelated to filename search) | — | — |

**codex is the anchor** — a dedicated `codex-file-search` crate whose *only* job is
ranked fuzzy filename matching feeding `@`-mention completion, and it pins exactly
the shape this seam wants:

- **Ranked `FileMatch`es over an `ignore` walk.** `run(pattern, roots, options)`
  feeds every walked path into `nucleo` (`Config::DEFAULT.match_paths()`), then
  returns the top `limit` `FileMatch{score, path, match_type, indices}` — files
  **and** directories (`MatchType::File`/`Directory`), with optional match-index
  highlighting. Test: `run_returns_matches_for_query`,
  `run_returns_directory_matches_for_query`.
- **Deterministic ordering + tie-break.** `cmp_by_score_desc_then_path_asc` sorts
  by descending score, then **ascending path** on ties — a pure, testable
  comparator. Test: `tie_breakers_sort_by_path_when_scores_equal`.
- **Non-match is scoreless, not zero-scored.** `pattern.score(...) == None` for a
  query whose chars aren't a subsequence. Test:
  `verify_score_is_none_for_non_match`.
- **Bounded result set.** `FileSearchOptions.limit` is a `NonZero<usize>`
  (default 20); the walk is capped and cancellable (`cancel_flag`,
  `cancel_exits_run`).
- **Gitignore semantics, deliberately git-scoped.** `respect_gitignore` toggles the
  walker; crucially `require_git(true)` means `.gitignore` applies **only inside a
  git repo**, so a broad parent `~/.gitignore` can't silently hide a non-repo
  tree. Tests: `parent_gitignore_outside_repo_does_not_hide_repo_files`,
  `git_repo_still_respects_local_gitignore_when_enabled`. (This mirrors the spec 05
  "fixture must be a git repo" correction.)
- **Basename helper.** `file_name_from_path` for name-vs-path scoring. Tests:
  `file_name_from_path_uses_basename`, `..._falls_back_to_full_path`.

**pi is the second anchor, and its ranking logic is the most directly portable.**
`fuzzyMatch(query, text)` is a pure subsequence scorer: chars must appear **in
order**, with a **word-boundary bonus** (`-10` after `\s\-_./:`), a
**consecutive-run bonus**, a **gap penalty**, and a big bonus for an exact-equal
string — `matches:false` if any query char is left over. `scoreEntry` in
`autocomplete.ts` is a coarser tier ladder (exact filename `100` > prefix `80` >
filename-substring `50` > path-substring `30`, `+10` for directories). Its tests
pin the properties this seam must reproduce: `characters must appear in order`
(and `cba` does *not* match `abc`), `consecutive matches score better than
scattered`, `word boundary matches score better`, `prioritizes exact matches over
longer prefix matches` (`["cl","clone"]` for query `cl`), and the `fd @ file
suggestions` table (`matches file with extension in query`, `filters are case
insensitive`, `ranks directories before files`, `returns nested file paths`,
`scopes fuzzy search to relative directories`).

**opencode** ships the richest *product* — a native `fff` finder that blends
frecency with fuzzy ranking (`fileSearch`/`mixedSearch` returning `scores.total`),
which the TUI trusts verbatim for `@`-file completion (`fuzzysort` handles only the
non-file items). But the ranking itself lives in a native module / a third-party
library, so there is **no unit test of the fuzzy scoring in the clone** — it is a
data point on *shape* (frecency + fuzzy, files+dirs mixed, ranked shortlist), not
on a portable algorithm. Its frecency bias is the explicit **Deferred** follow-up.

**hermes has no fuzzy filename surface at all** — marked "—". Its `@file:` handling
resolves near-exact paths (spec 17), and its only "fuzzy" code
(`tools/fuzzy_match.py`) matches *text to replace inside a file* for the edit/patch
tools, which is a different feature. This is where agent-seddon can leapfrog: codex
has the crate but no distribution/observability story; pi has the algorithm but
TUI-private; opencode has the product but a closed ranker — **none exposes fuzzy
file-search as a metered, benched, swappable seam that routes into `@`-resolution.**

## Completeness gaps

Behaviour agent-seddon must add to be the most complete (spec only — do **not**
implement here). Each maps to a test case below.

- **`FileSearch` seam.** New async trait in `agent-core`:
  `search(query: &str, opts: &FileSearchOpts) -> Result<Vec<FileHit>>`, where
  `FileHit{path, score, kind: File|Dir, indices: Option<Vec<u32>>}` and
  `FileSearchOpts{limit, respect_gitignore, kind_filter}`. Impl in a sibling
  `agent-filesearch` crate behind a `filesearch-walk` feature; one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs); config-selected
  via `[filesearch] backend = "walk"`. *(spec only — do **not** implement here)*
- **Ranking: exact > prefix > subsequence, with tie-break.** A pure scorer that
  ranks an exact basename above a prefix hit above a scattered subsequence, with
  word-boundary/consecutive bonuses (port pi `fuzzyMatch` + `scoreEntry`), and a
  **deterministic tie-break by ascending path** on equal score (port codex
  `cmp_by_score_desc_then_path_asc`). A non-subsequence query scores `None`/drops
  (port codex `verify_score_is_none_for_non_match`). This pure function is the
  **iai bench hot path**. *(spec only — do **not** implement here)*
- **Candidate source is pluggable (walk *or* index).** The first cut walks the tree
  with the spec 05 `ignore` crate (gitignore + hidden semantics inherited, git-scoped
  like codex's `require_git(true)`); a **deferred** `= "index"` backend ranks over
  [`SearchBackend::list_files`](../../crates/agent-core/src/lib.rs) (spec 15) so the
  candidate list is the already-maintained index, not a fresh walk. The seam is the
  same either way. *(spec only — do **not** implement here)*
- **Gitignore / hidden handling (reuse spec 05).** `.git/`, `node_modules/`, and
  `.gitignore`d paths never appear in the shortlist — the spec 05 walker default,
  which is also the hermes `#1558` injection boundary. `respect_gitignore=false`
  opts out (codex `FileSearchOptions`). *(spec only — do **not** implement here)*
- **Bounded result cap.** `opts.limit` (default ~20, a `NonZero`) caps the returned
  shortlist regardless of how many candidates match; the ranker keeps the top-N,
  the rest are dropped (port codex `limit`, pi `.slice(0,20)`). A cap is not
  optional — a fuzzy `@a` on a large tree matches almost everything. *(spec only —
  do **not** implement here)*
- **`@`-completion route into spec 17.** A `@file` that **misses exact resolution**
  in `LocalResolver` (`confined()` fails / path absent) falls through to
  `FileSearch.search(name)` and the resolver attaches a `did you mean …` suggestion
  (top hit) or resolves the unambiguous single hit — the seam is what makes
  spec 17 *tolerant*. `FileSearch` holds no `Arc` to the resolver; the resolver
  optionally holds an `Arc<dyn FileSearch>`. *(spec only — do **not** implement here)*
- **Confinement of the query (untrusted input).** The query is attacker-controlled
  (spec 17's model is untrusted). A *scoped* query like pi's `@dir/prefix` splits
  into a base dir + fuzzy tail — that **base dir must go through
  [`confine`](../../crates/agent-core/src/security.rs)** (no `../` escape, no symlink
  escape, sensitive paths refused); a query carrying a **NUL byte** or path
  separators that would break out is rejected, and a **huge query** (megabytes) is
  **capped** before scoring so a hostile prompt can't DoS the scorer. *(spec only —
  do **not** implement here)*
- **Metered + benched (differentiator).** `filesearch_queries_total`,
  `filesearch_candidates_scored` (how big the ranked set was), and a
  `filesearch_query_seconds` histogram in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs); a `MeteredFileSearch`
  decorator ([`metered.rs`](../../crates/agent-runtime/src/metered.rs)); and an
  **iai-callgrind bench with an Ir ceiling** on the pure score-and-sort step
  (deterministic CPU). *(spec only — do **not** implement here — no peer has a
  metered/benched fuzzy matcher.)*

## Table-driven test plan

New `#[rstest]` tables in the `agent-filesearch` crate: a **pure-ranking** table
(fully deterministic, no I/O — the CPU hot path the bench guards) and a
**search-over-fixture** table driven by a git-repo `tempdir()` from
[`agent-testkit`](../../crates/agent-testkit/src/lib.rs). As in spec 05 the fixture
**must be a git repo** (`git init`) for `.gitignore` to apply, and hidden dirs are
skipped by the walker default. Prefixes: `positive_` ranks/returns cleanly,
`negative_` rejects/drops, `corner_` odd-but-valid, `boundary_` at a limit;
`adversarial_` (untrusted-input) cases are **mandatory** and assert the rejection.
`(port: <peer>)` marks a case mined from a peer test; `(new: agent-seddon)` are
ours.

```rust
// ---- pure ranking: exact > prefix > subsequence, deterministic tie-break ----
// score_and_rank(query, candidates) -> Vec<FileHit> (best-first). No I/O.
#[rstest]
#[case::positive_exact_basename_ranks_first(
    "lib.rs",
    vec!["src/lib.rs", "src/librarian.rs", "crates/lib_util.rs"],
    /*top=*/ "src/lib.rs")]                                                  // (port: codex run_returns_matches / pi scoreEntry exact=100)
#[case::positive_prefix_beats_subsequence(
    "auto",
    vec!["src/autocomplete.ts", "src/a_u_t_o.ts"],
    /*top=*/ "src/autocomplete.ts")]                                         // (port: pi "prioritizes exact matches over longer prefix")
#[case::positive_subsequence_typo_tolerant(
    "atcmplt",
    vec!["src/autocomplete.ts", "README.md"],
    /*top=*/ "src/autocomplete.ts")]                                         // (port: pi "characters must appear in order")
#[case::corner_word_boundary_beats_mid_token(
    "fb",
    vec!["foo-bar.rs", "affbx.rs"],
    /*top=*/ "foo-bar.rs")]                                                  // (port: pi "word boundary matches score better")
#[case::corner_consecutive_beats_scattered(
    "foo",
    vec!["foobar.rs", "f_o_o_bar.rs"],
    /*top=*/ "foobar.rs")]                                                   // (port: pi "consecutive matches score better")
#[case::boundary_tie_breaks_by_ascending_path(
    "path",                              // equal score on two files
    vec!["b_path.rs", "a_path.rs"],
    /*top=*/ "a_path.rs")]                                                   // (port: codex tie_breakers_sort_by_path_when_scores_equal)
#[case::negative_out_of_order_not_a_subsequence(
    "cba",
    vec!["abc.rs"],
    /*top=*/ "")] /* no hit: query chars not in order */                     // (port: pi "cba does not match abc")
fn rank_cases(#[case] query: &str, #[case] candidates: Vec<&str>, #[case] top: &str) {
    // score_and_rank(query, candidates); assert first hit's path == top
    // (or empty when top == ""). Pure, deterministic — this is the bench input.
}

// ---- search over a git-repo fixture: gitignore + hidden + dirs + cap --------
// fixture_repo(): git init; src/lib.rs, src/librarian.rs, docs/guide.md,
//   build.out (gitignored via .gitignore "build.out\n"), .secret/h.rs (hidden),
//   plus N padding files for the cap case.
#[rstest]
#[case::positive_ranked_files_and_dirs(
    "lib", vec!["src/lib.rs"], vec![])]                                      // (port: codex run_returns_directory_matches / pi fd suggestions)
#[case::positive_case_insensitive(
    "LIB", vec!["src/lib.rs"], vec![])]                                      // (port: pi "filters are case insensitive")
#[case::negative_gitignored_not_a_candidate(
    "build", vec![], vec!["build.out"])]                                    // (port: codex git_repo_still_respects_local_gitignore; spec 05)
#[case::negative_hidden_dir_not_a_candidate(
    "h.rs", vec![], vec![".secret"])]                                       // (port: hermes #1558 via spec 05 walker default)
#[tokio::test]
async fn search_fixture_cases(
    #[case] query: &str,
    #[case] present: Vec<&str>,   // substrings that MUST appear in the shortlist
    #[case] absent: Vec<&str>,    // substrings that MUST NOT appear
) { /* build WalkFileSearch over fixture_repo(); search(query, default opts);
       assert present ⊆ paths, absent ∩ paths == ∅. */ }

// ---- bounded result cap ------------------------------------------------------
#[tokio::test]                                                                // (port: codex FileSearchOptions.limit / pi .slice(0,20))
async fn boundary_result_set_capped_at_limit() {
    // fixture with 500 files all matching a broad fuzzy query "a";
    // search("a", opts{limit:20}); assert hits.len() == 20 and they are the
    // top-scored 20 (not an arbitrary walk-order slice).
}

// ---- no-match returns an empty, non-error shortlist -------------------------
#[tokio::test]                                                                // (port: codex session_emits_complete_when_query_changes_with_no_matches)
async fn corner_no_match_returns_empty_not_error() {
    // search("zzzznotathing", …) over fixture_repo() -> Ok(vec![]) (empty),
    // never an Err; filesearch_queries_total still increments.
}

// ---- adversarial: query is attacker-controlled (untrusted input) ------------
#[rstest]
#[case::adversarial_traversal_scoped_query_confined(
    "../../etc/passwd/foo", Reject::Confined)]                              // (new: agent-seddon; cf. spec 17 confine)
#[case::adversarial_nul_byte_rejected(
    "src/li\0b.rs", Reject::Rejected)]                                      // (new: agent-seddon)
#[case::adversarial_huge_query_capped(
    /*"a" * 5_000_000*/ "<<HUGE>>", Reject::Capped)]                        // (new: agent-seddon; DoS cap)
#[tokio::test]
async fn adversarial_query_cases(#[case] query: &str, #[case] expect: Reject) {
    // Confined: a scoped "@dir/tail" whose base escapes the workspace is refused
    //   by confine() before any walk (no results outside root, no panic).
    // Rejected: a NUL/separator-injecting query is rejected pre-scoring.
    // Capped: a multi-megabyte query is truncated to MAX_QUERY_LEN before
    //   scoring so the scorer can't be DoS'd; result is Ok (bounded), not a hang.
}

// ---- @-completion route: resolver falls through to FileSearch (spec 17) -----
#[tokio::test]                                                                // (new: agent-seddon — the differentiator route)
async fn positive_missing_exact_ref_falls_through_to_suggestion() {
    // LocalResolver with an Arc<dyn FileSearch>; prompt "@file:lb".
    // confined("lb") misses (no such path) -> resolver calls search("lb") ->
    // attaches a "did you mean src/lib.rs" suggestion (top hit). The turn still
    // runs; a genuinely absent query yields graceful passthrough (spec 17).
}
```

Case-prefix key (repo convention): `positive_` expected success/order,
`negative_` expected drop/reject, `corner_` odd-but-valid (word-boundary,
no-match), `boundary_` at a limit (cap, tie-break), `adversarial_` untrusted-input
(traversal/NUL/huge — **mandatory**, must assert the rejection). `(port: <peer>)`
names the peer test a case was mined from; `(new: agent-seddon)` marks the
confinement, DoS-cap, metered-count, and resolver-route cases that have **no peer
analogue** (no peer confines the query, caps it, meters it, or wires the matcher
back into `@`-resolution).

## Harness obligations

The implementing PR must satisfy all (follows the #21–45 pattern):

- **Seam + registry.** `FileSearch` trait in
  [`agent-core`](../../crates/agent-core/src/lib.rs); impl in a new
  `agent-filesearch` crate behind a `filesearch-walk` feature (an `ignore`-crate
  walk + a pure scorer; `nucleo`-class or an in-house subsequence scorer ported
  from pi); one factory line in
  [`register_builtins`](../../crates/agent-runtime/src/registry.rs) keyed
  `"walk"`, config-selected via a `[filesearch]` block; a `MeteredFileSearch` in
  [`metered.rs`](../../crates/agent-runtime/src/metered.rs); doc in
  `docs/components/filesearch.md`.
- **Resolver route (spec 17).** Extend
  [`crates/agent-reference/src/resolver.rs`](../../crates/agent-reference/src/resolver.rs)
  `LocalResolver` to hold an optional `Arc<dyn FileSearch>` and fall through to a
  ranked suggestion when `confined()` misses — wired in
  [`builder.rs`](../../crates/agent-runtime/src/builder.rs) (like the other
  `Arc`-handle seams), **not** a bare factory. Keep the confine + sensitive-path +
  injection guards in front of any resolution the suggestion triggers.
- **Metrics.** `filesearch_queries_total`, `filesearch_candidates_scored`, and a
  `filesearch_query_seconds` histogram in
  [`agent-metrics`](../../crates/agent-metrics/src/lib.rs), surfaced through the
  metered decorator — the metered-matcher differentiator.
- **Bench (the point of the seam).** An **iai-callgrind bench** on the pure
  `score_and_rank(query, candidates)` step over a fixed candidate list (the
  `rank_cases` inputs scaled up), with an **absolute Ir ceiling** in
  [`nix/checks/bench.nix`](../../nix/checks/bench.nix). The score-and-sort is a
  deterministic CPU hot path — unlike spec 05's I/O-bound walk, this *is* a
  benchable inner loop. (The walk/candidate-gathering half is I/O-bound; document
  that split, benching only the ranker.)
- **Leak.** A dhat `tests/leak.rs` (`dhat-heap` feature) over
  **search → rank → collect top-N**, asserting the hot path frees its candidate
  buffer and stays under an allocation budget when the candidate set is large and
  the `limit` small (the drop-the-rest path).
- **Deferred (record, don't build).** `filesearch.proto` +
  `--serve-filesearch` + reflection (mirror an existing unary seam; commit the
  `buf.image.binpb` bump via `nix run .#buf-image`, add the endpoint to
  `nix/constants.nix` → `nix run .#gen-constants`); the `= "index"` candidate
  source over `SearchBackend::list_files`; frecency/recency weighting; and a
  streaming `FileSearchSession` (codex) for keystroke-debounced completion.

## References

- **agent-seddon:**
  [`crates/agent-reference/src/resolver.rs`](../../crates/agent-reference/src/resolver.rs) (`LocalResolver` — exact `@`-resolution this seam makes tolerant; `confined`, `is_sensitive`, `scan_for_injection`),
  [`crates/agent-core/src/security.rs`](../../crates/agent-core/src/security.rs) (`confine`, `scan_for_injection` — the query/base-dir guards),
  [`crates/agent-core/src/lib.rs`](../../crates/agent-core/src/lib.rs) (`SearchBackend`, `list_files` — the deferred index candidate source),
  [`crates/agent-tools/src/search.rs`](../../crates/agent-tools/src/search.rs) (`grep`/`find`/`ls` — the exact/regex, unranked baseline; the `ignore`-walker conventions to reuse),
  [`crates/agent-search`](../../crates/agent-search) (tantivy index, spec 15),
  [`crates/agent-runtime/src/registry.rs`](../../crates/agent-runtime/src/registry.rs) (`register_builtins`),
  [`crates/agent-runtime/src/metered.rs`](../../crates/agent-runtime/src/metered.rs) (`Metered*` pattern),
  [`crates/agent-runtime/src/builder.rs`](../../crates/agent-runtime/src/builder.rs) (`Arc`-handle wiring),
  [`crates/agent-metrics/src/lib.rs`](../../crates/agent-metrics/src/lib.rs) (metric families),
  [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs) (`tempdir`, doubles);
  dependencies: spec [17](17-reference-resolution.md) (`@`-resolution — the completion route), spec [05](05-text-search.md) (gitignore/hidden walk conventions), spec [15](15-semantic-search.md) (index candidate source).
- **codex (anchor):** `codex-rs/file-search/src/lib.rs` (`run`, `create_session`, `nucleo` fuzzy over an `ignore` walk, `FileMatch`/`MatchType`, `cmp_by_score_desc_then_path_asc`, `FileSearchOptions{limit,exclude,respect_gitignore}`, `require_git(true)`), `codex-rs/file-search/src/cli.rs`, `codex-rs/file-search/Cargo.toml` (`nucleo`, `ignore`); tests inline in `src/lib.rs` `#[cfg(test)] mod tests` (`verify_score_is_none_for_non_match`, `tie_breakers_sort_by_path_when_scores_equal`, `file_name_from_path_uses_basename`, `run_returns_matches_for_query`, `run_returns_directory_matches_for_query`, `run_classifies_followed_directory_symlink_as_directory`, `cancel_exits_run`, `parent_gitignore_outside_repo_does_not_hide_repo_files`, `git_repo_still_respects_local_gitignore_when_enabled`).
- **pi (algorithm anchor):** `pi/packages/tui/src/fuzzy.ts` (`fuzzyMatch`/`fuzzyFilter` — in-order subsequence + word-boundary/consecutive/gap scoring), `pi/packages/tui/src/autocomplete.ts` (`scoreEntry` tier ladder, `getFuzzyFileSuggestions` via `fd`, `.slice(0,20)`); tests `pi/packages/tui/test/fuzzy.test.ts` (`characters must appear in order`, `consecutive matches score better`, `word boundary matches score better`, `prioritizes exact matches over longer prefix matches`), `pi/packages/tui/test/autocomplete.test.ts` (`fd @ file suggestions`: `matches file with extension in query`, `filters are case insensitive`, `ranks directories before files`, `returns nested file paths`, `scopes fuzzy search to relative directories`).
- **opencode:** `opencode/packages/core/src/filesystem/fff.{node,bun}.ts` (native `fileSearch`/`directorySearch`/`mixedSearch`, frecency + fuzzy ranked), `opencode/packages/tui/src/component/prompt/autocomplete.tsx` (trusts fff order for `@`-files; `fuzzysort` for non-file items) — ranking is native/library, **no clone-side unit test of the fuzzy scoring**; `opencode/packages/core/test/filesystem/search.test.ts` covers ripgrep/glob (spec 05), not fff.
- **hermes:** — (no fuzzy filename matcher / `@`-mention completion; `agent/context_references.py` resolves near-exact `@file:` paths per spec 17; `tools/fuzzy_match.py` is an unrelated edit/patch text-replacement matcher).
