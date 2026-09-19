import 'dart:io';

import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/config.dart';
import 'package:grpc/grpc.dart';

import 'recording.dart';

/// An in-process fake of the agent gRPC gateway for hermetic Layer-A tests.
///
/// It starts a **real** gRPC [Server] on an ephemeral loopback port hosting the
/// caller's fake service impls, then hands back a [PortalClients] whose three
/// channels all dial it. Because the Dart VM transport is the same `channel_io`
/// loopback the native app uses, a test drives a page against this fake over the
/// true wire — proving the real RPC fired with the real encoded arguments.
///
/// Each page's robot builds the seam fakes it needs (bound to the shared [log])
/// and passes them to [start]; the fakes record every call into [log] and return
/// scripted responses. This keeps FakeGateway seam-agnostic, so a new page adds
/// its own fake without touching this file.
class FakeGateway {
  FakeGateway._(this._server, this.log, this.port);

  final Server _server;

  /// The ordered record of every RPC served — assert against this.
  final RecordingLog log;

  /// The ephemeral loopback port the fake is listening on.
  final int port;

  /// Start the fake serving [build]'s services (bound to a fresh shared log).
  static Future<FakeGateway> start(
      List<Service> Function(RecordingLog log) build) async {
    final log = RecordingLog();
    final server = Server.create(services: build(log));
    await server.serve(address: InternetAddress.loopbackIPv4, port: 0);
    return FakeGateway._(server, log, server.port!);
  }

  /// A [PortalConfig] whose gateway/sessions/fleet endpoints all point at this
  /// fake — the web-proxy URLs keep their defaults (unused on the VM transport).
  PortalConfig get config => PortalConfig(
        gatewayHost: '127.0.0.1',
        gatewayPort: port,
        sessionsHost: '127.0.0.1',
        sessionsPort: port,
        fleetHost: '127.0.0.1',
        fleetPort: port,
      );

  /// A fresh [PortalClients] dialing this fake.
  PortalClients clients() => PortalClients(config);

  Future<void> shutdown() async => _server.shutdown();
}
