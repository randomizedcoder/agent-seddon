# Status — review LLM parallelism (bottleneck exploration)

Legend: 🔬 exploring · ⬜ designed, not built · 🟡 partially built · ✅ built + merged ·
❌ **tried, did not pay off — abandoned** (kept as a negative result).

Design-of-record for the chunked map-reduce lever (lever 3): [README.md](README.md).

**Track state: 🔬 EXPLORATION — outcome uncertain, may be marked ❌.** This is a measurement-first
investigation into review latency, opened after OTLP tracing was enabled on the l2 review-fleet
(2026-09-18). The dominant cost is remote-Kimi inference wait; the open question is whether we can cut
it **without** hurting review quality. There is real risk the levers below don't pay off (or regress
recall), in which case we record the evidence here and mark the relevant lever ❌ rather than shipping
it. Nothing here is committed to as a deliverable until a `fleet-measure` before/after justifies it.

## The problem (grounded, from OTLP traces on l2)

Two traced runpod/host reviews (#2746, #2781), then a 3-way concurrent run (#2831/#2830/#2829),
`default.otel_traces` on the ClickStack collector:

- **~90% of review wall-time is the agent loop** (`agent.turn` 169s avg of a 188s review); the
  static-analysis suite the [review-analysis-depth](../review-analysis-depth/README.md) track added is
  ~8% and **fully parallel** — not a bottleneck.
- **The loop is 100% remote-inference idle-wait.** Every `provider.complete` span splits
  `busy_ns` vs `idle_ns`: local CPU does **~4–6ms per review**; the other ~169s is awaiting the remote
  RunPod/Kimi-K3 round-trip. Pure remote-latency bound, not local compute.
- **Per-call latency is the multiplier:** avg 11–20s, p95 65s, **max 157s** (a single non-streamed
  8192-token completion). Iteration count (9–13/review) is the other multiplier.
- **All in-loop calls go to remote Kimi** (`model=moonshotai/Kimi-K3`); the local MI50 pool member
  absorbs only the Review-role summary/digest path, **not** the generation loop.

## Concurrency — measured, cross-review works

Kimi's litellm proxy fronts a **load-balanced ≥3× Nvidia B300 cluster** (per the operator), so it
serves concurrent requests. The fleet already fans reviews at it via `max_total`:

| Concurrent reviews | Wall | Σ provider time | Effective parallelism |
|---:|---:|---:|---:|
| 2 PRs | 202s | 338s | **1.67×** |
| 3 PRs | 329s | 756s | **2.3×** |

Scales with load. Gap to a clean 3× = (a) **staggered starts** (each `grpcurl` spins a fresh
`nix develop` shell), (b) **uneven review lengths**, (c) **giant monolithic non-streamed calls**
(the 157s tail; `stream=false`). **Within a single review the loop is STRICTLY SERIAL** (ReAct: iter N
needs iter N-1's tool result), so one PR uses ~1 cluster slot regardless of cluster width.

## The two axes (they optimize different things)

- **Throughput (many PRs):** cross-review concurrency **already delivers this** (2.3× → ~3× when the
  queue is kept full and reviews start together). Nearly free.
- **Single-PR latency (the 157s tail):** only **within-review chunking** touches this — split a big
  PR into K≈3 coherent chunks, review them as parallel Kimi calls, merge locally. This is the meaty,
  higher-risk lever (quality/recall risk from cross-file context loss + per-chunk context-ingest tax).

## Plan / levers (cheap → risky), each gated on a `fleet-measure` before/after

| # | Lever | Cost | Risk | State |
|--:|-------|------|------|-------|
| 1 | `stream=true` in the fleet toml — attack the 157s non-streamed tail | ~free (config) | low | ✅ **confirmed win** (see 2026-09-18 log) |
| 2 | Keep the poller queue full + start reviews **together** → chase a clean 3× cross-review | ~free | low | 🟡 **capped by review-length variance** (see log) |
| 3 | **Chunked map-reduce review**: MI50 split diff → K≈3 parallel Kimi chunk-reviews → MI50/1×Kimi merge; gated on file-count, K=min(cluster_width, files/group) | new track (multi-PR) | **high** (recall/context-loss; per-chunk ingest tax; the "too-small" sweet-spot) | ⬜ **designed** — see [README.md](README.md) |
| 4 | **Non-convergence guard**: `[agent] max_unproductive_iters` — force-finalize after N consecutive turns that re-issue only already-seen tool calls; `agent_loop_early_stops_total` metric | gated PR (agent-loop) | low (fires only on genuine repetition; distinct exploration untouched) | ✅ **built + merged (#405)** — a safety net; no false-fires live (see log) |

**Sweet-spot reasoning (why chunking may NOT pay off):** each chunk re-pays the fixed context-ingest
cost (the 65s "iter-1 full-context" call), so K chunks cost `K×ingest + reasoning`, not `1/K`. Too
small ⇒ the ingest tax eats the parallelism win AND a bug spanning two files (caller in A, callee in B)
is missed if they land in different chunks. If the measured win is marginal or recall drops, lever 3 is
marked ❌ here.

## Evidence log

- **2026-09-18 — bottleneck identified + concurrency measured (l2, OTLP traces).** Numbers above.
  Verdict so far: throughput is effectively solved by existing cross-review concurrency; the only
  untapped lever for single-PR latency is chunking, which carries real quality risk. Levers 1+2 to be
  measured next; lever 3 pending a design + a go/no-go from the numbers.

- **2026-09-18 — lever 1 (`stream=true`) + lever 2 (started-together) measured (l2, 3 PRs
  #2828/#2827/#2826, `provider.stream`).** Fleet redeployed with `stream=true`; all 3 fired from a
  single dev-shell (no per-call stagger).
  - **Lever 1 ✅ CONFIRMED WIN:** max single provider call **157s → 62s** — the monolithic
    non-streamed tail is gone. Normal-PR iteration counts unchanged (7 / 10 vs the prior non-stream
    run's 6 / 7 / 9), so streaming does **not** perturb the loop or quality. Zero cost. **Kept in the
    fleet toml.**
  - **Lever 2 🟡 INCONCLUSIVE / capped:** concurrency came out **1.64×**, *worse* than the earlier
    2.3×, but the run was confounded by an outlier (below) — the two small PRs (7 & 10 iters) finished
    early and left the big one running **solo** at the tail, dropping the cluster to 1×. **Finding:
    cross-review concurrency is capped by review-length variance** — one long PR strands the cluster at
    1× during its solo tail, so "keep the queue full" only helps when review lengths are comparable.
  - **★ KEY NEW FINDING — the dominant single-PR tail is a BIG-PR iteration runaway, not the
    monolithic call.** #2828 (**40 files / 3298 churn**, 200 findings capped) looped `git_grep` **29×**
    exploring and hit **`max_iterations=40`** → 361s `agent.turn` (a new worst case, worse than the
    157s call streaming just fixed). It still produced a draft, but burned the whole iteration budget.
    This is the **strongest evidence FOR lever 3 (chunking)**: a 40-file sprawling PR is exactly the
    case where the model can't hold the whole diff and loops exploring; splitting it into bounded
    coherent chunks would (a) give each chunk a focused context that's far less likely to run away, and
    (b) parallelize across the B300 cluster. So the exploration is trending **toward "chunking is
    justified"**, not unsuccessful — though the sweet-spot/recall risk is still unproven and remains
    the go/no-go for lever 3.
  - **Cheaper adjacent lever surfaced (lever 4, candidate):** a **non-convergence / repeated-tool-call
    guard** (e.g. detect N identical `git_grep`s and force the final answer, or scale `max_iterations`
    down for the common case) would cap the runaway tail directly, at far less cost than chunking.
    Worth measuring before committing to the chunking track.

- **2026-09-18 — lever 4 BUILT (non-convergence guard).** `[agent] max_unproductive_iters` (default 3,
  `0` disables): the agent loop signature-hashes each turn's tool calls (name+args, via `fnv1a_hex`);
  after that many *consecutive* turns that introduce no new signature (the model re-issuing already-seen
  calls) it force-finalizes early through the existing tools-disabled finalize turn, instead of spinning
  to `max_iterations`. **Fires only on genuine repetition** — distinct exploration always introduces a
  novel signature, so a progressing review is never cut (this is the deliberate split from lever 3:
  distinct-exploration on a big PR is chunking's job, not the guard's). Ships with a new
  `agent_loop_early_stops_total` metric so a live sweep can measure how often it fires (the go/no-go
  signal). Gate green; table-driven tests cover positive/negative/boundary/corner + adversarial
  (hostile/huge args hashed without panic). **Merged as #405** (main 2e7d411).

- **2026-09-18 — lever 4 LIVE-VERIFIED on l2 (guard binary, `max_unproductive_iters=3`).** Redeployed
  the fleet with the merged guard and swept 3 fresh PRs, two of them **446-file / ~43k-churn monsters**
  (#2699, #2733) plus #2783 (25 files). **The guard did NOT fire** — `agent_loop_early_stops_total`
  stayed **0** — and all three converged normally: **11 / 10 / 6 iterations**, well under
  `max_iterations=40`. **Honest outcome:** the guard is a **safety net with zero false-positives** —
  it correctly left healthy reviews untouched even at 446 files. It does **not** deliver a common-case
  latency win: #2828's 40-iter runaway was a *specific pathology* (looping near-identical greps), not a
  general big-PR property (446-file PRs converge in ~10 iters). It remains valuable as the fail-safe
  for that pathology (proven to fire in the unit tests). #2828 itself is C16-locked (already drafted
  pre-guard), so an on-demand live *firing* wasn't reproducible without a fresh pathological PR. **The
  real single-PR latency lever remains chunking (lever 3, [README.md](README.md)).**
