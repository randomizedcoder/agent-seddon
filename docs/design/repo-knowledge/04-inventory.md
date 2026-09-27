# 04 — Inventory: the deterministic skeleton, the cited summaries, the embeddings, the LLM extras

The inventory answers "what does this repo consist of?" in a form both humans and the model can
use. Its skeleton is derived from the graph with no model involved (RK-10). Its prose is written
by a cheap model but every sentence must cite a graph key that exists, or it is rejected (RK-11).

## Deterministic skeleton (RK-10, every ready snapshot)

| `repo_features.kind` | `feature_key` | Derived from | Evidence roles |
|---|---|---|---|
| `crate` | `crate:<name>` | `rust:crate:*` nodes | `defines` (pub items), `tests` (tests in crate), `documents` (docs linking it) |
| `seam` | `seam:<Trait>` | traits in `profile.seam_crate` with `attrs.async_trait`, or listed in `profile.seams` | `defines` (the trait), `implements` (impls), `tests` (tests covering impls), `exposes` (gRPC service when one mirrors it) |
| `tool` | `tool:<name>` | impls of `profile.tool_trait` with `attrs.tool_name` | `implements`, `tests`, `gates` (cargo feature) |
| `cargo_feature` | `cargo_feature:<crate>/<f>` | `rust:feature:*` nodes | `gates` (items with `gated_by`) |
| `config_section` | `config_section:<s>` | `config_key` nodes grouped by section (RK-15) | `defines` |
| `capability` | `capability:<name>` | fixed rules per profile: `observability` (metric / span nodes), `grpc` (proto services), `persistence` (table nodes) | `defines` |

For agent-seddon, `profile` is `{ "seam_crate": "agent_core", "tool_trait": "Tool",
"config_root": "crates/agent-runtime/src/config.rs", "testkit_crate": "agent_testkit" }`.

`attrs` on each feature carry counts: `impls`, `tests_by_class` (`{positive, negative, corner,
boundary, adversarial, other}`), `pub_items`, `dependents`. `parent_key` links a `tool` or
`cargo_feature` to its `crate:`.

`subgraph_hash` = sha256 over the sorted `(node_key, body_hash)` pairs of the evidence nodes plus
their outgoing edges. It is the change detector for the LLM step: a feature whose evidence did
not change is not re-summarised.

Feature-to-feature relations are computed at query time (shared evidence nodes; `depends_on`
between parent crates), not stored.

## LLM summaries (RK-11)

For each subject whose `subgraph_hash` differs from the stored `repo_summaries.subgraph_hash`:

1. **Slice.** A deterministic text of at most ~3 k tokens: the feature row, its evidence nodes
   printed as `` `node_key` (file:line) — <attrs.doc> ``, test counts by class, dependents. The
   model sees keys, not raw source.
2. **Prompt.** "Describe this <kind> in at most N words for an engineer new to the repo. Cite
   every claim with a key from the slice. Output JSON `{ "summary": "...", "citations": ["..."] }`."
3. **Validate, fail closed** (`cite.rs`): every entry of `citations[]` must be a `node_key` in
   the snapshot; the summary must contain at least one citation; unknown keys trigger one retry
   whose prompt lists them; a second failure keeps the previous summary and records
   `status = "citation_rejected"` in the run report.
4. **Sanitize.** The digest template: cap at 16 KiB (`crates/agent-digest/src/lib.rs:43-64`)
   and reject when `scan_for_injection` fires (`crates/agent-core/src/security.rs:97`).
5. **Store** with `model`, `tokens_in`, `tokens_out`, `ts_ms`.

The `architecture` summary (subject `repo`) is built from the crate summaries, the `depends_on`
layering (a topological order of crates with their layer index) and the seam list. It is
regenerated whenever any crate summary changed.

Model: the cheap pool, as `summaries` and the digest distillation use
(`crates/agent-runtime/src/config.rs:650,922`), never the session model.

## Embeddings (RK-11)

`Embedder::embed_docs` (`crates/agent-core/src/lib.rs:1526`) over:

- every summary (`subject_kind = 'summary'`, key = `<kind>/<subject_key>`);
- every pub `fn`, `struct`, `trait` as `name + first doc line` (`subject_kind = 'symbol'`,
  key = `node_key`); about 3–4 k rows for this workspace.

`content_hash` skips unchanged inputs. `dim` must equal `Embedder::dimensions()` (256 for the
local embedder, `crates/agent-embed/src/local.rs:41-49`); rows with a different `model` are
ignored on read. Similarity is cosine in Rust over a bounded fetch (`LIMIT 4096` rows of the
matching `subject_kind` and `model`), the same brute-force approach as
`crates/agent-search/src/vector.rs:2`.

## Token estimate

Cold build for agent-seddon: ~24 crates + ~75 seams + ~30 tools + ~257 cargo features ≈ 390
subjects × ~900 tokens in and ~100 tokens out, plus one architecture pass: ≈ 340 k tokens in,
≈ 40 k out. A typical PR dirties 1–3 crates and their features: ≈ 15–25 k in. Priced through the
provider rate card, not here.

## LLM extras (RK-18)

Same machinery (slice → cited JSON → validate → sanitize → hash-gated), new `repo_summaries.kind`
values already in the CHECK set:

| kind | subject_key | Deterministic input | What the model adds |
|---|---|---|---|
| `conventions` | `repo` | error enum, async runtime, test harness and class prefixes, lint config (`Cargo.toml` `[workspace.lints]`), `#[cfg]` conventions, `CLAUDE.md` | house style in ≤ 800 words with ≤ 5 cited examples |
| `glossary` | `repo` | candidates: type names with docs, `docs/**` titles, doc headings | one line per term, each citing its defining node or doc |
| `playbook` | `playbook:<seam\|tool\|migration\|config-key\|check>` | the RK-16 change recipe (ordered file list + example commits) | narration only; the file list is copied from the recipe and validated against it |
| `pitfalls` | `repo` | commits whose subject matches `fix\|security\|regress\|leak\|panic` (subjects only, ≤ 50), `CLAUDE.md` "rules that already cost a bug", guard nodes | each pitfall cites a commit sha and a guard key |
| `overview_l0` | `repo` | the architecture summary, the seams, the conventions summary | ≤ 2 KB: what it is, the five rules, where to look first |

The `playbook` validator rejects any file path in the text that is not in the recipe's touch
set, so a model cannot invent a file to edit.
