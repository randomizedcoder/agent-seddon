# nix/gen-openapi.nix
#
# Renders the OpenAPI (Swagger 2.0) contract for the gRPC surface from the
# `(google.api.http)` annotations on the .proto files (gap-analysis §4,
# docs/design/rest-openapi/). protoc-gen-openapiv2 (from the pinned grpc-gateway) reads
# the annotations via `buf generate` and emits one path per route; `allow_merge` folds
# every service into a single document, and jq rewrites the `info` block.
#
# Returns a single derivation (the formatted `agent.swagger.json`). Both the
# `gen-openapi` app (which copies it into the repo) and the `openapi-sync` check (which
# diffs it against the committed file) reference THIS derivation, so they can never
# disagree — the constants.rs / gen-constants / constants-sync pattern, applied to the
# REST contract. Rust codegen stays on tonic-build; this only produces the published
# REST/JSON contract.
{ pkgs, versions }:
let
  template = ./openapi/buf.gen.openapi.yaml;
  infoFilter = ./openapi/info.jq;
  # Scope the inputs to the proto module + buf config so the doc only regenerates when
  # the wire (or the generator config) actually changes — not on every repo edit.
  bufYaml = ../buf.yaml;
  protoRoot = ../crates/agent-proto/proto;
in
pkgs.runCommand "agent-openapi-swagger.json"
  {
    nativeBuildInputs = [
      versions.buf
      versions.grpc-gateway # supplies protoc-gen-openapiv2 on PATH for buf's local plugin
      pkgs.jq
    ];
  }
  ''
    # Reassemble the buf module layout buf.yaml expects (module path
    # crates/agent-proto/proto), in a writable tree.
    mkdir -p work/crates/agent-proto/proto
    cp -r ${protoRoot}/. work/crates/agent-proto/proto/
    cp ${bufYaml} work/buf.yaml
    cp ${template} work/buf.gen.openapi.yaml
    cd work
    export HOME="$TMPDIR" # buf writes a cache under $HOME

    # Only our services (agent/); imports (google/api) resolve but produce no paths.
    buf generate --template buf.gen.openapi.yaml \
      --path crates/agent-proto/proto/agent --output gen

    jq --indent 2 -f ${infoFilter} gen/gen-openapi/agent.swagger.json > "$out"
  ''
