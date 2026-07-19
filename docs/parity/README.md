# Coding-fundamentals parity specs

Per-feature specs for the **top-10 coding fundamentals**, each measuring
agent-seddon against the three reference harnesses — **pi**, **hermes-agent**, and
**opencode** — with a focus on *tests*. Every doc mines the peers' own test suites
and lays out a table-driven `#[rstest]` plan to **match and exceed** them.

These are **specs, not implementations**: they describe the target behaviour and
the test tables to write, but change no code. They complement the high-level
[`../features-comparison.md`](../features-comparison.md) (the matrix + deep dive)
with execution-ready detail, and the per-seam docs under
[`../components/`](../components/).

Each doc follows the same six sections: *Feature & why it matters · agent-seddon
today · Peer implementations & their tests · Completeness gaps · Table-driven test
plan · References.*

| # | Spec | Feature | agent-seddon status |
|---|------|---------|---------------------|
| 1 | [01-code-editing.md](01-code-editing.md) | `edit` (surgical string replace) | Strong (7 cases); no CRLF/BOM/fuzzy/multi-edit/stale guard |
| 2 | [02-patch-diff-editing.md](02-patch-diff-editing.md) | `apply_patch` (unified diff) | **Missing** — the headline gap |
| 3 | [03-file-read-write.md](03-file-read-write.md) | `read_file` / `write_file` | Works; **0 direct tests** |
| 4 | [04-shell-bash.md](04-shell-bash.md) | `bash` shell execution | Works; **0 direct tests** |
| 5 | [05-text-search.md](05-text-search.md) | `grep` / `find` / `ls` | Good (14 cases); extend to gitignore/injection/context edges |
| 6 | [06-tool-calling-loop.md](06-tool-calling-loop.md) | tool dispatch loop + registry | **No direct loop unit tests** (only gRPC roundtrip) |
| 7 | [07-skills.md](07-skills.md) | skills (SKILL.md) | Good (12 cases); user-loaded only, no traversal hardening |
| 8 | [08-permissions-policy.md](08-permissions-policy.md) | `Policy` approval seam | **0 tests** |
| 9 | [09-context-compaction.md](09-context-compaction.md) | context assembly + compaction | Good (~18 cases); harden boundaries + summarizer-fallback |
| 10 | [10-memory.md](10-memory.md) | memory recall + safety | Good (25 cases); no injection scan / embedding recall |

**Conventions** (see any doc's test plan): `#[rstest]` + `#[case::name]` tables
with `positive_`/`negative_`/`corner_`/`boundary_` prefixes, modelled on
[`../../crates/agent-tools/src/edit.rs`](../../crates/agent-tools/src/edit.rs);
test doubles from
[`../../crates/agent-testkit/src/lib.rs`](../../crates/agent-testkit/src/lib.rs);
gate stays `nix flake check` (clippy `-D warnings` + rustfmt + `cargo test`).

Peer sources are read-only clones under `/home/das/Downloads/{pi,hermes-agent,opencode}`.
