# nix/checks/openapi-sync.nix
#
# Fails if the committed `crates/agent-proto/openapi/agent.swagger.json` differs from
# what `nix/gen-openapi.nix` renders from the .proto `(google.api.http)` annotations —
# i.e. someone changed a route (or added/removed an RPC) without regenerating the
# published REST contract. The fix is a one-liner: `nix run .#gen-openapi`.
#
# The constants.rs / constants-sync pattern, applied to the OpenAPI document
# (docs/design/rest-openapi/). The committed doc is a JSON file, which crane's Rust
# source filter drops from `commonArgs.src` — so, like `buf.nix`, we reference it by a
# direct nix path (repo-root-relative) rather than through the filtered source.
{
  pkgs,
  openapiDoc,
}:
let
  committed = ../../crates/agent-proto/openapi/agent.swagger.json;
in
pkgs.runCommand "openapi-sync-check" { } ''
  if ! diff -u ${committed} ${openapiDoc}; then
    echo "" >&2
    echo "crates/agent-proto/openapi/agent.swagger.json is stale vs the .proto (google.api.http) annotations." >&2
    echo "Regenerate it: nix run .#gen-openapi" >&2
    exit 1
  fi
  touch $out
''
