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
  constants = import ../constants.nix;
  ports = {
    grpcWeb = 8090; # browser -> gateway
    grpcWebSessions = 8091; # browser -> sessions
    portalWeb = 8092; # static server for the built web bundle
    grpcWebFleet = 8093; # browser -> fleet (--serve-fleet)
    rest = 8094; # browser/curl -> gateway via grpc_json_transcoder (REST/JSON)
    webhook = 8096; # GitHub -> the fleet's webhook receiver (8095 is l2's llama)
    # The gateways the proxy forwards to (mirror nix/constants.nix gateway/sessions).
    gateway = 50100;
    sessions = 50080;
    fleet = 50086; # the full review-fleet process (--serve-fleet)
    fleetWebhook = constants.fleetWebhookPort; # --serve-fleet's plain-HTTP webhook receiver
    # OTLP/gRPC collector (the decomposed HyperDX otel-collector), reached on host
    # loopback since envoy runs --network host.
    otelCollector = versions.otlpGrpcPort;
    # The front door (security-hardening 10): one TLS origin for the portal, its RPCs
    # and the observability tools, and HyperDX on its own port (its UI is served
    # from `/`). Every request is checked by the agent's gate first.
    frontDoor = 8443;
    frontDoorHyperdx = 8444;
    gate = 8097; # the agent's `[auth.gate] listen` (loopback; Envoy's ext_authz)
    dex = 5556;
    grafana = 3000;
    prometheus = 9090;
    hyperdxUi = 8080;
  };
  # F4's header table: what reaches each route's upstream (one source with the
  # gate's Rust tests).
  frontDoorHeaders = builtins.fromJSON (builtins.readFile ./front-door-headers.json);
in
{
  inherit ports frontDoorHeaders;
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
      # The GitHub App webhook listener (forge-auth F6). Its own listener kind: plain
      # HTTP/1.1 to the fleet's receiver, ONE exact route (`POST /webhooks/github`,
      # everything else 404 at the edge), a request-body cap and header/request
      # timeouts (the receiver's tiny_http has no read timeout, so the edge absorbs slow
      # clients) — and deliberately NO jwt_authn: GitHub sends no bearer, the receiver
      # verifies every delivery's `X-Hub-Signature-256` HMAC before parsing it.
      webhook = {
        name = "fleet_webhook";
        port = ports.webhook;
        cluster = "agent_fleet_webhook";
        upstream_port = ports.fleetWebhook;
        path = "/webhooks/github";
        max_body = 1048576; # = the receiver's MAX_WEBHOOK_BODY
      };
      # The front door (security-hardening 10, S28): rendered only with
      # PORTAL_FRONT_DOOR=on, which needs its own TLS certificate.
      front_door = {
        main = {
          name = "front_door";
          port = ports.frontDoor;
          check = "/_check/main"; # the gate's check prefix for this listener
        };
        hyperdx = {
          name = "front_door_hyperdx";
          port = ports.frontDoorHyperdx;
          check = "/_check/hyperdx";
        };
        upstreams = {
          gate = ports.gate;
          dex = ports.dex;
          portal = ports.portalWeb;
          gateway = ports.gateway;
          sessions = ports.sessions;
          fleet = ports.fleet;
          grafana = ports.grafana;
          prometheus = ports.prometheus;
          hyperdx = ports.hyperdxUi;
        };
        # The RPC services the portal dials on the sessions and fleet processes;
        # every other `/agent.v1.*` goes to the gateway (`--serve-all`).
        rpc_services = {
          sessions = [
            "agent.v1.AgentSessionService"
            "agent.v1.SessionRegistryService"
          ];
          fleet = [ "agent.v1.ReviewFleetService" ];
        };
        headers = frontDoorHeaders;
      };
      otel_port = ports.otelCollector;
      gateway_port = ports.gateway;
    }
  );
}
