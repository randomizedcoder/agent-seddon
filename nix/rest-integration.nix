# nix/rest-integration.nix
#
# `rest-integration` — the opt-in live proof that the REST/JSON compat surface
# (gap-analysis §4, docs/design/rest-openapi/) actually round-trips to gRPC.
#
# The hermetic gate already proves a lot statically: `http_annotations` /
# `openapi_parity` prove every RPC carries a `(google.api.http)` route, and the
# `portal-envoy` check runs real `envoy --mode validate` over the transcoder +
# the real 40-service descriptor. What it CANNOT do inside `nix flake check` (no
# docker/network in the sandbox) is stand up Envoy + a gateway and drive a real
# `curl`. That is this harness: boot `agent --serve-all` on loopback, bring up
# the `grpc_json_transcoder` listener (`grpc-web-up` renders the `rest` block of
# `nix/portal/envoy-spec.nix`), then curl the REST surface — proving a REST call
# transcodes to gRPC and AGREES with the equivalent gRPC call, and that hostile
# REST input fails 4xx (never 5xx / OOM / hang). Registered in
# `nix/integration.nix` (model-free tier; self-skips without a container runtime).
#
# The representative RPC is `ConfigService.GetValues` (empty request → `GET
# /v1/config/values`): the config seam is wired on the `--serve-all` gateway
# whenever the CLI loaded config from a file (`--config`, which we do), so the
# transcoded route always reaches a real backend — no UNIMPLEMENTED noise.
#
# The container runtime is picked the way every container app picks it
# (`nix/lib/mk-container-app.nix`): `$CONTAINER_RUNTIME` (default `docker`), so a
# docker-less, podman-only host (l2) runs this with `CONTAINER_RUNTIME=podman`
# instead of self-skipping — `grpc-web-up`/`-down` inherit the same env var.
#
# Exit codes (the shared 0/1/2 contract): 0 clean or skipped (no runtime), 1 a
# harness failure (a server never came up), 2 a contract failure (a REST call
# disagreed with gRPC, an unmapped path was not 404, or hostile input 5xx'd).
{
  pkgs,
  lib,
  versions,
  agent,
  grpc-web-up,
  grpc-web-down,
  harness,
}:
let
  restPort = 8094; # nix/portal/envoy-spec.nix `ports.rest` (loopback transcoder listener)
  gatewayPort = 50100; # nix/portal/envoy-spec.nix `ports.gateway` (--serve-all)
