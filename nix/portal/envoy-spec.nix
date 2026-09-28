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
  # The Envoy `grpc_json_transcoder` inputs (rest-openapi §4): the descriptor Envoy
  # loads + the service list to transcode, both derived from the SAME `buf` build so
  # they cannot drift. Referencing these store paths gives `file` (and hence any
  # derivation that reads it — the `portal-envoy` check) a build dependency on the
  # descriptor, so `envoy --mode validate` sees a real file, IFD-free.
  restDescriptor = import ../rest-descriptor.nix { inherit pkgs versions; };
  ports = {
    grpcWeb = 8090; # browser -> gateway
    grpcWebSessions = 8091; # browser -> sessions
    portalWeb = 8092; # static server for the built web bundle
    grpcWebFleet = 8093; # browser -> fleet (--serve-fleet)
    rest = 8094; # browser/curl -> gateway via grpc_json_transcoder (REST/JSON)
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
      # The REST/JSON transcoder listener (rest-openapi §4). A distinct listener kind:
      # it fronts the SAME agent_gateway cluster the grpc-web gateway uses, but its
      # filter chain runs `grpc_json_transcoder` (not `grpc_web`), projecting the gRPC
      # surface to REST per the `.proto` `(google.api.http)` routes. Pinned to loopback
      # (a compat surface; the agent's own AuthLayer still applies to every transcoded
      # call), so — unlike the grpc-web listeners — it ignores PORTAL_GRPC_WEB_HOST.
      rest = {
        name = "rest_transcoder";
        port = ports.rest;
        cluster = "agent_gateway";
        upstream_port = ports.gateway;
        descriptor = "${restDescriptor}/agent_descriptor.pb";
        services_file = "${restDescriptor}/services.txt";
      };
      otel_port = ports.otelCollector;
      gateway_port = ports.gateway;
    }
  );
}
