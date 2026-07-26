import 'package:grpc/grpc.dart' as native;
import 'package:grpc/service_api.dart';

import '../config.dart';

/// Native desktop: dial the `--serve-all` gateway directly over raw gRPC (HTTP/2).
/// No proxy — the gateway hosts every seam's service on one endpoint. Returned as
/// the abstract [ClientChannel] the generated clients accept.
ClientChannel createGatewayChannel(PortalConfig cfg) => native.ClientChannel(
      cfg.gatewayHost,
      port: cfg.gatewayPort,
      options: const native.ChannelOptions(
        credentials: native.ChannelCredentials.insecure(),
      ),
    );
