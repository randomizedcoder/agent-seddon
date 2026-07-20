# Parity: Skills system (`SKILL.md` discovery + loading)

Per-feature parity spec for the **skills** seam — reusable, on-demand instruction
snippets discovered from `SKILL.md` files and injected into the conversation via
progressive disclosure.

> **Status: implemented (in-scope gaps).** `crates/agent-runtime/src/skills.rs`
> now discovers dir-based `SKILL.md` **recursively** (skipping hidden dirs like
> `.git`/`.venv`, preferring a directory's root `SKILL.md`), tolerates a **UTF-8
> BOM**, and falls back to the body's first prose line for a missing
> **description** — 36 tests total (discovery/recursion/hidden-skip, first-wins
> precedence, `find` name-safety, frontmatter robustness). `find` traversal-safety
> is **structural** (it matches discovered *names*, never using them as paths), so
> the security cases pin that rather than an added guard. Deferred (larger design
> directions, §4 gaps 2/3/7): a **model-invocable `skill` tool**, per-skill
> permission filtering, and remote/URL sources — the notable remaining gap is that
> skills are user-driven (`/skill:<name>`), not model-selected. No bench/leak:
> discovery is light sync I/O with trivial parsing; skills are runtime-internal
> (no gRPC seam).

## 1. Feature & why it matters

A *skill* is a `SKILL.md` file carrying YAML-ish frontmatter (`name`,
`description`) plus a markdown body of instructions. The agent lists the
available skills (name + one-line description only — cheap to keep resident) and
loads a chosen skill's **body** on demand, so a large procedure ("fill a PDF
form", "cut a release") only costs context when it's actually needed. This is the
now-standard *progressive disclosure* pattern: the model (or user) sees a menu of
capabilities, and pays the token cost of the full instructions for exactly the
one it picks.

Because skills are just files on disk, they are the lowest-friction extension
point in the whole system — a user drops a `SKILL.md` into a directory and it is
immediately discoverable, with no code, no recompile, and no plugin registration.
That same "just run whatever text is in a file" property is why skills are also a
**security surface**: a skill name or a referenced file path is attacker-controlled
input, and every peer here has had to harden discovery/loading against path
traversal, absolute-path escapes, cross-origin fetches, symlink escapes, and
name-collision shadowing.

## 2. agent-seddon today

Skills live in [`crates/agent-runtime/src/skills.rs`](../../crates/agent-runtime/src/skills.rs).
The surface is small and file-only:

- `SkillInfo { name, description, path }` — a discovered skill's metadata + where
  to load its body from.
- `default_dirs()` → `["skills", ".agent/skills"]`, searched in order.
- `discover(dirs)` — walks each directory (non-recursively). An entry is a skill
  if it is a subdir containing `SKILL.md`, or a flat `<name>.md` file. Deduped by
  `name` (first-wins across dirs), then sorted by name.
- `find(dirs, name)` — `discover` + linear `name ==` match.
- `load_body(path)` — reads the file, returns the body after the frontmatter,
  trimmed.
- `split_frontmatter(content)` — splits `---\n<front>\n---\n<body>`; tolerates a
  closing `---` at EOF with no trailing newline; no frontmatter ⇒ `("", whole)`.
- `field(front, key)` — first-match `key: value` reader, trims quotes; empty
  value ⇒ `None` (so a name/description falls back).

Wiring is **user-driven only**, in the REPL
([`crates/agent-cli/src/repl.rs`](../../crates/agent-cli/src/repl.rs)): `/skills`
lists (`list_skills`), and `/skill:<name>` or `/skill <name>` loads a body into
the session (`load_skill`). There is no `Tool` for skills — the model cannot
invoke one on its own.

Existing tests (in-module `#[cfg(test)]`, `rstest`, `agent_testkit::tempdir`):

- `split_frontmatter_cases` — 5 `#[case]`s: with-frontmatter, no-frontmatter,
  unterminated, EOF-close, empty.
- `field_cases` — 7 `#[case]`s: plain, double-quoted, single-quoted, extra
  whitespace, first-match-wins, missing, empty-value.
- `parses_frontmatter_and_body`, `discovers_dir_and_flat_skills` (dir + flat,
  sorted, `find` + `load_body`), `missing_dirs_are_ignored`.

**Design gaps vs. the peers (deliberately called out):**

- **(a) User-loaded only.** Skills reach context only through the human typing
  `/skill:<name>`. There is no model-invocable `skill` tool the way opencode has
  (`tool/skill`), and no "advertise the skill menu into the system prompt" the way
  pi does (`formatSkillsForPrompt`). The model can't self-select a skill.
- **(b) Local directory only, unhardened.** Discovery is a shallow read of two
  fixed local dirs. There is **no** URL/remote source, no embedded/bundled source,
  no per-agent permission filtering, and no path-traversal / absolute-path /
  symlink hardening on the skill `name` or on any referenced file. A skill named
  `../../etc` or a `find(name)` that walks outside the roots is neither rejected
  nor tested; name collisions across dirs are silently resolved first-wins with no
  warning.

## 3. Peer implementations & their tests

| Peer | Impl path | Test path | Framework |
| --- | --- | --- | --- |
| opencode (discovery) | `packages/core/src/skill/discovery.ts` | `packages/core/test/skill-discovery.test.ts` | bun:test + Effect |
| opencode (tool) | `packages/core/src/tool/skill.ts` | `packages/core/test/tool-skill.test.ts` | bun:test + Effect |
| pi | `packages/coding-agent/src/core/skills.ts` | `packages/coding-agent/test/skills.test.ts` | vitest |
| hermes-agent (tool) | `tools/skills_tool.py` | `tests/tools/test_skills_tool.py` | pytest |
| hermes-agent (guard) | `tools/skills_guard.py` | `tests/tools/test_skills_guard.py` | pytest |

**opencode — `SkillDiscovery.pull` (remote catalog fetch, security-first):**

- Rejects a **skill-name** traversal (`name: "../outside"`) *without fetching any
  files* — only `index.json` is requested, cache dir stays empty.
- Rejects a **file** traversal (`files: ["../outside.md"]`) without fetching.
- Rejects an **absolute** file path (`"/tmp/outside.md"`) without fetching.
- Rejects a **cross-origin** file URL (`"https://evil.example.test/..."`) without
  fetching.
- Downloads **safe nested** files under the skill root (`references/guide.md`) and
  writes them to the expected on-disk layout.
- **Refreshes** cached files when the skill `version` changes; a same-version
  re-pull requests only `index.json` (cache hit).
- Publishes **complete** updates and **removes stale** files (`old.md` deleted on a
  later version that no longer lists it); a partial/failed pull leaves the previous
  version intact (**partial-failure resilience**).

**opencode — `SkillTool` (model-invocable):**

- Lists available skills, **authorizes** the selected name through the permission
  seam (`assert({ action: "skill", resources: ["effect"] })`), and returns
  model-facing content = frontmatter + body + "Base directory for this skill: …".
- Missing skill → graceful `{ type: "error", value: "Unable to load skill missing" }`.
- **Permission denied** → same graceful error (`Unable to load skill effect`), not a crash.
- A flat (non-dir) skill loads with no linked reference files.

**pi — `loadSkillsFromDir` / `loadSkills` / `formatSkillsForPrompt`:**

- Loads a **valid** skill; `name` **need not match** its parent directory (no warning).
- **Warns** on a name with invalid characters; warns on a name **> 64 chars**.
- **Warns and skips** a skill whose `description` is missing; likewise a file with
  **no frontmatter** (missing description) is skipped.
- **Ignores unknown** frontmatter fields (no warning).
- Loads **nested** skills recursively; **prefers a directory's root `SKILL.md`**
  over nested ones.
- **Warns and skips** invalid YAML (diagnostic cites a line); **preserves multiline**
  descriptions.
- **Collision handling:** same name across two dirs → keep first, emit a
  `name collision` warning.

**hermes-agent — `skills_tool` (recursive discovery + view):**

- **Recursive** `.md` discovery; skips `.git/` and nested `.venv/…/site-packages`
  trees; follows **symlinked** category dirs.
- YAML frontmatter incl. a leading **UTF-8 BOM**; **description-from-body** fallback
  (first non-heading line) when frontmatter has none; long descriptions truncated.
- **Categorization** by subdir (`mlops/axolotl` → category `mlops`).
- Metadata/tag parsing accepts a **list**, a **comma-separated** string, or a
  **`[bracket, wrapped]`** string; strips quotes; filters empties.
- `skill_view` resolves by **frontmatter name even when the dir differs**; refuses
  an **ambiguous** name across local + external dirs (both paths surfaced); a
  reference `<skill>.md` under `references/` is **not** treated as a real skill.
- **guard** (`skills_guard`): `is_relative_to` symlink-escape check (catches the
  `axolotl` vs `axolotl-backdoor` prefix-confusion bug), invisible-unicode /
  exfil / traversal scanning, trust levels gating install.

## 4. Completeness gaps

Relative to the peers, agent-seddon is missing:

1. **Path-traversal / escape hardening (highest priority).** `find(dirs, name)`
   and any future file loader accept an arbitrary `name`. A `../` name, an
   absolute path, or a symlinked `SKILL.md` that resolves outside the skill root
   is neither rejected nor tested. Peers (opencode, hermes guard) treat this as a
   security invariant. The tools crate already has `resolve_within`
   ([`agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs)) — the same
   discipline should apply here.
2. **Model-invocable skill loading.** No `skill` `Tool`; skills can't be
   self-selected by the model (opencode `SkillTool`), and the skill menu is never
   surfaced into the system prompt (pi `formatSkillsForPrompt`).
3. **Per-skill permission filtering.** No `Policy`/permission gate on which skills
   a given agent may load (opencode authorizes each `skill` action).
4. **Precedence & collision reporting.** First-wins dedup is silent; peers warn
   (pi) or refuse ambiguous names (hermes).
5. **Recursive discovery + root-preference.** Discovery is one level deep; peers
   recurse and prefer a directory's root `SKILL.md` over nested ones, while
   skipping `.git`/`.venv`.
6. **Robust frontmatter.** No UTF-8 BOM tolerance, no description-from-body
   fallback, no invalid-YAML diagnostic (the naive parser silently yields empty
   fields).
7. **Alternate sources.** Local-dir only — no remote/URL catalog (opencode
   `pull`) and no embedded skills.

Gaps 2, 3, and 7 are larger design directions; gaps 1, 4, 5, 6 are addressable
within the current file-only model and are where the test plan below concentrates.

## 5. Table-driven test plan

Target file: `crates/agent-runtime/src/skills.rs` (extend the existing in-module
`#[cfg(test)] mod tests`, matching the surrounding `rstest` style — see the sibling
[`agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs) `edit_cases`).

Doubles: no seam doubles needed — skills tests are on-disk. Use
`agent_testkit::tempdir()` (already re-exported as the local `tempdir()` helper) and
write real `SKILL.md` fixtures into it. Case-name prefixes follow the house style:
`positive_` / `negative_` / `corner_` / `boundary_` / `security_`. Ports of a
concrete peer case are tagged `(port: <peer>)`; behaviours with no current impl are
`(new: agent-seddon)` and codify the intended hardening.

```rust
// crates/agent-runtime/src/skills.rs  (in `mod tests`)

// --- discovery: what counts as a skill, and precedence -----------------------
#[rstest]
// (each case seeds one or more SKILL.md fixtures under a tempdir; asserts the
//  discovered `name`s in order)
#[case::positive_dir_skill(&[("pdf/SKILL.md", "---\nname: pdf\ndescription: d\n---\nb")], &["pdf"])]                        // (port: pi valid-skill)
#[case::positive_flat_md(&[("changelog.md", "---\nname: changelog\ndescription: d\n---\nb")], &["changelog"])]              // existing behaviour
#[case::positive_sorted(&[("b/SKILL.md","---\nname: b\ndescription: d\n---\n"),("a/SKILL.md","---\nname: a\ndescription: d\n---\n")], &["a","b"])]
#[case::corner_name_neednt_match_dir(&[("alias/SKILL.md","---\nname: real\ndescription: d\n---\n")], &["real"])]            // (port: pi name-mismatch / hermes dir-differs)
#[case::boundary_missing_description_still_listed(&[("x/SKILL.md","---\nname: x\n---\nbody")], &["x"])]                      // NOTE: pi *skips* these; document our choice
#[case::negative_no_skill_md(&[("nota/README.md","hello")], &[])]                                                           // subdir without SKILL.md
fn discover_cases(#[case] files: &[(&str, &str)], #[case] expected_names: &[&str]) { /* … */ }

// --- discovery: dirs we must NOT descend into, and recursion -----------------
#[rstest]
#[case::security_skips_git_dir(".git/evil/SKILL.md", false)]                    // (port: hermes skips_git_directories) (new: agent-seddon)
#[case::security_skips_venv(".venv/lib/site-packages/x/SKILL.md", false)]       // (port: hermes skips nested venv)   (new: agent-seddon)
#[case::positive_nested_recursed("group/child/SKILL.md", true)]                 // (port: pi nested)                  (new: agent-seddon)
fn discovery_scope_cases(#[case] rel: &str, #[case] should_discover: bool) { /* … */ }

// --- precedence: first-wins across dirs, reported ----------------------------
#[rstest]
#[case::positive_first_dir_wins("skills", ".agent/skills", "skills")]           // dir order decides the winner
#[case::corner_root_skill_preferred_over_nested("root", "nested", "root")]      // (port: pi root-skill-preferred) (new: agent-seddon)
fn precedence_cases(#[case] dir_a: &str, #[case] dir_b: &str, #[case] winner_source: &str) { /* asserts collision is deduped AND surfaced */ }

// --- name resolution hardening (find) : the security core --------------------
#[rstest]
#[case::positive_plain("pdf", true)]
#[case::security_parent_traversal("../outside", false)]                         // (port: opencode reject skill-name traversal) (new: agent-seddon)
#[case::security_deep_traversal("../../etc/passwd", false)]                     // (new: agent-seddon)
#[case::security_absolute_path("/etc/passwd", false)]                          // (port: opencode reject absolute paths)      (new: agent-seddon)
#[case::security_embedded_slash("a/../b", false)]                              // (new: agent-seddon)
#[case::negative_unknown("nope", false)]
fn find_name_safety_cases(#[case] name: &str, #[case] resolves: bool) {
    // seed one real skill "pdf" under tempdir; `find(dirs, name)` must return the
    // skill ONLY for the safe in-tree name, and never a path outside the roots.
}

// --- referenced-file loading hardening (future load_file(skill, rel)) --------
#[rstest]
#[case::positive_nested_ref("references/guide.md", true)]                       // (port: opencode safe nested files / hermes view_reference_file) (new: agent-seddon)
#[case::security_ref_traversal("../outside.md", false)]                        // (port: opencode reject file traversal)   (new: agent-seddon)
#[case::security_ref_absolute("/tmp/outside.md", false)]                       // (port: opencode reject absolute)         (new: agent-seddon)
#[case::security_ref_symlink_escape("escape.md", false)]                       // (port: hermes guard symlink_escape via is_relative_to) (new: agent-seddon)
fn skill_file_safety_cases(#[case] rel: &str, #[case] allowed: bool) { /* resolve_within the skill root */ }

// --- frontmatter robustness --------------------------------------------------
#[rstest]
#[case::corner_utf8_bom("\u{feff}---\nname: x\ndescription: d\n---\nbody", Some("x"), Some("d"))] // (port: hermes utf8_bom) (new: agent-seddon)
#[case::corner_description_from_body("---\nname: x\n---\n# H\n\nFirst para.\n", Some("x"), Some("First para."))] // (port: hermes desc-from-body) (new: agent-seddon)
#[case::negative_no_frontmatter("just a body\n", None, None)]                   // (port: pi skip files without frontmatter)
#[case::corner_ignore_unknown_field("---\nname: x\ndescription: d\nweird: q\n---\nb", Some("x"), Some("d"))] // (port: pi unknown-field)
fn frontmatter_cases(#[case] content: &str, #[case] name: Option<&str>, #[case] desc: Option<&str>) { /* … */ }
```

Doubles/fixtures note: `security_*_symlink_*` cases create a real symlink with
`std::os::unix::fs::symlink` and should `#[cfg_attr(windows, ignore)]` (mirrors
hermes' `_can_symlink()` skip). The traversal cases assert on the *resolved* path
staying under the skill root, not on string prefixes — the hermes
`is_relative_to` regression proves why `starts_with` is insufficient.

## 6. References

- Impl: [`crates/agent-runtime/src/skills.rs`](../../crates/agent-runtime/src/skills.rs)
- REPL wiring: [`crates/agent-cli/src/repl.rs`](../../crates/agent-cli/src/repl.rs) (`list_skills`, `load_skill`)
- House rstest style: [`crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs) (`edit_cases`), [`crates/agent-tools/src/lib.rs`](../../crates/agent-tools/src/lib.rs) (`resolve_within`)
- Test doubles + `tempdir()`: [`crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs)
- Component docs (tone reference): [`docs/components/tools.md`](../components/tools.md)
- Peers:
  - opencode discovery — `opencode/packages/core/test/skill-discovery.test.ts`
  - opencode tool — `opencode/packages/core/test/tool-skill.test.ts`
  - pi — `pi/packages/coding-agent/test/skills.test.ts`
  - hermes tool — `hermes-agent/tests/tools/test_skills_tool.py`
  - hermes guard — `hermes-agent/tests/tools/test_skills_guard.py`
