import 'package:grpc/service_api.dart';

import '../config.dart';

/// Fallback for a platform with neither dart:io nor dart:html. The portal only
/// targets native desktop + web, so this should never be reached.
ClientChannel createGatewayChannel(PortalConfig cfg) =>
    throw UnsupportedError('no gRPC transport available on this platform');

ClientChannel createSessionsChannel(PortalConfig cfg) =>
    throw UnsupportedError('no gRPC transport available on this platform');
