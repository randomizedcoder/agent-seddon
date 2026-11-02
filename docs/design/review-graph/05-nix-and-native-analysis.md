# 05 — Nix, shell-snippet and C/C++ analysis

nixpkgs PRs are mostly about builds: version bumps, build-system changes, patches, compiler
flags. Today the analyzer skips them all: `"no analyzable (.go/.rs) changes"`
(`crates/agent-review/src/analyzer.rs:159`). This document adds a nix suite and a C/C++
suite, run static and then as a sandboxed eval, with **no builds** (user decision,
2026-11-02).

## Prerequisite: "Files changed: 0" (increment 1)

Several nixpkgs drafts report `Files changed: 0` while the model reviews real changes.
Every new collector keys off the changed-file list, so this is fixed first. The evidence:

- The base is the PR's target branch. The diff is three-dot `base...head`
  (`crates/agent-git/src/cli.rs:505`).
- Before each review the orchestrator refreshes the mirror's base refs. That refresh is
  fail-soft and bounded by `PREP_TIMEOUT` = 300 s
  (`crates/agent-review-fleet/src/orchestrator.rs:320`, `:705`).
- On l2 (2026-11-01) the nixpkgs refresh **regularly hits the 300 s cap**: "mirror base-ref
  refresh failed; reviewing against the last-known base … exceeded the 300s fleet prep
  timeout".
