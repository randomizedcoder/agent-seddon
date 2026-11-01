# nix/checks/agent-runtime-path.nix
#
# Every program the agent runs must come from the wrapped agent's OWN `PATH`
# (`agentRuntimePath` in nix/default.nix, the makeWrapper `--prefix PATH`), because
# that is all a systemd service (`services.agentSeddon`) or the k8s image gives it.
# A stdenv builder always has bash and whatever a check adds, so a gap there is
# invisible to builder-PATH checks: l2's fleet reported every call graph as
# `agent-go-ast not found on PATH` because `bash` (the Sandbox runs `bash -c`) was
# missing, and shellcheck, gcc (cgo for `go test -race`) and cargo (cargo-deny's
# `cargo metadata`) were missing too, the last two failing silently.
#
# Table-driven: each row is a program the code runs, why, and whether it must
# resolve. Probed under `env -i` with only the wrapper's PATH. The `negative_` rows
# are the check-the-check: a probe that leaked the builder's PATH would find them.
# A `builder_only` row must first resolve on this builder's own PATH, so the row
# proves the leak would be seen rather than passing because nothing provides the name
# (the old `makeWrapper` row: a setup-hook function, never a binary, so it could not fail).
#
# Operator-configured programs are out of scope (LSP servers, the non-Go SCIP
# indexers, MCP stdio servers): they are off by default and named in config.
{
  pkgs,
  agent,
}:
let
  # name, description, program, expected ("found" | "absent" | "builder_only").
  cases = [
    {
      name = "positive_bash";
      description = "the Sandbox runs every ExecSpec::sh command as `bash -c`";
      program = "bash";
      expected = "found";
    }
    {
      name = "positive_agent_go_ast";
      description = "the review call-graph collector";
      program = "agent-go-ast";
      expected = "found";
    }
    {
      name = "positive_agent_go_graph";
      description = "the AstBackend `go` engine";
      program = "agent-go-graph";
      expected = "found";
    }
    {
      name = "positive_scip_go";
      description = "the AstBackend `scip` engine's built-in Go indexer";
      program = "scip-go";
      expected = "found";
    }
    {
      name = "positive_ast_grep";
      description = "the structural_search tool";
      program = "ast-grep";
      expected = "found";
    }
    {
      name = "positive_git";
      description = "agent-git, repo index, review collectors";
      program = "git";
      expected = "found";
    }
    {
      name = "positive_rg";
      description = "the grep tool and the search index";
      program = "rg";
      expected = "found";
    }
    {
      name = "positive_golangci_lint";
      description = "review analyzer, Go lint";
      program = "golangci-lint";
      expected = "found";
    }
    {
      name = "positive_gosec";
      description = "review analyzer, Go security";
      program = "gosec";
      expected = "found";
    }
    {
      name = "positive_go";
      description = "go vet, go test (analyzer + go_checks), golangci-lint's toolchain";
      program = "go";
      expected = "found";
    }
    {
      name = "positive_gofmt";
      description = "review analyzer, Go formatting";
      program = "gofmt";
      expected = "found";
    }
    {
      name = "positive_gcc";
      description = "cgo for `go test -race`; without it the race run fails to build";
      program = "gcc";
      expected = "found";
    }
    {
      name = "positive_cargo_audit";
      description = "review analyzer, RustSec advisories";
      program = "cargo-audit";
      expected = "found";
    }
    {
      name = "positive_cargo_deny";
      description = "review analyzer, banned deps and sources";
      program = "cargo-deny";
      expected = "found";
    }
    {
      name = "positive_cargo";
      description = "cargo-deny runs `cargo metadata`; without it it reports nothing";
      program = "cargo";
      expected = "found";
    }
    {
      name = "positive_shellcheck";
      description = "the review shellcheck collector";
      program = "shellcheck";
      expected = "found";
    }
    {
      name = "positive_nix";
      description = "the nix-run tool provider and the nix Sandbox backend";
      program = "nix";
      expected = "found";
    }
    {
      name = "positive_bwrap";
      description = "the bwrap Sandbox backend";
      program = "bwrap";
      expected = "found";
    }
    {
      name = "corner_awk";
      description = "shell basics for the model's bash tool: not on a systemd PATH";
      program = "awk";
      expected = "found";
    }
    {
      name = "corner_diff";
      description = "shell basics: diff";
      program = "diff";
      expected = "found";
    }
    {
      name = "corner_patch";
      description = "shell basics: patch";
      program = "patch";
      expected = "found";
    }
    {
      name = "corner_tar";
      description = "shell basics: tar";
      program = "tar";
      expected = "found";
    }
    {
      name = "corner_gzip";
      description = "shell basics: gzip";
      program = "gzip";
      expected = "found";
    }
    {
      name = "corner_make";
      description = "shell basics: make";
      program = "make";
      expected = "found";
    }
    {
      name = "boundary_coreutils";
      description = "coreutils come from the wrapper too, not from the host";
      program = "ls";
      expected = "found";
    }
    {
      name = "negative_unknown_program";
      description = "a program nothing provides must not resolve (the probe is not leaking the builder PATH)";
      program = "agent-seddon-no-such-tool";
      expected = "absent";
    }
    {
      name = "negative_builder_only_tool";
      description = "GNU hello is on this builder's PATH only; the probe must not see it";
      program = "hello";
      expected = "builder_only";
    }
    {
      name = "adversarial_path_traversal_name";
      description = "a program name with a slash is not looked up on PATH at all";
      program = "../bin/bash";
      expected = "absent";
    }
  ];
  rows = pkgs.lib.concatMapStrings (
    c: "${c.name}\t${c.program}\t${c.expected}\t${c.description}\n"
  ) cases;
in
pkgs.runCommand "agent-runtime-path"
  {
    # `hello` is the builder-only program the negative row looks for.
    nativeBuildInputs = [ pkgs.hello ];
    inherit rows;
    passAsFile = [ "rows" ];
  }
  ''
    set -eu
    # The PATH the agent really gets: run the wrapper's own PATH setup from an empty
    # environment, printing PATH where it would exec the binary.
    sed 's/^exec .*/printf "%s" "$PATH"/' ${agent}/bin/agent > wrapper-path.sh
    grep -q '^printf' wrapper-path.sh || { echo "FAIL: the agent wrapper has no exec line" >&2; exit 1; }
    rp="$(${pkgs.coreutils}/bin/env -i ${pkgs.runtimeShell} wrapper-path.sh)"
    [ -n "$rp" ] || { echo "FAIL: the agent wrapper sets no PATH" >&2; exit 1; }

    failed=0
    while IFS="$(printf '\t')" read -r name program expected description; do
      [ -n "$name" ] || continue
      if [ "$expected" = builder_only ]; then
        command -v -- "$program" >/dev/null || {
          echo "FAIL $name: $program is not on the builder's PATH, so the row proves nothing — $description" >&2
          failed=1
          continue
        }
        expected=absent
      fi
      # `env -i` + only the wrapper's PATH; bash itself must come from it.
      if ${pkgs.coreutils}/bin/env -i PATH="$rp" bash -c 'case "$1" in */*) exit 1;; esac; command -v -- "$1" >/dev/null' _ "$program" 2>/dev/null; then
        got=found
      else
        got=absent
      fi
      if [ "$got" = "$expected" ]; then
        echo "ok   $name: $program $got"
      else
        echo "FAIL $name: $program is $got, expected $expected — $description" >&2
        failed=1
      fi
    done < "$rowsPath"
    [ "$failed" = 0 ] || exit 1

    echo "OK: every program the agent runs is on its own runtime PATH" > "$out"
  ''
