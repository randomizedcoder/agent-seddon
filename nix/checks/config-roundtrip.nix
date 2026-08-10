# nix/checks/config-roundtrip.nix
#
# Config-roundtrip coverage (mirrors hermes' `nix/checks.nix` config-roundtrip):
# feed representative `agent.toml` fixtures through the REAL loader via
# `agent --check-config` — which parses the config and BUILDS the agent, so every
# selected seam impl must resolve to a registered factory — then assert the printed
# selections, and that a broken config fails closed.
#
# `--check-config` is a dry run: it prints the chosen impls and exits 0 before any
# model call, socket, or REPL. So this runs fully offline in the hermetic
# `nix flake check` sandbox with just `agent`. It catches config-schema drift (a
# renamed field, a selector with no factory) that no in-process test sees, because
# only the shipped binary wires config → registry → agent.
{
  pkgs,
  agent,
}:
pkgs.runCommand "agent-config-roundtrip"
  {
    nativeBuildInputs = [
      agent
      pkgs.coreutils
    ];
  }
  ''
    export HOME="$(mktemp -d)"
    work="$HOME/work"
    mkdir -p "$work"

    # A config that must load AND build, then match the expected selections.
    # $1 = fixture name, $2 = file, rest = "key = value" substrings to require.
    expect_ok() {
      local name="$1" cfg="$2"; shift 2
      local out
      if ! out="$(agent --config "$cfg" --check-config 2>"$HOME/$name.err")"; then
        echo "FAIL($name): a valid config must check clean; stderr:" >&2
        cat "$HOME/$name.err" >&2
        exit 1
      fi
      echo "--- $name ---"; echo "$out"
      for want in "$@"; do
        case "$out" in
          *"$want"*) : ;;
          *) echo "FAIL($name): expected '$want' in the selections, got:" >&2
             echo "$out" >&2; exit 1 ;;
        esac
      done
    }

    # A config that must FAIL closed (nonzero, no selections banner).
    expect_fail() {
      local name="$1" cfg="$2"
      if agent --config "$cfg" --check-config >"$HOME/$name.out" 2>&1; then
        echo "FAIL($name): a broken config must not check clean:" >&2
        cat "$HOME/$name.out" >&2
        exit 1
      fi
      echo "--- $name (correctly rejected) ---"
    }

    # 1) The shipped default shape: openai-compat provider, file memory.
    cat > "$HOME/default.toml" <<TOML
    [agent]
    provider    = "openai-compat"
    context     = "sliding-window"
    policy      = "auto-approve"
    working_dir = "$work"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "none"
    api_key  = "none"
    [memory]
    backend = "file"
    [tools]
    enabled = ["read_file", "write_file", "ls"]
    [metrics]
    enabled = false
    [search]
    auto_index = false
    [git]
    auto_fetch_secs = 0
    TOML
    expect_ok default "$HOME/default.toml" \
      "config: OK" \
      "provider  = openai-compat" \
      "policy    = auto-approve" \
      "memory    = file" \
      "tokenizer = approx" \
      "tools     = 3 enabled"

    # 2) A different provider must select a different impl — both are built into the
    #    default feature set, so swapping is a one-line config change.
    cat > "$HOME/anthropic.toml" <<TOML
    [agent]
    provider    = "anthropic"
    policy      = "auto-approve"
    working_dir = "$work"
    [provider]
    model   = "claude-haiku-4-5"
    api_key = "none"
    [memory]
    backend = "file"
    [metrics]
    enabled = false
    [search]
    auto_index = false
    [git]
    auto_fetch_secs = 0
    TOML
    expect_ok anthropic "$HOME/anthropic.toml" \
      "config: OK" \
      "provider  = anthropic"

    # 3) Adversarial: a selector with no factory must fail at build, not silently
    #    fall back to a default.
    cat > "$HOME/bad-provider.toml" <<TOML
    [agent]
    provider    = "does-not-exist"
    policy      = "auto-approve"
    working_dir = "$work"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "none"
    api_key  = "none"
    [metrics]
    enabled = false
    [search]
    auto_index = false
    [git]
    auto_fetch_secs = 0
    TOML
    expect_fail bad-provider "$HOME/bad-provider.toml"

    # 4) Adversarial: malformed TOML must be a hard parse failure.
    printf '[agent\nprovider = "openai-compat"\n' > "$HOME/broken.toml"
    expect_fail broken "$HOME/broken.toml"

    # 5) The task-router (model-router 02+02b): a routed fleet with role AND
    #    task_mode rules must load and build through the real factory chain.
    cat > "$HOME/route.toml" <<TOML
    [agent]
    provider    = "task-router"
    policy      = "auto-approve"
    working_dir = "$work"
    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "unused"
    api_key  = "unused"
    [route]
    [[route.upstreams]]
    name = "kimi"
    endpoint = "http://127.0.0.1:1/v1"
    model = "m"
    tags = ["reasoning"]
    tier = "heavy"
    [[route.upstreams]]
    name = "glm"
    endpoint = "http://127.0.0.1:2/v1"
    model = "m"
    [[route.rules]]
    match  = { role = "judge" }
    prefer = { tags = ["reasoning"], tier = "heavy" }
    [[route.rules]]
    match  = { task_mode = "debug", min_context = 4096 }
    prefer = { upstreams = ["kimi"] }
    [route.default_prefer]
    upstreams = ["kimi", "glm"]
    [memory]
    backend = "file"
    [metrics]
    enabled = false
    [search]
    auto_index = false
    [git]
    auto_fetch_secs = 0
    TOML
    expect_ok route "$HOME/route.toml" \
      "config: OK" \
      "provider  = task-router"

    # 6) Adversarial: a typo'd match constraint must fail the BUILD (02b strict
    #    parsing) — never a rule that silently matches everything.
    sed 's/task_mode = "debug"/task_mode = "reveiw"/' "$HOME/route.toml" > "$HOME/route-typo.toml"
    expect_fail route-typo "$HOME/route-typo.toml"

    # 7) Adversarial: the router must reject routing to itself.
    sed 's/name = "glm"/name = "task-router"/' "$HOME/route.toml" > "$HOME/route-self.toml"
    expect_fail route-self "$HOME/route-self.toml"

    # 8) The textproto scenario loader (model-router 03): the fleet + policy as
    #    ONE ModelRouterConfig file must load through the shipped binary's real
    #    loader (parse -> validate -> [route] overlay -> task-router factory).
    cat > "$HOME/mrc.textproto" <<TEXTPROTO
    upstreams {
      id: "kimi"
      kind: "openai-compat"
      enabled: true
      base_url: "http://127.0.0.1:1/v1"
      model: "m"
      tags: "reasoning"
      tier: POOL_TIER_HEAVY
    }
    upstreams {
      id: "glm"
      kind: "openai-compat"
      enabled: true
      base_url: "http://127.0.0.1:2/v1"
      model: "m"
    }
    policy {
      rules {
        match { role: ROUTE_ROLE_JUDGE task_mode: TASK_MODE_DEBUG }
        prefer { tags: "reasoning" }
      }
      default_prefer { upstreams: "kimi" upstreams: "glm" }
    }
    TEXTPROTO
    sed "s|\[route\]|[route]\n    # replaced by the scenario file|" "$HOME/route.toml" > "$HOME/route-mrc.toml"
    printf '\n' >> "$HOME/route-mrc.toml"
    if ! out="$(agent --config "$HOME/route-mrc.toml" --model-router-config "$HOME/mrc.textproto" --check-config 2>"$HOME/mrc.err")"; then
      echo "FAIL(mrc): a valid scenario file must check clean; stderr:" >&2
      cat "$HOME/mrc.err" >&2
      exit 1
    fi
    echo "--- mrc ---"; echo "$out"
    case "$out" in
      *"provider  = task-router"*) : ;;
      *) echo "FAIL(mrc): expected task-router selection, got:" >&2; echo "$out" >&2; exit 1 ;;
    esac

    # 9) Adversarial: a malformed scenario file must fail the build closed.
    printf 'upstreams { id: "a" bogus: 1' > "$HOME/mrc-bad.textproto"
    if agent --config "$HOME/route-mrc.toml" --model-router-config "$HOME/mrc-bad.textproto" --check-config >"$HOME/mrc-bad.out" 2>&1; then
      echo "FAIL(mrc-bad): a malformed scenario file must not check clean:" >&2
      cat "$HOME/mrc-bad.out" >&2
      exit 1
    fi
    echo "--- mrc-bad (correctly rejected) ---"

    echo "OK: representative configs load, build, and select the right impls; broken ones fail closed" > "$out"
  ''
