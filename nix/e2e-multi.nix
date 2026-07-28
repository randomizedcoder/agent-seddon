# `nix run .#e2e-multi` — concurrent multi-session e2e.
#
# Launches N agent sessions AT THE SAME TIME (default 10), each writing a small program
# (hello-world or FizzBuzz, round-robin across C / Go / Rust) via the agent's tools; the
# harness compiles + runs each and asks a strong external judge model (GLM-5.2 by
# default) to grade correctness as a final check. The multi-turn REPL companion is
# `nix run .#e2e-expect`; the single-shot one is `nix run .#e2e-live`.
#
# Not a hermetic check: it needs a running generator model (ollama), the judge endpoint,
# and a network socket — none of which the `nix flake check` sandbox has. The harness
# logic lives in test/e2e-multi/run.sh so it also runs from the dev shell.
#
# Env knobs (all optional): AGENT_E2E_SESSIONS, AGENT_E2E_{BASE_URL,MODEL,API_KEY}
# (generator), AGENT_E2E_JUDGE{,_BASE_URL,_MODEL,_API_KEY_FILE,_INSECURE_TLS}.
{
  pkgs,
  lib,
  versions,
  agent,
}:
pkgs.writeShellApplication {
  name = "e2e-multi";
  runtimeInputs = [
    agent
    pkgs.gcc # cc for the C task
    versions.go # go for the Go task
    versions.rustToolchain # rustc for the Rust task
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gnused
  ];
  text = ''
    exec bash "${../test/e2e-multi}/run.sh" "$@"
  '';
}
