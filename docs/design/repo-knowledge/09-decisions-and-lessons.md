# 09 — Architectural decisions and confirmed review lessons

**Proposed; no runtime implementation in this documentation change.** This extends the
[existing graph and inventory](README.md), after structural review integration RK-03/06–09.
Delivery is RK-19–21 in [the increment sequence](06-increments.md); tests are K1–K4 in
[the matrix](08-test-matrix.md); execution and evaluation are in [the runbook](10-integration-and-evaluation.md).

## Purpose and boundaries

The graph answers what code exists and how it relates. This layer records why a repository
uses a particular approach and what a human has confirmed from a review. An example is a
retry implementation linked to a decision about non-idempotent writes, the document explaining
that decision, and a regression test. The agent retrieves the explanation when the code changes.

Keep three distinct sources of knowledge:

| Source | Authority | Storage and use |
|---|---|---|
| Parser/git relationships | Extracted facts at a commit; completeness depends on extractor | Existing graph nodes/versions/edges |
| Generated feature summaries | Model interpretation with validated evidence | Existing RK-11 inventory; labelled model-written |
| Decisions and review lessons | Human-confirmed interpretation with evidence | New records linked to graph keys; retrieved as cited context |

No model writes deterministic graph edges. The new links belong to the knowledge layer.
No graph visualization, Obsidian synchronization, arbitrary cross-repository links, or new graph
or vector database is required. Existing episodic, semantic and dimensional memory continue to
serve conversations; this is shared repository knowledge scoped to the authenticated tenant.

## Evidence and data model (RK-19)

Add `RepoKnowledgeStore` and its types in `agent-core`; put the memory conformance double in
`agent-testkit` and the Postgres implementation in `agent-repo-graph`. The handle owns tenant
identity, following the as-built graph store; method arguments cannot select another tenant.
All reads/writes require a repository id. Do not introduce a gRPC service in RK-19–21.

Reserve migration **0004** after the existing planned inventory 0002 and profile/history 0003.
Use the graph migration ledger and transaction discipline. Do not edit applied migration 0001.
The proposed tables are:

| Table | Required fields and constraints |
|---|---|
| `repo_knowledge` | Tenant/repo/id primary key; kind `decision` or `lesson`; immutable title/body/evidence digest; state `candidate`, `confirmed`, `superseded`, or `rejected`; creation/source metadata; revision for compare-and-swap; confirmation actor/time; nullable replacement id scoped to the same repo |
| `repo_knowledge_evidence` | Tenant/repo/knowledge id/ordinal; source kind; source locator; source commit/head SHA; content SHA-256; bounded excerpt; optional originating snapshot id as metadata; node key and role where applicable |
| `repo_knowledge_links` | Tenant/repo/knowledge id/node key/role; roles `applies_to`, `explained_by`, `validated_by`; expected node kind, signature/body hashes, and source-file hash captured at confirmation |
| `repo_knowledge_events` | Tenant/repo/knowledge id/event id; action, authenticated actor, timestamp, prior/new revision, reason, replacement id; append-only audit of transitions |

Composite foreign keys enforce repository and tenant boundaries. Evidence links reference the
knowledge record, not a retained snapshot: retention must not erase confirmed history. Store
immutable source identity and a bounded excerpt for audit; this does not make expired evidence
eligible for current retrieval. Deleting the registered repository cascades its knowledge too.

Source kinds are repository documents, code/tests, and fleet review feedback. Review evidence
uses forge identity, PR number, review id, finding `item_id`, and head SHA; retain references to
existing feedback, not a copy of the PR/event stream. Preserve a small redacted evidence excerpt
because telemetry retention can remove the original. URLs are references, never automatically
fetched during retrieval. Credentials and raw telemetry payloads are not stored.

At least one `applies_to` code key and one supporting evidence item are required. A decision
must cite a repository document; a lesson must cite a review finding or a repository document
recording the lesson. Code/test evidence may supplement either. Confirming validates evidence
against a ready snapshot and the source at its recorded revision. Existence of a citation proves
its identity, not the truth of the prose; the human confirmation supplies that judgment.

Limits: title 256 UTF-8 bytes; body 8 KiB; 32 evidence items; 64 links; source locator 2 KiB;
excerpt 2 KiB each and 16 KiB total; transition reason 1 KiB. Over-limit writes are rejected,
not silently truncated. Read limits default to 20; explicit values clamp to 1–100 (zero becomes one). Reuse graph key/path grammar,
parameterized SQL, content scanning and redaction. Rejection messages do not echo hostile text.
Scanner success does not establish trust; imported prose is always delimited as evidence.

New ids are server-generated UUIDs; revisions start at 1 and increase on each state transition.
Use a same-repo foreign key for replacement ids, CHECK constraints for closed enums/positive
revision, and indexes on `(tenant, repo, state, id)` and `(tenant, repo, node_key)` for selection.
All byte/count limits are enforced in the shared validator before either store writes.

The proposal fingerprint is SHA-256 over kind, normalized title/body (LF and trimmed outer
whitespace), sorted links, and sorted evidence identities/content hashes. A unique
`(tenant, repo, fingerprint)` makes repeated proposals return the existing id, including rejected
records. Edited content creates a new candidate; human decisions cannot be overwritten by a
model retry. Related but non-identical proposals remain separate for human comparison.

