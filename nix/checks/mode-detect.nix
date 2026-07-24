# nix/checks/mode-detect.nix
#
# General mode-detection coverage (docs/design/adaptive-cognition/01-mode.md).
# Drives `agent --detect-mode "<prompt>"` — a thin offline surface that runs the
# classifier's deterministic prefilter (no pool, no model) and prints the verdict —
# and asserts each taxonomy cue maps to the right `TaskMode`.
#
# Fully offline: the config points the provider at an unreachable address and wires
# no `[pool]`, so only the free prefilter runs. No toolchain, no network — it runs
# in the hermetic `nix flake check` sandbox with just `agent`.
{
  pkgs,
  agent,
}:
pkgs.runCommand "agent-mode-detect"
  {
    nativeBuildInputs = [
      agent
      pkgs.coreutils
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    cfg="$HOME/agent.toml"
    cat > "$cfg" <<'TOML'
    [agent]
    provider = "openai-compat"
    policy   = "auto-approve"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "none"
    api_key  = "none"
    [memory]
    backend = "file"
    [search]
    auto_index = false
    [mode]
    classifier = "hybrid"
    TOML

    check() {
      local prompt="$1" want="$2"
      local out
      out="$(agent --config "$cfg" --detect-mode "$prompt")"
      echo "  $prompt => $out"
      case "$out" in
        "mode=$want "*) : ;;
        *) echo "FAIL: expected mode=$want for '$prompt', got: $out" >&2; exit 1 ;;
      esac
    }

    check "please review this pull request"   review
    check "implement a caching layer"          implement
    check "why does the build fail here"       debug
    check "design a schema for sessions"       design
    check "explain how the router works"       explain
    check "hello there"                        other

    echo "OK: mode detection classifies each taxonomy cue offline" > "$out"
  ''
