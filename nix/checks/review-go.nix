# nix/checks/review-go.nix
#
# Reproducible Go coverage for the code-review flow. Reconstructs a flake-pinned
# xtcp2 change (base + head trees, hash-locked) into a temp git repo and asserts
# `agent --review base..head` produces correct Go facts. Offline + deterministic
# (the trees are store paths; the collector never calls a model), so it runs in
# the hermetic `nix flake check` sandbox — unlike the real-repo Rust sweep, which
# needs the stripped `.git` and lives in `nix run .#review-eval`.
{
  pkgs,
  agent,
  reviewGoCorpus,
}:
let
  c = reviewGoCorpus."s3-secret-file"; # #56 feat/s3-secret-file (2 .go files)
in
pkgs.runCommand "agent-review-go"
  {
    nativeBuildInputs = [
      agent
      pkgs.git
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
    [review]
    backend = "local"
    [pool]
    members = []
    TOML

    wd="$(mktemp -d)"
    cd "$wd"
    git init -q -b main
    git config user.email t@e
    git config user.name t
    cp -r ${c.base}/. .
    chmod -R u+w .
    git add -A -f && git commit -q -m base
    base="$(git rev-parse HEAD)"
    git remote add origin https://github.com/randomizedcoder/xtcp2.git
    git rm -rqf . >/dev/null
    cp -r ${c.head}/. .
    chmod -R u+w .
    git add -A -f && git commit -q -m head
    head="$(git rev-parse HEAD)"

    ctx="$(agent --config "$cfg" --review "$base..$head")"
    echo "----- generated Go review context -----"
    echo "$ctx"
    echo "----------------------------------------"

    fail() { echo "FAIL: $1" >&2; exit 1; }
    echo "$ctx" | grep -q "Grounded review facts" || fail "no grounded facts block"
    echo "$ctx" | grep -q "go project"            || fail "not detected as a Go project"
    echo "$ctx" | grep -q "host github"           || fail "remote host not github"
    echo "$ctx" | grep -q "cmd/xtcp2/xtcp2.go"    || fail "missing the changed .go file"

    echo "OK: pinned Go change reconstructed; review facts correct" > "$out"
  ''
