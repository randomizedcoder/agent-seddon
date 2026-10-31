# nix/lib/mk-review-check.nix
#
# Factory for the code-review coverage checks (nix/checks/review-*.nix). Every one
# of them reconstructs a git history, runs `agent --review base..head` against a
# model-free config, and greps the grounded context for a section — differing only
# in the history they build, the `[review]` collectors they enable, and the
# assertions. This renders the shared skeleton (the runCommand wrapper, the
# model-free `agent.toml`, the git-init preamble, `fail()`, the `$out` epilogue) so
# each check states only its unique parts.
#
# Curried: `import ./mk-review-check.nix { inherit pkgs agent; } { name; ...; }`.
#
# runCommand scripts are NOT shellchecked, so `epi`/other helper vars can sit unused.
{
  pkgs,
  agent,
}:
{
  # Check name → derivation `agent-review-<name>` (+ the section it exercises).
  name,
  # Extra `nativeBuildInputs` for the check's own setup/asserts (e.g. jq). Not seen by
  # the agent: it runs on its wrapper's PATH alone, so a review tool goes on
  # `agentRuntimePath` (nix/default.nix), not here.
  extraInputs ? [ ],
  # Extra lines under `[memory] backend = "file"` (e.g. `episodic_path = "$epi"`).
  memoryExtra ? "",
  # The collector toggle lines under `[review] backend = "local"`.
  reviewConfig,
  # Bash that builds the history + runs the review. Runs in a fresh `git init` repo
  # (`$cfg` = the config, `$wd` = the repo). Conventionally sets `ctx="$(agent
  # --config "$cfg" --review "$base..$head")"` for the asserts to grep.
  setup,
  # Bash assertions. `fail "msg"` is defined (prints + exits 1).
  asserts,
  # The success line written to `$out`.
  okMsg,
}:
pkgs.runCommand "agent-review-${name}"
  {
    nativeBuildInputs = [
      agent
      pkgs.git
      pkgs.coreutils
    ]
    ++ extraInputs;
  }
  ''
    export HOME="$(mktemp -d)"
    epi="$HOME/.agent/episodic.jsonl" # for memoryExtra/asserts that use it
    cfg="$HOME/agent.toml"
    # Unquoted heredoc so a `$epi`-style path in memoryExtra expands; nothing else in
    # the config carries a `$`.
    cat > "$cfg" <<TOML
    [agent]
    provider = "openai-compat"
    policy   = "auto-approve"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "none"
    api_key  = "none"
    [memory]
    backend = "file"
    ${memoryExtra}
    [search]
    auto_index = false
    [review]
    backend = "local"
    ${reviewConfig}
    [pool]
    members = []
    TOML

    # The agent runs with ONLY the wrapper's runtime PATH (an empty inherited PATH),
    # as a systemd service runs it: a helper the review needs (bash, agent-go-ast,
    # golangci-lint, shellcheck, …) must come from `agentRuntimePath`, never from this
    # builder's PATH, which always has bash and whatever `extraInputs` adds.
    agent() { PATH="" ${agent}/bin/agent "$@"; }

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main
    git config user.email t@e
    git config user.name t

    ${setup}

    fail() { echo "FAIL: $1" >&2; exit 1; }
    # Shared: no collector may report its tool missing from the agent's PATH (each
    # one fails soft with "… not found on PATH", which a section assert can miss).
    if printf '%s' "''${ctx:-}" | grep -q "not found on PATH"; then
      printf '%s\n' "''${ctx:-}" | grep "not found on PATH" >&2
      fail "a review tool is missing from the wrapped agent's runtime PATH"
    fi
    ${asserts}

    echo ${pkgs.lib.escapeShellArg okMsg} > "$out"
  ''
