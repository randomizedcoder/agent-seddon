# nix/review-eval.nix
#
# `nix run .#review-eval [-- --judge] [--out DIR] [--all]`
#
# Generates the grounded review context for a curated, code-heavy corpus and
# records a base rate for the code-review flow (docs/design/code-review/eval/):
#
#   * Rust — the real working tree's merge-PR history (run this from the repo),
#     filtered to code-heavy PRs; each is materialized with a detached worktree so
#     the diff AND the file-set/language scan are faithful to that commit.
#   * Go — the flake-pinned xtcp2 changes (hash-locked, reproducible), each
#     reconstructed into a temp repo from its base + head trees.
#
# `--judge` additionally sends each context to a configured GLM endpoint for a
# quality assessment (env: REVIEW_EVAL_BASE_URL / _MODEL / _API_KEY /
# _INSECURE_TLS). Refuses (exit 1) rather than skipping when it can't reach one —
# a skip that exits 0 reads as a pass.
#
# Not a `nix flake check`: the Rust corpus needs the stripped `.git`, and
# `--judge` needs a network endpoint. The Go path is covered hermetically by
# nix/checks/review-go.nix instead.
{
  pkgs,
  lib,
  agent,
  reviewGoCorpus,
}:
let
  # <label>\t<base-tree>\t<head-tree>, one Go change per line.
  goCorpus = lib.concatMapStringsSep "\n" (
    label:
    let
      c = reviewGoCorpus.${label};
    in
    "${label}\t${c.base}\t${c.head}"
  ) (lib.attrNames reviewGoCorpus);
