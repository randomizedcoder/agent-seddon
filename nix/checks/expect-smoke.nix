# nix/checks/expect-smoke.nix
#
# Hermetic, model-free smoke test of the tcl/expect capability harness. It drives
# the REAL `agent` REPL over a pty (via test/expect/repl_smoke.exp) using only
# slash commands — none of which contact the provider — so it runs in the
# `nix flake check` sandbox with an unreachable model configured.
#
# Its purpose is to keep the tcl/expect harness itself honest in CI: if the
# shared procs (test/expect/lib.exp) or the binary's REPL contract regress, this
# fails here rather than silently breaking the opt-in live tier
# (`nix run .#e2e-expect`, which needs a real model and cannot run in the gate).
#
# Fully offline: the provider points at an unreachable address and slash commands
# never call it. Needs `expect` (for the pty) and `agent`; nothing else.
{
  pkgs,
  agent,
}:
pkgs.runCommand "agent-expect-smoke"
  {
    nativeBuildInputs = [
      agent
      pkgs.expect
      pkgs.coreutils
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    # Match on text, not terminal control bytes (mirrors repl_pty.rs).
    export TERM=dumb
    export NO_COLOR=1

    cfg="$HOME/agent.toml"
    cat > "$cfg" <<'TOML'
    [agent]
    provider = "openai-compat"
    policy   = "auto-approve"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "none"
    api_key  = "none"
    [tools]
    enabled = ["read_file", "write_file", "ls"]
    [memory]
    backend = "file"
    [search]
    auto_index = false
    TOML

    echo "expect-smoke: driving the REPL model-free over a pty ..."
    expect ${../../test/expect}/repl_smoke.exp "$cfg"

    echo "OK: the tcl/expect harness drives the real agent REPL in the sandbox" > "$out"
  ''