- **Root cause found on l2 (2026-11-02), when the disk filled.**
  - The mirror is a `clone --mirror` (`crates/agent-git/src/cli.rs:239`), so its refspec is
    `+refs/*:refs/*`. `fetch --prune --all` therefore pulls every `refs/pull/*` ref in
    nixpkgs, about 11.7M objects (a ~2.9 GB pack).
  - That never finishes inside 300 s, and killing it at the timeout leaves the refs
    unwritten. The next attempt has nothing to negotiate from, so it downloads the whole
    repo again.
  - Result: the mirror grew to 61 GB of duplicate `.keep` packs plus orphaned `tmp_pack_*`
    files, about 2.5 GB every 5 minutes, until the 1.7 TB root was full.
  - The fix makes the mirror heads and tags only: `+refs/heads/*:refs/heads/*` and
    `+refs/tags/*:refs/tags/*`, with no `refs/pull/*`. Tags are cheap and bounded, and
    listing them in the refspec means `--prune` keeps them in step, so a deleted or moved
    upstream tag doesn't linger. PR heads are already fetched one at a time by `fetch_pr`.
    - **New mirrors** are made with `clone --bare` plus these two refspecs, not
      `clone --mirror`.
    - **Existing mirrors** are migrated by an out-of-band **maintenance job**, not inside a
      review's refresh. A repack of the 61 GB mirror can't finish in the 300 s
      `PREP_TIMEOUT`, and a killed repack keeps no progress, so it would loop forever.
      The job runs on its own `maintenance` pool (one worker per process, separate from
      `index` so a two-hour repack doesn't hold up every row's indexing) with its own
      deadline (`mirror_migration_deadline_secs`, default 7200). Meanwhile reviews of that row keep using the mirror as it is: the base fetch
      uses the full refspec from *Fix direction*, which works on either shape.
      `corner_migration_repack_exceeds_prep_timeout_converges` checks that a
      migration slower than `PREP_TIMEOUT` completes in the job and never inside a
      refresh. A refresh that finds the mirror still in the old shape queues that job,
      at most one per row. Detection checks the state that each step leaves behind, so a run killed
      after *any* step is seen again. The migration runs if any of these holds:
      - `remote.origin.mirror` is set (step 1 not done);
      - the configured fetch refspec set is not exactly the two healthy refspecs, compared
        as a set, so order doesn't matter (step 2
        not done). This covers `{}` (killed before the first `--add`), `{heads}` (killed
        between the two `--add`s, where tags would otherwise go stale for good),
        `{+refs/*:refs/*}`, any `refs/pull` refspec, and anything unexpected;
      - any ref exists outside `refs/heads/*`, `refs/tags/*` and `refs/fleet/pr/*` (step 3
        not done), found with one `git for-each-ref` that stops at the first match;
      - any `objects/pack/*.keep` or `tmp_pack_*` older than `PREP_TIMEOUT` is present
        (step 4 not done). A leftover younger than that is caught on a later refresh,
        once it ages past it, so step 4 converges *eventually* rather than on the very
        next refresh.

      The trigger is that detected state, never a missing marker file, so a mirror made
      by the new `clone --bare` path (which never has a marker) is never migrated. Each
      check is cheap enough to run on every refresh. The steps run in this order,
      because the `refs/pull/*` objects are *reachable* and a
      repack alone keeps them:
      1. `git config --unset remote.origin.mirror`. `mirror = true` implies `*:*` on
         fetch, so changing the refspec alone isn't enough. The key may already be gone
         after an interrupted run, and `--unset` then exits 5, so exit 5 counts as
         success.
      2. `git config --unset-all remote.origin.fetch` (exit 5 counts as success, as in
         step 1), then two `git config --add remote.origin.fetch` calls, for
         `'+refs/heads/*:refs/heads/*'` and `'+refs/tags/*:refs/tags/*'`. This is the
         same shape a new mirror gets, so a rerun converges on it.
      3. Delete every ref outside `refs/heads/*`, `refs/tags/*` and the fleet's own
         `refs/fleet/pr/*` (those are pruned by the retention sweep below) with one
         batched `git update-ref --stdin`. `--prune` can't do
         this: it only prunes refs that match the configured refspec, so stale
         `refs/pull/*` refs would survive the refspec change.
      4. Remove `objects/pack/*.keep` and `tmp_pack_*` files older than `PREP_TIMEOUT`
         (younger ones may belong to a fetch in progress), then run `git repack -a -d`
         and `git prune --expire=2.hours.ago`. **Locking:** steps 1–3 take the row's
         mirror lock, which `fetch_pr` and the `knowledge/` refresh also take, and they
         finish in seconds. Step 4 runs **without** the lock. `git repack -a -d` is safe
         alongside a fetch, and the prune grace period protects any object a concurrent
         fetch has just written. So a row's PR fetches are never blocked for the length
         of a repack. The prune is safe while
         reviews are in flight: their heads are under `refs/fleet/pr/<n>`, their bases
         under `refs/heads/*`, and `knowledge/` worktrees sit at a default-branch head.
         Step 3 keeps all of those, so the prune only drops objects that nothing in
         flight can reach, and the grace period covers what a concurrent fetch is writing. Increment 1 also
         confirms on the pinned git that `git prune` treats each linked worktree's
         `HEAD` and index as reachability roots, with a test row
         (`corner_prune_keeps_worktree_head_and_index`).
      5. Write a marker file. It is only a record for the operator, not the trigger.
         Every step is idempotent (absent-key `--unset` and `--unset-all`, then `--add`,
         deleting refs that are already gone, a repack), so a run killed between any two
         steps converges on the next refresh. The
         `adversarial_migration_interrupted_is_idempotent` table kills a fixture
         migration after step 1, inside step 2 before the first `--add` (no refspec),
         between the two `--add`s (heads only), with an unexpected extra refspec added by
         hand, after step 2, after step 3, and with
         SIGKILL during the step-4 `repack` (aged leftovers, converging on a later
         refresh), and checks each rerun
         ends in the same refs and config as an uninterrupted run.
    - **`refs/fleet/pr/*` retention.** `fetch_pr` writes one ref per reviewed PR, and those
      refs keep their objects alive, which would be the same unbounded growth only slower.
      The refresh sweep deletes `refs/fleet/pr/<n>` once:
      - the PR is closed or merged (the poller already sees this), or
      - the ref is older than `pr_ref_max_age_days` (default 14) with no review in flight.

      Pruning (`git gc --auto --prune=…`) then drops the objects. In-flight reviews are
      protected by the same refcount as `knowledge/` generations. Row:
      `corner_closed_pr_ref_swept_in_flight_kept`.
    - Every git subprocess runs in its own process group, and a timeout kills the whole
      group (`fetch` → `remote-https` → `index-pack`).
    - Each refresh sweeps `tmp_pack_*` files older than `PREP_TIMEOUT` first.
    - `agent_fleet_mirror_bytes{tenant,repo}` reports the mirror's size. Over
      `mirror_max_bytes` (default 20 GiB), the refresh is refused and logged. Old-shape
      detection and queueing the migration job happen **first**, before the size check,
      so an over-cap old-shape mirror (l2's 61 GB) still gets migrated rather than being
      refused for ever. Row: `corner_over_cap_old_shape_mirror_still_queues_migration`.
    - A table test proves a timed-out fetch leaves no children and no `tmp_pack_*`. A
      second test runs the migration on a fixture mirror with `refs/pull/*` refs and
      checks that it shrinks.
- Two hypotheses to reproduce, each with a table-test row:
  - **(a)** the target branch (e.g. `staging`) was never fetched into the mirror, so the
    base doesn't resolve and the diff comes out empty;
  - **(b)** a fetch that times out partway leaves no usable merge base.
- Fix direction:
  - Fetch exactly the target branch with a full refspec,
    `git fetch origin +refs/heads/<target>:refs/heads/<target>`, not `--all`. A bare
    `fetch origin <target>` only writes `FETCH_HEAD`, and updates the ref only when a
    configured refspec happens to match, so on its own it would leave hypothesis (a)
    unfixed: the fetch "succeeds" and the diff is still empty. `<target>` comes from the
    forge, so it passes `safe_segment` first. A table row
    (`positive_staging_target_base_written`) checks `refs/heads/staging` resolves after the
    fetch.
  - Fail **loud** when the diff is empty while the forge reports changed files. Use the
    forge's file list as the fallback changed set, and say so in the brief.
- **One slow repo must not stall the fleet.** Today all prep runs on one serial drain
  loop; the fix is the staged work queue in [06](06-work-queue.md) (increment 1b).

## Nix static suite (increment 6)

This adds a `has_nix` branch to `AnalyzerCollector`, next to `has_go` / `has_rust`. It reuses
the Go-suite plumbing: `ToolTask` (`analyzer.rs:99`), `plan_go_tool` (`:418`) as the model for
a `plan_nix_tool`, `finalize` (`:1026`) and the `MAX_FINDINGS` = 200 cap (`:32`). Scope is the
changed `.nix` files only, passed through the existing `is_safe_dir` / `shell_quote` hostile-
path handling.

| Tool | What it catches | Parser |
|---|---|---|
| `statix check -o json` | anti-patterns (manual `inherit`, `if … then true`, legacy `let`) | JSON |
| `deadnix --output-format json` | unused bindings and lambda args | JSON |
| `nixf-diagnose` (nixf-tidy) | parse errors, undefined vars, semantic lints | JSON |
| `nixfmt --check` | formatting (nixpkgs requires RFC-style nixfmt) | file list |
| `nixpkgs-vet` (static half) | by-name structure rules | text |

**Phase-snippet shellcheck.** Build logic sits in `''…''` strings inside `.nix` files, and
the shellcheck collector only sees `.sh`/`.bash` files (`crates/agent-review/src/shellcheck.rs:232`).

1. `ast-grep` extracts indented strings bound to attributes matching `*Phase`, `pre*`,
   `post*`, `*Hook`, `script`, `shellHook`. `nix/versions.nix:137` pins it as
   `pkgs.ast-grep`, which is 0.44.1 at the current nixpkgs lock (the version moves with
   the lock, so increment 6 records it in a comment beside the pin). That build has the Nix
   grammar built in, so no extra grammar needs shipping. This was checked on
   2026-11-02: a rule with `language: nix`, `kind: binding` and `has: {field: attrpath,
   regex: Phase$}` matched a `buildPhase = ''…${foo}…''` binding, with JSON output.
2. Each `${…}` antiquotation becomes a placeholder variable with a per-snippet random
   prefix of 12 base-36 characters, e.g. `"$__nix_k3f9q2m8x1ab_1"`, so shellcheck sees
   valid shell, and a real variable in the snippet can't collide with a placeholder and be
   silenced. The prefix comes from a `PlaceholderPrefix` seam: random in production, and
   fixed in tests so the collision row is deterministic. Nix's `''$` escape becomes `$`.
3. Run `shellcheck -s bash -f json` on each snippet with a line-offset map.
4. Map each finding back to the `.nix` file and line. Drop codes that the placeholder
   substitution caused, a fixed list: SC2154 on that snippet's placeholders only, plus
   SC2034 only when the variable is one of them. Row:
   `corner_real_var_named_like_placeholder_still_reported`.

This is a new `snippet` mode in `shellcheck.rs`. The existing collector's ban on inline
`# shellcheck disable` comments carries over.

**C/C++ suite.** `has_c` is set when the PR changes a file ending in `.c`, `.cc`, `.cpp`,
`.cxx`, `.C`, `.h`, `.hh`, `.hpp` or `.hxx`. Only `.c` and `.C` are matched case-sensitively
(`.C` is C++, `.c` is C, as compilers treat them). Every other extension matches
case-insensitively, so `.H`, `.CPP` and `.CXX` count too.

| Tool | Mode |
|---|---|
| `cppcheck --enable=warning,style,performance,portability --inconclusive --xml` | no compile db needed |
| `clang-tidy` with a conservative check set (`bugprone-*`, `cert-*`, `clang-analyzer-*`, `performance-*`), `--` with `-std=c++20` defaults | no compile db; findings with missing-include noise filtered |

These mostly apply to repos with C/C++ sources. For nixpkgs `.patch` files, a later step
could apply the patch to the fetched source, but that needs network and stays out of scope.

**Shipping the tools.** The agent supplies its own tools, so they ship via nix, never the
host PATH:

1. Pin `statix`, `deadnix`, `nixf`, `nixfmt` (already pinned at `nix/versions.nix:71`),
   `cppcheck`, `clang-tools` (already pinned at `:105` but dev shell only), `nixpkgs-vet` and
   `nixpkgs-hammering` in `nix/versions.nix`.
2. Add them to `review-toolbox` (`nix/review-tools.nix`).
3. Add rows to the `nix/checks/agent-runtime-path.nix` table and the
   `nix/checks/review-toolbox.nix` loop.
4. Add hermetic `review-nix` and `review-cpp` fixture checks (via
   `nix/lib/mk-review-check.nix`), each with a seeded-defect fixture the check must flag.

**Nixpkgs lens prompts.** `prompts/modes/review/nixpkgs/` contains fragments on:

- by-name, `finalAttrs`, meta (license, maintainers, mainProgram, platforms)
- hashes (`hash =` SRI), `strictDeps`, `nativeBuildInputs` vs `buildInputs`
- `versionCheckHook`, `updateScript`
- patches (fetchpatch with a pinned hash; upstreamed?)
- the commit-message format (`pkg: 1.2 -> 1.3`)

They are selected by a `repo:nixpkgs` tag on the row ([01](01-review-graph.md#repo-specific-lenses)).

## Sandboxed eval collector (increment 7, opt-in)

The new `nix_eval` collector is shaped like `gochecks.rs`: opt-in, behind its own toggle, and
run with `NetworkPolicy::Off` (`crates/agent-review/src/gochecks.rs:158`).

**Threat model.** Evaluating PR `.nix` code runs attacker-controlled code:

- **IFD:** import-from-derivation, which builds at eval time.
- **`builtins.fetchurl` / `fetchGit` / `fetchTarball`:** network access at eval time.
- **`builtins.exec`:** if `allow-unsafe-native-code-during-evaluation` is on.
- **Path reads outside the tree:** `builtins.readFile /etc/…`, `<nixpkgs>` lookup paths.
- **Resource exhaustion:** infinite recursion and huge strings or lists.

**Mitigations:**

- **bwrap only.** The `local` and `nix` sandbox backends can't enforce network-off
  (`docs/components/sandbox.md`), so the collector refuses to run on them and reports
  `skipped: needs bwrap`. It uses `--unshare-net`, a read-only `/nix` bind, a private `/tmp`,
  and the worktree mounted read-only through the `readonly_exec` overlay.
- **Flags:**
  - `nix-instantiate --option restrict-eval true --option allow-import-from-derivation false`
  - `--option allow-unsafe-native-code-during-evaluation false`
  - `--option pure-eval true` where the attr path allows it
  - `-I nixpkgs=<worktree>` only, with `NIX_PATH` cleared
  - `--option max-call-depth` plus `ulimit -v`, and a wall-clock deadline. The
    increment-7 spike confirms the option name on the pinned nix, since it differs across
    nix versions and forks. An unknown `--option` fails the eval outright rather than
    being ignored, so the collector probes for support:
    - **Probe.** `nix-instantiate --option max-call-depth 1000 --eval -E 1`, run *outside*
      the sandbox at startup, so a sandbox fault can't masquerade as "unsupported".
    - **Cache.** Keyed by the nix store path. Only definitive answers (supported,
      unsupported) are cached for the process's life. A `probe_failed` is retried with
      `agent-retry` backoff, so a startup blip (daemon socket, full disk) costs minutes of
      nix findings, not the whole process lifetime.
    - **Classification.** Only exit 0 means supported. Only an error naming the unknown
      option means unsupported. The spike pins the exact message on the pinned nix, and
      a misread never counts as "unsupported", only as `probe_failed`. Every `probe_failed` increments
      `agent_review_nix_eval_probe_failed_total`, so a nix upgrade that changes the
      message shows up as a metric rather than as findings quietly stopping. Any other
      failure is `probe_failed`. Until a retry succeeds, the collector reports `skipped:
      probe failed` rather than weakening the bound.
    - **Without support.** The flag is omitted, `ulimit -v` and the deadline are the
      bound, and `agent_review_nix_eval_option_omitted_total{option}` plus a warning log
      make that visible.
    - **Rows.** `corner_max_call_depth_unsupported_omitted_not_failed`,
      `negative_probe_other_failure_skips_collector`,
      `positive_probe_failed_then_retry_succeeds`.
- **Attr selection.** Changed paths are mapped to attrs:
  - `pkgs/by-name/xx/<name>/` → `<name>`;
  - a `pkgs/top-level/all-packages.nix` hunk → its attr names.
  
  Each attr passes `safe_segment` and the count is capped at `nix_eval_max_attrs` (default 10).
- **What runs per attr:**
  - `--eval --strict -A <attr>.meta` (license, maintainers, platforms present?);
  - `-A <attr>.drvPath` (does it evaluate?);
  - `nixpkgs-hammering <attr>` (its eval-based checks). It runs its own evaluations, so
    it gets the same restrictions:
    - it runs in the same bwrap sandbox;
    - `NIX_CONFIG` carries the same options (`restrict-eval`,
      `allow-import-from-derivation = false`,
      `allow-unsafe-native-code-during-evaluation = false`, `pure-eval`);
    - `NIX_PATH` is cleared.

    An adversarial row (`adversarial_hammering_ifd_blocked`) proves an IFD in the
    package fails under hammering too.
  
  The output is parsed into `AnalysisFinding`s.
- **Daemon socket (spike first).** `.drvPath` writes `.drv` files through the nix daemon. The
  increment opens with a spike to prove `nix-instantiate` works inside bwrap with
  `--unshare-net` and the daemon socket (`/nix/var/nix/daemon-socket`) bound in, and to
  settle whether to use `--readonly-mode` for `--eval`-only runs. If the socket can't be made
  safe, the collector ships `--eval`-only, without `drvPath`.
- **Store growth.** The `drvPath` leg writes `.drv` files through the daemon, so a hostile
  package can grow the store even with `nix_eval_max_attrs = 10`, through huge attr values
  or deep `--strict` forcing. The spike measures store growth on an adversarial fixture.
  The collector then enforces `nix_eval_max_store_bytes` (default 256 MiB) by measuring
  `nix path-info -S` over the new `.drv` closure and failing the leg when it is over.
  That check runs *after* the leg, so on its own it detects rather than prevents. The
  preventive bounds are:
  - the per-attr wall-clock deadline;
  - `nix_eval_max_attrs`;
  - `max-call-depth`, which limits how much a single evaluation can generate.

  The accepted residual risk is what a hostile package can write within one deadline.
  The recovery is that the `.drv`s are unrooted and go at the next store GC. The fleet
  disk guard (`disk_min_free_pct`, [04](04-repo-knowledge.md)) pauses this collector
  along with indexing when the disk is low. Row:
  `adversarial_drv_store_growth_capped`.

**Output path.** The collector needs a new `ReviewFacts` slot, `nix_eval: Option<AnalysisReport>`
(proto field 18 in `review.proto`). That means edits to `convert.rs`, the
`digest::compute` report list (`crates/agent-review/src/digest.rs:53`) and the render call.
`DIGEST_MAX_FINDINGS` (80) still caps the digest.

## Tests

- **Parsers:** a table per tool, with fixtures captured from the real tool and four classes
  plus adversarial rows (path traversal in output, a huge message, non-UTF-8).
- **Snippet extraction:**
  - `positive_buildPhase`
  - `corner_antiquotation_placeholder`
  - `corner_escaped_dollar`
  - `boundary_line_mapping`
  - `adversarial_nested_quotes`
  - The ast-grep Nix-grammar rule is also a hermetic fixture inside `review-nix`. A
    nixpkgs bump that drops the grammar fails the build instead of quietly emptying
    the collector.
- **Eval:**
  - `positive_by_name_attr_meta`
  - `negative_not_bwrap_skipped`
  - `adversarial_fetchurl_blocked`
  - `adversarial_ifd_blocked`
  - `adversarial_readfile_outside_blocked`
  - `adversarial_infinite_recursion_deadline`
  - `adversarial_attr_name_traversal_rejected`
  - `boundary_max_attrs`
