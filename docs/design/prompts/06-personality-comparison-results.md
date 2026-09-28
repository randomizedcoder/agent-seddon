# 06R — Personality comparison: **empirical results-of-record**

> Companion to [`06-personality-comparison.md`](06-personality-comparison.md).
> That doc is *descriptive analysis* and explicitly defers the verdict to "a live
> A/B on our own generators (Kimi/Qwen) … measure before you commit." **This doc
> is that measurement**, as an append-only log. Each run re-answers one question —
> *when agent-seddon reviews code as each personality, which head base does the
> best job?* — and records enough provenance that, as the prompts change over
> time, a later run's numbers can be compared against an earlier one and the delta
> explained.

**How to read this.** The [Runs](#runs) section is **newest-first**; each entry is
self-dated and stamped with the repo commit, the generator/judge models, and a
content hash of every personality head base *as tested*. If two runs disagree,
diff their head-base hashes and model stamps first — a changed number usually
tracks a changed prompt (or a changed generator), which is exactly the history
this log exists to capture.

**What this is not.** Not a gate and not a claim about absolute review quality —
it is a *relative* ranking of the five personalities under one fixed target and
one generator, with the honest caveats each entry records.

---

## Methodology (stable across runs unless an entry says otherwise)

The experiment isolates **one variable — the personality head base**. Everything
else is held identical across the five profiles within a run.

1. **Target.** A single fixed source file with a known, recorded defect set:
   *N* planted bugs to catch and *M* clean functions as false-positive bait (the
   [current fixture](#fixture) is embedded below so a run is reproducible from this
   doc alone). The same bytes are reviewed by every profile.
2. **Vehicle.** The agent **loop**, one-shot, once per personality:
   `agent --config personality-<p>.toml "<review goal + the fixture inline>"`.
   The only line that differs between the five configs is `[agent] personality`;
   the head base is read at session assembly
   (`resolve_system_prompt`, `crates/agent-prompt/src/lib.rs`). Note the CLI
   `agent --review` path is a *deterministic, no-LLM* fact collector and is
   **personality-invariant** — it is deliberately **not** used here.
3. **Generator.** One model for all profiles (recorded per run), so any spread is
   attributable to the head base, not the model.
4. **Judge.** GLM-5.2, **blind** to which personality wrote each review, scoring
   against the recorded ground truth: *caught* (which planted bugs), *false
   positives* (clean functions wrongly called buggy), *clarity* 1–5. Three votes
   per review; a bug counts as caught on a ≥2/3 majority, FP/clarity by median.
5. **Reps.** Each profile reviews the target ≥2× to average endpoint variance;
   runs interleave by rep.
6. **Composite** (for ordering only): `catch·10 − false_pos·5 + clarity`.

**Operational gotchas** (learned 2026-09-18 — save yourself the debugging):
- **Run the one-shots sequentially.** Concurrent `agent` processes collide on the
  tantivy index writer-lock and the metrics port (`LockBusy`, `9600 in use`). Give
  each its own `index_dir`/port if you must parallelise.
- **GLM-5.2 is a reasoning model.** A small `max_tokens` gets spent on reasoning
  and the answer comes back empty; give the judge a large budget (≈8k) and fall
  back to `reasoning_content`, or votes silently drop.
- **RunPod endpoints are ephemeral / self-signed** — see
  [`docs/llm-endpoints.md`](../../llm-endpoints.md); the judge needs a custom
  User-Agent (edge 403s urllib's default) and modest per-call latency (edge 524s
  at ~100 s).

The harness that produced the first entry (config generator, judge, fixture) is a
scratchpad script, not yet committed; productionising it as
`nix run .#personality-review-arena` (adapting the graph-arena engine — blind
packets, majority-vote judging, paired comparison) is the natural next step and
would make every future entry a single command.

---

## Runs

### 2026-09-18 — first live A/B/n (Kimi K3 generator)

**Verdict: `agent-seddon` ties for first (with `codex`); nothing beats it.** Every
profile caught **all six** planted bugs with top clarity — a **ceiling effect**:
the generator (Kimi K3) is strong enough that the head base did not move
catch-rate or clarity. The only separator was **false-positive discipline**.

| personality      | catch /6 | false-pos | clarity /5 | composite |
|------------------|:-------:|:---------:|:----------:|:---------:|
| **agent-seddon** | 6.0 | **0.0** | 5.0 | **65.0** |
| **codex**        | 6.0 | **0.0** | 5.0 | **65.0** |
| pi               | 6.0 | 0.5 | 5.0 | 62.5 |
| hermes           | 6.0 | 0.5 | 5.0 | 62.5 |
| opencode         | 6.0 | 1.0 | 5.0 | 60.0 |

Per-rep false positives (the whole signal): agent-seddon `[0,0]`, codex `[0,0]`,
pi `[1,0]`, hermes `[0,1]`, opencode `[0,2]`. Verbosity (words/review): codex ~619,
agent-seddon ~832, pi ~851, opencode ~1086, hermes ~1195.

**Observations.**
- Both agent-seddon and pi *executed the code to verify each defect* ("verify,
  don't guess" showing through), quoting concrete failing outputs — so
  verification-by-execution is not unique to agent-seddon on this generator.
- The false positives were borderline nits on a clean function (e.g. flagging
  `line_labels` for a `KeyError` on a missing key), not invented bugs — the
  weaker profiles are *over-eager*, not *wrong*.
- agent-seddon and codex hit the sweet spot: complete catch, zero over-flagging;
  codex did it in the fewest words.

**Honest caveats.** One target, N=2 reps, one generator; the FP gaps (0 vs 0.5 vs
1.0) are small and within plausible noise — treat the ordering as indicative, not
decisive. Because catch-rate saturated, this run does **not** discriminate the head
bases on *finding* ability. To stress the head base, a future run should use a
**harder target** (subtler bugs, a larger multi-file diff) and ideally a **weaker
generator** (Qwen on the l2 MI50) so the system prompt carries more weight.

**Provenance (stamp for future diffs).**
- Repo commit: `01dcdc2`
- Generator: Kimi K3 (`moonshotai/Kimi-K3`) · Judge: GLM-5.2 (`/model`), 3 votes,
  blind — endpoints per [`docs/llm-endpoints.md`](../../llm-endpoints.md)
- Head bases tested (`prompts/personalities.example/<p>/0001_<p>.md`, sha256 first
  12 hex): agent-seddon `386c748ccc6e` (4338 B) · pi `8e48fc1a8877` (1455 B) ·
  hermes `ee0b9d684399` (2533 B) · opencode `ade9199b00df` (8695 B) · codex
  `ac8ae107a0d7` (20903 B)
- Switch mechanism separately verified live via
  `PromptService/{Get,Set}ActivePersonality` on `--serve-all` (round-trip + hostile
  id → `InvalidArgument`, state unchanged on reject) — see
  [`10-portal-selector.md`](10-portal-selector.md).

---

## Fixture (as of the latest run)

`shop/cart.py` — 6 planted defects (B1–B6), 4 clean functions as FP bait.

<details><summary>Ground truth</summary>

- **B1** `add_item` — mutable default argument `tags=[]` (shared across calls; state leaks between carts). *subtle.*
- **B2** `apply_discount` — `total*(1-percent)` with percent 0–100 → negative/wrong charge; must divide by 100.
- **B3** `average_price` — `sum(prices)/len(prices)` with no empty guard → `ZeroDivisionError`.
- **B4** `find_user` — SQL built by string concatenation of `name` → SQL injection *(high)*.
- **B5** `save_receipt` — `open(path,"w")` never closed (no `with`) → file-handle leak.
- **B6** `top_n` — `range(n+1)` → returns n+1 items and `IndexError` when n≥len (off-by-one).

Clean (flagging any as a *defect* is a false positive): `subtotal`, `line_labels`,
`find_item`, `checkout`. (Noting `checkout` is *affected by* the buggy
`apply_discount` is correct, not an FP.)
</details>

<details><summary>shop/cart.py</summary>

```python
"""Shopping-cart helpers for the checkout service."""

import sqlite3
from typing import Optional


def add_item(cart, item, tags=[]):                      # B1: mutable default arg
    tags.append(item.get("category", "misc"))
    cart.append({"item": item, "tags": tags})
    return cart


def apply_discount(total, percent):                     # B2: percent not /100
    return total * (1 - percent)


def average_price(items):                               # B3: div by zero on empty
    prices = [it["price"] for it in items]
    return sum(prices) / len(prices)


def find_user(db: sqlite3.Connection, name: str):       # B4: SQL injection
    cur = db.cursor()
    cur.execute("SELECT id, name, email FROM users WHERE name = '" + name + "'")
    return cur.fetchone()


def save_receipt(path, lines):                          # B5: handle never closed
    f = open(path, "w")
    for line in lines:
        f.write(line + "\n")
    return path


def subtotal(items):                                    # clean
    total = 0.0
    for it in items:
        total += it["price"] * it.get("qty", 1)
    return total


def line_labels(items):                                 # clean
    return [f"{it.get('qty', 1)} x {it['name']}" for it in items]


def find_item(items, name: str) -> Optional[dict]:      # clean
    for it in items:
        if it.get("name") == name:
            return it
    return None


def top_n(items, n):                                    # B6: off-by-one / IndexError
    ordered = sorted(items, key=lambda it: it["price"], reverse=True)
    picked = []
    for i in range(n + 1):
        picked.append(ordered[i])
    return picked


def checkout(cart, discount_percent=0):                 # clean (uses buggy apply_discount)
    base = subtotal(cart)
    if discount_percent:
        base = apply_discount(base, discount_percent)
    return round(base, 2)
```
</details>

---

## Adding a run

1. Re-run the five one-shots + blind judge (methodology above); keep the target
   fixed unless you are deliberately raising difficulty — if you change it, update
   [Fixture](#fixture) and say so in the entry.
2. Prepend a new dated `### YYYY-MM-DD` entry under [Runs](#runs) (newest first)
   with: the verdict line, the ranking table, per-rep FPs, observations, caveats,
   and the **provenance stamp** (repo commit, generator/judge, per-personality
   head-base sha256 + bytes).
3. If the head-base hashes differ from the previous entry, note *what changed in
   the prompt* and whether the numbers moved with it — that linkage is the point of
   this log.