in
pkgs.writeShellApplication {
  name = "review-eval";
  runtimeInputs = [
    agent
    pkgs.git
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
    pkgs.gawk
  ];
  text = ''
    judge=0
    all=0
    out=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --judge) judge=1 ;;
        --all) all=1 ;;
        --out) shift; out="''${1:-}" ;;
        *) echo "usage: review-eval [--judge] [--all] [--out DIR]" >&2; exit 1 ;;
      esac
      shift
    done
    if [ -z "$out" ]; then out="$(mktemp -d)/review-eval"; fi
    mkdir -p "$out"
    echo "review-eval: writing to $out"

    # Minimal config: the collector never calls the model, so a placeholder
    # provider is fine; search is off so no index is built.
    cfg="$out/agent.toml"
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

    summary="$out/summary.tsv"
    printf 'id\tlang\tchanged\thost\trelationship\tcollect_ms\n' > "$summary"

    # Pull the headline facts out of a generated context into the summary.
    record() {
      local id="$1" ctx="$2"
      local repo_line change_line lang host rel changed ms
      repo_line="$(grep -m1 '^Repo:' "$ctx" || true)"
      change_line="$(grep -m1 '^Change:' "$ctx" || true)"
      lang="$(echo "$repo_line" | sed -n 's/^Repo: \([a-z]*\) project.*/\1/p')"
      host="$(echo "$repo_line" | sed -n 's/.*host \([a-z]*\) .*/\1/p')"
      rel="$(echo "$repo_line" | sed -n 's/^Repo: [a-z]* project · \([a-z]*\) .*/\1/p')"
      changed="$(echo "$change_line" | sed -n 's/.*— \([0-9]*\) changed file.*/\1/p')"
      ms="$(grep -m1 '^Collection:' "$ctx" | sed -n 's/^Collection: \([0-9]*\) ms.*/\1/p' || true)"
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$id" "''${lang:-?}" "''${changed:-?}" "''${host:-?}" "''${rel:-?}" "''${ms:-?}" >> "$summary"
    }

    # ---- Rust corpus: the real repo's code-heavy merge PRs -------------------
    if git rev-parse --show-toplevel >/dev/null 2>&1; then
      repo="$(git rev-parse --show-toplevel)"
      limit="''${REVIEW_EVAL_RUST_LIMIT:-10}"
      n=0
      while IFS='|' read -r parents subject; do
        base="$(echo "$parents" | awk '{print $1}')"
        head="$(echo "$parents" | awk '{print $2}')"
        [ -z "$head" ] && continue
        num="$(echo "$subject" | grep -oE '#[0-9]+' | head -1 | tr -d '#')"
        # Code-ratio filter: keep Rust-code-heavy PRs (unless --all).
        rs="$(git -C "$repo" diff --name-only "$base" "$head" | grep -c '\.rs$' || true)"
        md="$(git -C "$repo" diff --name-only "$base" "$head" | grep -cE '\.(md|txt)$' || true)"
        if [ "$all" -eq 0 ] && { [ "$rs" -lt 3 ] || [ "$rs" -le "$md" ]; }; then continue; fi
        [ "$all" -eq 0 ] && [ "$n" -ge "$limit" ] && break
        n=$((n + 1))
        wt="$(mktemp -d)"
        if git -C "$repo" worktree add --detach -q "$wt" "$head" 2>/dev/null; then
          ctx="$out/rust-pr-''${num:-$n}.txt"
          ( cd "$wt" && agent --config "$cfg" --review "$base..$head" ) > "$ctx" 2>/dev/null || true
          record "rust-pr-''${num:-$n}" "$ctx"
          git -C "$repo" worktree remove --force "$wt" 2>/dev/null || rm -rf "$wt"
        fi
      done < <(git -C "$repo" log --merges --grep="Merge pull request #" --format="%P|%s")
      echo "review-eval: rust contexts generated ($n)"
    else
      echo "review-eval: not in a git repo — skipping the Rust corpus (run from the repo)" >&2
    fi

    # ---- Go corpus: the flake-pinned xtcp2 changes --------------------------
    while IFS=$'\t' read -r label base_tree head_tree; do
      [ -z "$label" ] && continue
      wd="$(mktemp -d)"
      (
        cd "$wd"
        git init -q -b main
        git config user.email t@e; git config user.name t
        cp -r "$base_tree"/. . ; chmod -R u+w .
        git add -A -f && git commit -q -m base
        base_oid="$(git rev-parse HEAD)"
        git remote add origin https://github.com/randomizedcoder/xtcp2.git
        git rm -rqf . >/dev/null; cp -r "$head_tree"/. . ; chmod -R u+w .
        git add -A -f && git commit -q -m head
        head_oid="$(git rev-parse HEAD)"
        agent --config "$cfg" --review "$base_oid..$head_oid"
      ) > "$out/go-$label.txt" 2>/dev/null || true
      record "go-$label" "$out/go-$label.txt"
      rm -rf "$wd"
    done <<EOF
    ${goCorpus}
    EOF
    echo "review-eval: go contexts generated"

    # ---- Optional GLM judge -------------------------------------------------
    if [ "$judge" -eq 1 ]; then
      base_url="''${REVIEW_EVAL_BASE_URL:-http://localhost:11434/v1}"
      model="''${REVIEW_EVAL_MODEL:-glm-4.6}"
      key="''${REVIEW_EVAL_API_KEY:-none}"
      ins=""; [ "''${REVIEW_EVAL_INSECURE_TLS:-0}" = "1" ] && ins="-k"
      # Refuse-don't-skip: a judge that can't reach a model must fail, not pass.
      if ! curl -s $ins -m 15 -o /dev/null "$base_url/models" \
         && ! curl -s $ins -m 15 -o /dev/null "$base_url/chat/completions"; then
        echo "review-eval --judge: no model endpoint reachable at $base_url" >&2
        exit 1
      fi
      rubric='You are grading an auto-generated, tool-derived "grounded review context" for a code change. It is meant to prepare a reviewer with FACTS (no hallucination). Score 1-5 on: (a) groundedness/accuracy, (b) review-readiness/completeness, (c) signal-to-noise. Then list concrete GAPS a reviewer still needs. Reply as compact JSON: {"groundedness":N,"readiness":N,"signal":N,"gaps":["..."]}.'
      for ctx in "$out"/*.txt; do
        [ -e "$ctx" ] || continue
        body="$(jq -n --arg m "$model" --arg r "$rubric" --rawfile c "$ctx" \
          '{model:$m,temperature:0,max_tokens:4096,messages:[{role:"user",content:($r+"\n\n---\n"+$c)}]}')"
        resp="$(curl -s $ins -m 600 -H "Authorization: Bearer $key" -H "Content-Type: application/json" \
          -d "$body" "$base_url/chat/completions" || true)"
        echo "$resp" | jq -r '.choices[0].message.content // "no content"' > "$out/judge-$(basename "$ctx" .txt).txt"
      done
      echo "review-eval --judge: GLM assessments written"
    fi

    echo "review-eval: done."
    echo "  contexts:  $out/*.txt"
    echo "  summary:   $summary"
    column -t -s $'\t' "$summary" || cat "$summary"
  '';
}
