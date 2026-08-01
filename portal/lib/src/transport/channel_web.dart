import 'package:grpc/grpc_web.dart';
import 'package:grpc/service_api.dart';

import '../config.dart';

/// Web build: browsers can't speak raw gRPC, so dial the grpc-web proxy (envoy),
/// which translates to raw gRPC against the gateway. Start it with
/// `nix run .#grpc-web-up`.
ClientChannel createGatewayChannel(PortalConfig cfg) =>
    GrpcWebClientChannel.xhr(Uri.parse(cfg.grpcWebUrl));

/// Web build: dial a second grpc-web proxy in front of the `--serve-sessions`
/// gateway (a second envoy → `:50080`). See the portal README.
ClientChannel createSessionsChannel(PortalConfig cfg) =>
    GrpcWebClientChannel.xhr(Uri.parse(cfg.sessionsGrpcWebUrl));
