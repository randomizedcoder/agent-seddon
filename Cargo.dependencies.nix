# Per-crate native inputs and environment variables for the experimental
# cargo(libnix) backend. Normal Cargo and the Crane build ignore this file.
#
# Keep this minimal: add entries only after an observed libnix build failure or a
# concrete build-script requirement. See docs/design/libnix/.
{ pkgs }:

{
  deps = {
    # `crates/agent-proto/build.rs` runs tonic-build/prost-build and needs protoc.
    "agent-proto" = [ pkgs.protobuf ];
  };

  envs = {
    "agent-proto" = {
      PROTOC = "${pkgs.protobuf}/bin/protoc";
    };
  };
}
