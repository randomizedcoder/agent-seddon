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
  - The fix replaces the mirror refspec with heads only (`+refs/heads/*:refs/heads/*`,
    no `refs/pull/*`). PR heads are already fetched one at a time by `fetch_pr`. The fix
    also:
    - kills the whole git process tree on timeout;
    - adds a startup sweep for stale `tmp_pack_*` files;
    - caps the mirror's size with a metric (`agent_fleet_mirror_bytes`);
    - adds a one-off `git repack -a -d` for an existing bloated mirror.
- Two hypotheses to reproduce, each with a table-test row:
  - **(a)** the target branch (e.g. `staging`) was never fetched into the mirror, so the
    base doesn't resolve and the diff comes out empty;
  - **(b)** a fetch that times out partway leaves no usable merge base.
- Fix direction:
  - Fetch exactly the target branch ref (`fetch origin <target>`), not `--all`, which is
    cheap on a 3.6 GB mirror.
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

1. `ast-grep` with the tree-sitter-nix grammar (already on the runtime PATH) extracts
   indented strings bound to attributes matching `*Phase`, `pre*`, `post*`, `*Hook`,
   `script`, `shellHook`.
2. Each `${…}` antiquotation becomes a placeholder variable, e.g. `"$__nix_1"`, so shellcheck
   sees valid shell. Nix's `''$` escape becomes `$`.
3. Run `shellcheck -s bash -f json` on each snippet with a line-offset map.
4. Map each finding back to the `.nix` file and line. Drop codes that the placeholder
   substitution caused, a fixed list: SC2154 on `__nix_*`, plus SC2034 only when the variable
   is a placeholder.

This is a new `snippet` mode in `shellcheck.rs`. The existing collector's ban on inline
`# shellcheck disable` comments carries over.

**C/C++ suite** (`has_c`: `.c`, `.cc`, `.cpp`, `.cxx`, `.h`, `.hpp`, changed in the PR):

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
  - `--option max-call-depth` plus `ulimit -v`, and a wall-clock deadline
- **Attr selection.** Changed paths are mapped to attrs:
  - `pkgs/by-name/xx/<name>/` → `<name>`;
  - a `pkgs/top-level/all-packages.nix` hunk → its attr names.
  
  Each attr passes `safe_segment` and the count is capped at `nix_eval_max_attrs` (default 10).
- **What runs per attr:**
  - `--eval --strict -A <attr>.meta` (license, maintainers, platforms present?);
  - `-A <attr>.drvPath` (does it evaluate?);
  - `nixpkgs-hammering <attr>` (its eval-based checks).
  
  The output is parsed into `AnalysisFinding`s.
- **Daemon socket (spike first).** `.drvPath` writes `.drv` files through the nix daemon. The
  increment opens with a spike to prove `nix-instantiate` works inside bwrap with
  `--unshare-net` and the daemon socket (`/nix/var/nix/daemon-socket`) bound in, and to
  settle whether to use `--readonly-mode` for `--eval`-only runs. If the socket can't be made
  safe, the collector ships `--eval`-only, without `drvPath`.

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
- **Eval:**
  - `positive_by_name_attr_meta`
  - `negative_not_bwrap_skipped`
  - `adversarial_fetchurl_blocked`
  - `adversarial_ifd_blocked`
  - `adversarial_readfile_outside_blocked`
  - `adversarial_infinite_recursion_deadline`
  - `adversarial_attr_name_traversal_rejected`
  - `boundary_max_attrs`