in
pkgs.writeShellApplication {
  name = "rest-integration";
  runtimeInputs = [
    agent
    grpc-web-up
    grpc-web-down
    versions.grpcurl # gRPC-leg parity probe + gateway health check
    versions.docker
    versions.podman # a podman-only host (l2) probes + runs via CONTAINER_RUNTIME=podman
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
    pkgs.gnugrep
  ];
  text = ''
    set -uo pipefail
  ''
  + harness.contract
  + ''

    REST="http://127.0.0.1:${toString restPort}"
    GW="127.0.0.1:${toString gatewayPort}"
    # No `[auth]` block below ⇒ the gateway runs mode="none" and accepts
    # unauthenticated loopback calls, so `grpc-web-up` must not try to fetch a
    # JWKS from a token-less agent — pin the edge to no jwt_authn.
    export PORTAL_AUTH=off

    # Opt-in resource: skip-with-notice (exit 0) on a bare machine so the whole
    # `nix run .#integration` aggregate stays runnable without a container runtime.
    # Pick the runtime like every container app does (mk-container-app.nix): honor
    # $CONTAINER_RUNTIME (default docker) so a podman-only host is not skipped.
    runtime="''${CONTAINER_RUNTIME:-docker}"
    if ! "$runtime" info >/dev/null 2>&1; then
      echo "rest-integration: SKIP — container runtime ($runtime) not reachable (the REST leg needs the Envoy transcoder container)."
      contract_exit "PASS: rest-integration skipped (no container runtime)."
    fi

    work="$(mktemp -d)"
    # shellcheck disable=SC2329  # invoked indirectly via the EXIT trap below.
    cleanup() {
      grpc-web-down >/dev/null 2>&1 || true
      for pf in "$work"/*.pid; do
        [ -f "$pf" ] || continue
        p="$(cat "$pf" 2>/dev/null || true)"
        [ -n "$p" ] && kill "$p" 2>/dev/null || true
      done
      rm -rf "$work"
    }
    trap cleanup EXIT

    # A hermetic, model-free config. `--config` gives the CLI a source_path, which
    # is exactly what wires the config seam (ConfigService) onto the gateway — our
    # representative transcoded route. No `[auth]` ⇒ mode="none" (loopback, no bearer).
    cat > "$work/agent.toml" <<EOF
    [agent]
    provider = "openai-compat"
    policy   = "auto-approve"
    working_dir = "$work"

    [provider]
    base_url = "http://127.0.0.1:1/v1"
    model    = "unused-no-model-needed"
    api_key  = "none"

    [memory]
    backend       = "file"
    episodic_path = "$work/.agent/episodic.jsonl"
    semantic_dir  = "$work/.agent/memory"

    [tokenizer]
    backend = "approx"

    [search]
    auto_index = false

    [metrics]
    enabled = false
    EOF

    # --- boot the gateway ------------------------------------------------------
    echo "==> rest-integration: starting 'agent --serve-all' on $GW"
    nohup agent --serve-all --config "$work/agent.toml" > "$work/gateway.log" 2>&1 &
    echo "$!" > "$work/gateway.pid"
    up=0
    for _ in $(seq 1 40); do
      if grpcurl -plaintext "$GW" grpc.health.v1.Health/Check >/dev/null 2>&1; then up=1; break; fi
      sleep 1
    done
    if [ "$up" -ne 1 ]; then
      echo "FAIL(harness): gateway never became healthy on $GW" >&2
      tail -n 20 "$work/gateway.log" >&2 || true
      note_fail 1
      contract_exit "done"
    fi
    echo "rest-integration: gateway healthy."

    # --- bring up the transcoder ----------------------------------------------
    echo "==> rest-integration: bringing up the grpc_json_transcoder listener ($REST)"
    if ! grpc-web-up; then
      echo "FAIL(harness): grpc-web bridge (transcoder) did not come up" >&2
      note_fail 1
      contract_exit "done"
    fi
    ready=0
    for _ in $(seq 1 30); do
      if curl -sf -o /dev/null "$REST/v1/config/status"; then ready=1; break; fi
      sleep 1
    done
    if [ "$ready" -ne 1 ]; then
      echo "FAIL(harness): REST listener never answered on $REST" >&2
      note_fail 1
      contract_exit "done"
    fi
    echo "rest-integration: REST listener ready."

    # --- helpers ---------------------------------------------------------------
    # HTTP status code of a request. Under `set -uo pipefail` (no `-e`), a curl
    # connection failure returns non-zero but still prints "000" via -w, which is
    # what we want to observe (a hang/crash, distinct from any 2xx/4xx/5xx).
    rest_code() { # METHOD PATH [JSON]
      local m="$1" p="$2"
      if [ "$#" -ge 3 ]; then
        curl -s -o /dev/null -w '%{http_code}' -X "$m" \
          -H 'content-type: application/json' --data-binary "$3" "$REST$p"
      else
        curl -s -o /dev/null -w '%{http_code}' -X "$m" "$REST$p"
      fi
    }
    # Assert an observed code matches an ERE; a mismatch is a CONTRACT violation.
    expect() { # LABEL WANT-ERE GOT
      if printf '%s' "$3" | grep -Eq "$2"; then
        echo "  ok: $1 -> $3"
      else
        echo "CONTRACT: $1 expected /$2/ got $3" >&2
        note_fail 2
      fi
    }

    echo "==> rest-integration: REST↔gRPC cases"

    # (1) positive — the representative read transcodes and returns JSON.
    expect "GET /v1/config/values status" '^200$' "$(rest_code GET /v1/config/values)"
    rest_values="$(curl -s "$REST/v1/config/values")"
    if printf '%s' "$rest_values" | jq -e 'has("values")' >/dev/null 2>&1; then
      echo "  ok: REST /v1/config/values body carries .values"
    else
      echo "CONTRACT: REST /v1/config/values missing .values: $(printf '%s' "$rest_values" | head -c 200)" >&2
      note_fail 2
    fi

    # (2) positive parity — the SAME read over gRPC agrees (both carry .values).
    grpc_values="$(grpcurl -d '{}' -plaintext "$GW" agent.v1.ConfigService/GetValues 2>/dev/null || true)"
    if printf '%s' "$grpc_values" | jq -e 'has("values")' >/dev/null 2>&1; then
      echo "  ok: gRPC ConfigService/GetValues agrees (carries .values)"
    else
      echo "CONTRACT: gRPC GetValues missing .values (REST/gRPC parity): $(printf '%s' "$grpc_values" | head -c 200)" >&2
      note_fail 2
    fi

    # (3) positive — the schema read also transcodes.
    expect "GET /v1/config/schema status" '^200$' "$(rest_code GET /v1/config/schema)"

    # (4) negative — an unmapped path is 404 (match_incoming_request_route:true),
    #     not silently routed anywhere.
    expect "GET /v1/does-not-exist" '^404$' "$(rest_code GET /v1/does-not-exist)"

    # (5) boundary — an empty-but-valid validate body (no edits) is handled, not 500.
    expect "POST /v1/config/validate {}" '^200$' "$(rest_code POST /v1/config/validate '{}')"

    # (6) adversarial — malformed JSON is a client 400 at the transcoder, never 500.
    expect "POST /v1/config/validate {malformed" '^400$' "$(rest_code POST /v1/config/validate '{bad')"

    # (7) adversarial — an unknown body field is ignored by protobuf-JSON (→ 200),
    #     never a 500. We assert the safe non-5xx outcome and record it.
    expect "POST /v1/config/validate {unknown-field}" '^(2..|4..)$' "$(rest_code POST /v1/config/validate '{"nope":1}')"

    # (8) adversarial — a percent-encoded traversal under a REAL served prefix does
    #     not match a route and is rejected 4xx (never a 5xx or a file read).
    expect "GET traversal under /v1/config" '^4..$' "$(rest_code GET '/v1/config/values/..%2F..%2Fadmin')"

    # (9) adversarial — an oversized body must fail closed (4xx, or a refused/closed
    #     connection), NEVER a 5xx or OOM. We build ~8MB inside a real field
    #     (ValidateConfigRequest.edits[].value) so it is a genuine large message,
    #     then prove the server SURVIVED by re-probing the gateway.
    big_val="$(head -c 8000000 /dev/zero | tr '\0' 'a')"
    printf '{"edits":[{"path":"x","value":"%s"}]}' "$big_val" > "$work/big.json"
    big_code="$(curl -s -o /dev/null -w '%{http_code}' -X POST \
      -H 'content-type: application/json' --data-binary @"$work/big.json" \
      "$REST/v1/config/validate" 2>/dev/null || echo 000)"
    if printf '%s' "$big_code" | grep -Eq '^5..$'; then
      echo "CONTRACT: oversized body produced a 5xx ($big_code)" >&2
      note_fail 2
    else
      echo "  ok: oversized body handled without 5xx -> $big_code"
    fi
    # Survival probe: the gateway is still healthy (no OOM crash on the big body).
    if grpcurl -plaintext "$GW" grpc.health.v1.Health/Check >/dev/null 2>&1; then
      echo "  ok: gateway survived the oversized body (still healthy)"
    else
      echo "CONTRACT: gateway did not survive the oversized body (health check failed)" >&2
      note_fail 2
    fi

    contract_exit "PASS: rest-integration — REST↔gRPC round-trips agree; unmapped paths 404; hostile input 4xx (never 5xx/OOM)."
  '';
}
