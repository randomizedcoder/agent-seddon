# nix/portal/envoy-spec.nix
#
# The portal's Envoy grpc-web bridge, as data: which browser-facing listener
# forwards to which agent gateway, plus the OTLP collector. The single source of
# those ports for nix/portal (`ports`) and the input to the config renderer
# (`file`, read by test/portal-envoy/portal_envoy.py): `grpc-web-up` renders it
# at bring-up and the `portal-envoy` check renders + `envoy --mode validate`s it
# (security-hardening S14).
#
# UI plumbing, not seams, so these live here rather than in nix/constants.nix's seam
# table (which the constants-sync check renders verbatim).
{
  pkgs,
  versions,
}:
let
  ports = {
    grpcWeb = 8090; # browser -> gateway
    grpcWebSessions = 8091; # browser -> sessions
    portalWeb = 8092; # static server for the built web bundle
    grpcWebFleet = 8093; # browser -> fleet (--serve-fleet)
    # The gateways the proxy forwards to (mirror nix/constants.nix gateway/sessions).
    gateway = 50100;
    sessions = 50080;
    fleet = 50086; # the full review-fleet process (--serve-fleet)
    # OTLP/gRPC collector (the decomposed HyperDX otel-collector), reached on host
    # loopback since envoy runs --network host.
    otelCollector = versions.otlpGrpcPort;
  };
in
{
  inherit ports;
  file = pkgs.writeText "portal-envoy-spec.json" (
    builtins.toJSON {
      listeners = [
        {
          name = "gateway_grpc_web";
          port = ports.grpcWeb;
          cluster = "agent_gateway";
          upstream_port = ports.gateway;
        }
        {
          name = "sessions_grpc_web";
          port = ports.grpcWebSessions;
          cluster = "agent_sessions";
          upstream_port = ports.sessions;
        }
        {
          name = "fleet_grpc_web";
          port = ports.grpcWebFleet;
          cluster = "agent_fleet";
          upstream_port = ports.fleet;
        }
      ];
      otel_port = ports.otelCollector;
      gateway_port = ports.gateway;
    }
  );
}
