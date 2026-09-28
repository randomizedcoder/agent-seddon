# nix/rest-descriptor.nix
#
# Builds the artifacts Envoy's `grpc_json_transcoder` needs to project the gRPC
# surface as REST/JSON (gap-analysis §4, docs/design/rest-openapi/):
#
#   $out/agent_descriptor.pb  a raw google.protobuf.FileDescriptorSet (imports
#                             included, so the (google.api.http) options resolve)
#                             — the `proto_descriptor` Envoy loads.
#   $out/services.txt         the fully-qualified gRPC service names to transcode,
#                             one per line — the transcoder's required `services`
#                             list, DERIVED FROM THE SAME PROTOS so it can never
#                             drift from the descriptor (a newly-added agent.v1
#                             service is picked up on the next build, with no
#                             hand-maintained list to forget).
#
# Not committed: rebuilt fresh and mounted read-only into the Envoy container by
# `grpc-web-up`. Envoy (C++ protobuf) reads the custom http option natively; Rust
# codegen stays on tonic-build. The buf invocation mirrors nix/gen-openapi.nix.
{ pkgs, versions }:
let
  bufYaml = ../buf.yaml;
  protoRoot = ../crates/agent-proto/proto;
in
pkgs.runCommand "agent-rest-descriptor"
  {
    nativeBuildInputs = [
      versions.buf
      pkgs.jq
    ];
  }
  ''
    # Reassemble the buf module layout buf.yaml expects (module path
    # crates/agent-proto/proto), in a writable tree.
    mkdir -p work/crates/agent-proto/proto
    cp -r ${protoRoot}/. work/crates/agent-proto/proto/
    cp ${bufYaml} work/buf.yaml
    cd work
    export HOME="$TMPDIR" # buf writes a cache under $HOME

    mkdir -p "$out"

    # (1) The binary FileDescriptorSet the transcoder loads. `--as-file-descriptor-set`
    #     emits a raw FileDescriptorSet (no buf image extensions); imports (google/api/*)
    #     are included by default, so the (google.api.http) options resolve.
    buf build --as-file-descriptor-set -o "$out/agent_descriptor.pb"

    # (2) The service list, from the SAME build — so the transcoder's `services` can
    #     never disagree with the descriptor it is given. google/api defines no
    #     services; keep only ours, and sort for a deterministic list.
    buf build -o descriptor.json#format=json
    jq -r '
      .file[]? | select(has("service"))
      | (.package // "") as $p
      | .service[]? | (if $p == "" then .name else $p + "." + .name end)
    ' descriptor.json \
      | grep -E '^agent\.' | sort -u > "$out/services.txt"

    if [ ! -s "$out/services.txt" ]; then
      echo "rest-descriptor: extracted no agent.* services — buf/jq shape changed?" >&2
      exit 1
    fi
  ''
