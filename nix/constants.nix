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
# Ports: seams from 50051 (the conventional gRPC range); the --serve-all
# gateway sits clear of that run at 50100. Sockets live under a single
# writable dir so a same-host deployment can bypass TCP; override per-seam in
# `[grpc]` config (e.g. a k8s emptyDir mount) as needed.
#
# Each seam also gets a Prometheus `metrics_port` (9601+; the gateway uses 9700). When a seam runs as
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
    repo = {
      port = 50057;
      socket = "/tmp/agent-seddon/repo.sock";
      metrics_port = 9607;
    };

    session = {
      port = 50058;
      socket = "/tmp/agent-seddon/session.sock";
      metrics_port = 9608;
    };

    # NOT a seam: the `agent --serve-all` gateway, which hosts every seam's
    # service in one process on one endpoint. A same-host deployment that wants
    # all seams distributed would otherwise run one process (and one port) per
    # seam. Kept in this table so the port allocation stays in one place.
    #
    # Deliberately well clear of the seam range: seams are allocated
    # contiguously from 50051 as they are distributed, and a gateway sitting in
    # the middle of that run would force every later seam to step around it.
    gateway = {
      port = 50100;
      socket = "/tmp/agent-seddon/gateway.sock";
      metrics_port = 9700;
    };
  };
}
