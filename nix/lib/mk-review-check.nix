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
  # Extra `nativeBuildInputs` beyond `agent`/`git`/`coreutils` (e.g. go-ast, a linter).
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

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main
    git config user.email t@e
    git config user.name t

    ${setup}

    fail() { echo "FAIL: $1" >&2; exit 1; }
    ${asserts}

    echo ${pkgs.lib.escapeShellArg okMsg} > "$out"
  ''
