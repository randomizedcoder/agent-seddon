import 'package:grpc/grpc.dart' as native;
import 'package:grpc/service_api.dart';

import '../config.dart';
import 'native_tls.dart';

/// Native desktop: dial the `--serve-all` gateway directly over raw gRPC (HTTP/2).
/// No proxy — the gateway hosts every seam's service on one endpoint. TLS when
/// the `PORTAL_TLS_*` settings ask for it (native_tls.dart). Returned as
/// the abstract [ClientChannel] the generated clients accept.
ClientChannel createGatewayChannel(PortalConfig cfg) => native.ClientChannel(
      cfg.gatewayHost,
      port: cfg.gatewayPort,
      options: native.ChannelOptions(credentials: nativeCredentials(cfg)),
    );

/// Native desktop: dial the `--serve-sessions` gateway directly (registry + driving
/// `AgentSessionService`, incl. `Send`).
ClientChannel createSessionsChannel(PortalConfig cfg) => native.ClientChannel(
      cfg.sessionsHost,
      port: cfg.sessionsPort,
      options: native.ChannelOptions(credentials: nativeCredentials(cfg)),
    );

/// Native desktop: dial the `--serve-fleet` process directly (roster CRUD +
/// `ReviewNow`/`Approve` + the review-draft read/edit RPCs).
ClientChannel createFleetChannel(PortalConfig cfg) => native.ClientChannel(
      cfg.fleetHost,
      port: cfg.fleetPort,
      options: native.ChannelOptions(credentials: nativeCredentials(cfg)),
    );