## State and freshness

| Action | Preconditions | Transactional outcome |
|---|---|---|
| Propose | Valid, bounded content and same-repository evidence | New candidate, or existing fingerprint id; never confirmed automatically |
| Confirm | Candidate; authorized human; matching revision; evidence resolved and current for the selected snapshot | Confirmed record plus actor/time and audit event |
| Reject | Candidate or confirmed; authorized human; matching revision and reason | Rejected, excluded from guidance, historical record retained |
| Supersede | Old record confirmed; replacement confirmed in same repo; distinct ids; both revisions match | Old becomes superseded and links to replacement; one atomic event; no cycles |
| Inspect/list | Authorized tenant/repo access | Record and evidence with state and freshness; bounded output |

Confirmation is explicit. A posted review, merged PR, or feedback `addressed` state is not
confirmation. Today `addressed` means a finding disappeared in a later review, which alone does
not establish a general lesson. Models may propose candidates but cannot confirm, reject or
supersede them. Expose those transitions only through the operator CLI, using the existing
operator/tenant authorization boundary; no model-callable mutation tool or caller-supplied
`confirmed_by` is accepted. Audit the resolved operator identity. Local operation follows the
existing trusted operator model; this feature adds no unauthenticated remote mutation surface.

Freshness is computed separately from lifecycle, relative to the snapshot used by the caller:

- `current`: every applicable code key and supporting document/code evidence matches its
  captured hashes. A newer commit alone does not make knowledge stale.
- `stale`: a key is deleted/renamed, its signature/body changes, or a supporting document changes.
- `unverifiable`: no suitable ready snapshot, source unavailable, or evidence cannot be checked.

Use source-file hashes for documentation changes; the existing structural `graph_hash` excludes
some attributes and line changes and is not an evidence freshness hash. Repository evidence
must be re-resolvable at its recorded commit for confirmation. Missing review history blocks
confirmation if it is required evidence; confirmed historical excerpts remain inspectable.
A confirmed lesson remains inspectable after retention, but only `current` knowledge is injected.
Do not silently remap a renamed symbol. Revalidation creates a revised candidate with new
citations, followed by human confirmation and explicit supersession of the old record.

## Interfaces and operator workflow (RK-20)

Proposed trait verbs: `propose(repo, proposal)`, `get(repo, id)`,
`list(repo, filter, limit)`, `confirm(repo, id, expected_revision, snapshot, actor)`,
`reject(repo, id, expected_revision, reason, actor)`,
`supersede(repo, old_id, old_revision, replacement_id, replacement_revision, reason, actor)`,
and `select_current(scope, keys, limit)`. `actor` is an internal trusted identity type constructed
by authorization, not a model argument. Use typed `NotFound`, `Conflict`, `Invalid`, `TooLong`,
and `Backend` errors consistent with the graph store. Confirmation validation and state changes
must share a transaction/snapshot view; concurrent contradictory transitions have one winner.

Future CLI surface (not runnable yet):

```sh
agent repo knowledge propose --repo <slug> --input <proposal.json>
agent repo knowledge list --repo <slug> --state candidate
agent repo knowledge show --repo <slug> --id <id> --at <sha>
agent repo knowledge confirm --repo <slug> --id <id> --revision <n> --at <sha>
agent repo knowledge reject --repo <slug> --id <id> --revision <n> --reason <text>
agent repo knowledge supersede --repo <slug> --id <old> --revision <n> \
  --replacement <new> --replacement-revision <m> --reason <text>
```

`proposal.json` mirrors the proposal type: kind, title, body, evidence[], links[]. It contains
no tenant, actor, lifecycle state, or database ids for another repository; reject unknown fields.
The explicit operator command may propose document-backed knowledge directly. Optional automated
proposal generation reads the existing fleet feedback interface and uses a scripted/provider
adapter to draft candidates with citations. It never participates in the review-post transaction.
Failures emit a bounded diagnostic and leave ordinary review unaffected.

Proposed configuration: `[repo_graph.knowledge] enabled = false`,
`propose_from_reviews = false`, `max_brief_bytes = 2048` (clamp 0–2048; zero disables injection).
The store uses the existing graph DSN reference and tenant handle. `enabled` requires graph
storage; inconsistent configuration is an initialization error. No raw DSN in tools or prompts.

## Retrieval and evaluation (RK-21)

Use touched graph keys in review mode and the RK-12 feature selection in Implement/Design mode.
Select only confirmed/current records, first direct `applies_to` matches then existing graph
neighbors within two hops; ties sort by knowledge id. Limit to five records within the 2 KiB
sub-budget, **inside** the existing review 4 KiB / Implement 6 KiB total, not extra context.
Drop whole records when they do not fit. Render id, title, bounded body, citation locators,
source SHA and confirmation status. Include a count of omitted stale/unverifiable items.
Knowledge is evidence and cannot override policy or operator instructions.

Keep the feature off by default until the [paired evaluation](10-integration-and-evaluation.md)
shows useful findings without worse false positives. Compare structural-only reviews with the
same reviews plus confirmed knowledge; freeze the knowledge corpus before the evaluation to
avoid learning the held-out answers. Record selection counts, bytes, stale omissions and lookup
latency using existing tenant/repo observability conventions; no knowledge text, source URLs,
PR ids or node keys in metric labels.
