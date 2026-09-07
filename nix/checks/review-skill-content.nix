# nix/checks/review-skill-content.nix
#
# Gates the *shipped* review-skill content (review-fleet increment 5b, C11): the
# checklist fragments under `prompts/modes/review/` and `skills/code-review/SKILL.md`.
#
# These are markdown (not cargo sources), so they are deliberately outside the crane
# source filter (`nix/default.nix`) and never reach the `test`/`coverage` sandboxes —
# a cargo test cannot read them. This check references them by nix path instead, so
# the files are copied into the store independently of that filter, and asserts the
# behaviour-bearing content is present + correctly ordered. The loop's fragment
# *selection* mechanism is unit-tested in `agent-context` (`system_fragments`); the
# roster→session skill bridge in `agent-runtime` (`seed_review`/`prompt_context`).
{
  pkgs,
  # `agent` is threaded to every agentCheck; unused here (pure content grep).
  agent ? null,
}:
let
  fragments = ../../prompts/modes/review;
  skill = ../../skills/code-review/SKILL.md;
in
pkgs.runCommand "agent-review-skill-content"
  {
    nativeBuildInputs = [ pkgs.coreutils ];
  }
  ''
    fail() { echo "FAIL: $1" >&2; exit 1; }

    frag=${fragments}
    # Exactly the six ordered checklist fragments, NNNN_-prefixed for deterministic order.
    for n in 0001_grounding 0002_objective_and_tone 0003_idioms_and_dry \
             0004_tests_and_security 0005_mechanized_checks 0006_output_order; do
      test -f "$frag/$n.md" || fail "missing fragment $n.md"
    done

    # Each fragment carries its behaviour-bearing content (the user's checklist).
    grep -qi "reviewing a change" "$frag/0001_grounding.md"        || fail "grounding lost"
    grep -qi "friendly"           "$frag/0002_objective_and_tone.md" || fail "tone lost"
    grep -qi "DRY"                "$frag/0003_idioms_and_dry.md"    || fail "DRY lost"
    grep -qi "table-driven"       "$frag/0004_tests_and_security.md" || fail "tests-are-table-driven lost"
    grep -qi "untrusted"          "$frag/0004_tests_and_security.md" || fail "security bar lost"
    grep -qi "shellcheck"         "$frag/0005_mechanized_checks.md" || fail "shellcheck item lost"
    grep -qi "race"               "$frag/0005_mechanized_checks.md" || fail "go race/bench item lost"
    # The OS-silence rule is load-bearing (reviews must not name the OS/toolchain).
    # (grep a single-line substring: the full sentence wraps across a line break.)
    grep -qi "operating system" "$frag/0005_mechanized_checks.md" \
      || fail "OS-silence rule lost"
    grep -qi "Order the review"   "$frag/0006_output_order.md"      || fail "output-order lost"

    # The code-review SKILL.md: discoverable frontmatter + the mechanizable checklist.
    sk=${skill}
    grep -q "^name: code-review$" "$sk" || fail "SKILL.md missing 'name: code-review' frontmatter"
    grep -qi "^description:"      "$sk" || fail "SKILL.md missing description"
    grep -qi "shellcheck"        "$sk" || fail "SKILL.md missing shellcheck item"
    grep -qi "no ignores"        "$sk" || fail "SKILL.md missing no-ignores rule"
    grep -qi "table-driven"      "$sk" || fail "SKILL.md missing table-driven tests item"

    echo "OK: review-skill fragments + code-review SKILL.md present, ordered, and complete" > "$out"
  ''
