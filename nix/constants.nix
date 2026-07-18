# nix/constants.nix
#
# Single source of truth for per-service gRPC endpoints: a unique TCP port and a
# unix-domain-socket path for each seam that can run as its own process.
#
# This file is authoritative. The Rust side does NOT re-declare these values —
# `nix run .#gen-constants` renders them into the committed
# `crates/agent-grpc/src/constants.rs`, and the `constants-sync` flake check fails
# if that file drifts from this one. Edit here, then regenerate.
#
# Ports: 50051–50055 (the conventional gRPC range). Sockets live under a single
# writable dir so a same-host deployment can bypass TCP; override per-seam in
# `[grpc]` config (e.g. a k8s emptyDir mount) as needed.
#
# Security: the server binds each socket 0o600 in a 0o700 dir, so only the owner
# UID can connect (on Linux, connecting requires write perm on the socket). On a
# multi-user host, prefer overriding to a per-user runtime dir —
# `[grpc.<seam>] listen = "unix:${XDG_RUNTIME_DIR}/agent-seddon/<seam>.sock"` — so
# sockets never share world-traversable /tmp. See docs/grpc.md.
{
  socketDir = "/tmp/agent-seddon";

  # Ordered provider → policy; keep in sync with the Rust `SEAMS` array order.
  grpc = {
    provider = {
      port = 50051;
      socket = "/tmp/agent-seddon/provider.sock";
    };
    memory = {
      port = 50052;
      socket = "/tmp/agent-seddon/memory.sock";
    };
    tools = {
      port = 50053;
      socket = "/tmp/agent-seddon/tools.sock";
    };
    context = {
      port = 50054;
      socket = "/tmp/agent-seddon/context.sock";
    };
    policy = {
      port = 50055;
      socket = "/tmp/agent-seddon/policy.sock";
    };
  };
}
