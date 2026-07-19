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
# Each seam also gets a Prometheus `metrics_port` (9601–9605). When a seam runs as
# its own `agent --serve-<seam>` process, it serves `/metrics` there instead of the
# main agent's default `127.0.0.1:9600`, so co-located seam servers don't collide
# and Prometheus can scrape each as a separate job (see nix/prometheus).
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
      metrics_port = 9601;
    };
    memory = {
      port = 50052;
      socket = "/tmp/agent-seddon/memory.sock";
      metrics_port = 9602;
    };
    tools = {
      port = 50053;
      socket = "/tmp/agent-seddon/tools.sock";
      metrics_port = 9603;
    };
    context = {
      port = 50054;
      socket = "/tmp/agent-seddon/context.sock";
      metrics_port = 9604;
    };
    policy = {
      port = 50055;
      socket = "/tmp/agent-seddon/policy.sock";
      metrics_port = 9605;
    };
    search = {
      port = 50056;
      socket = "/tmp/agent-seddon/search.sock";
      metrics_port = 9606;
    };
  };
}
