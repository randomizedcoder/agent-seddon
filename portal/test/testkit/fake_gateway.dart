import 'dart:io';

import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/config.dart';
import 'package:grpc/grpc.dart';

import 'fakes/prompt_service.dart';
import 'recording.dart';

/// An in-process fake of the agent gRPC gateway for hermetic Layer-A tests.
///
/// It starts a **real** gRPC [Server] on an ephemeral loopback port hosting fake
/// service impls, then hands back a [PortalClients] whose three channels all dial
/// it. Because the Dart VM transport is the same `channel_io` loopback the native
/// app uses, a test drives a page against this fake over the true wire — proving
/// the real RPC fired with the real encoded arguments, not a mocked method call.
///
/// The fakes record every call into [log] and return scripted responses (set
/// them on the per-service handles, e.g. [prompts]). Pass [extra] services to
/// register additional seams as later pages are tabled (inc 3+).
class FakeGateway {
  FakeGateway._(this._server, this.log, this.prompts, this.port);

  final Server _server;

  /// The ordered record of every RPC served — assert against this.
  final RecordingLog log;

  /// The `PromptService` fake (scriptable responses + fault injection).
  final FakePromptService prompts;

  /// The ephemeral loopback port the fake is listening on.
  final int port;

  static Future<FakeGateway> start({List<Service> extra = const []}) async {
    final log = RecordingLog();
    final prompts = FakePromptService(log);
    final server = Server.create(services: [prompts, ...extra]);
    await server.serve(address: InternetAddress.loopbackIPv4, port: 0);
    return FakeGateway._(server, log, prompts, server.port!);
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

  /// A fresh [PortalClients] dialing this fake. The caller owns it and must
  /// `shutdown()` it (or let [shutdown] tear the server down at test end).
  PortalClients clients() => PortalClients(config);

  Future<void> shutdown() async => _server.shutdown();
}
