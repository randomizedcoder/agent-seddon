import 'package:agent_portal/src/auth/auth_interceptor.dart';
import 'package:agent_portal/src/auth/auth_platform.dart';
import 'package:agent_portal/src/auth/capabilities.dart';
import 'package:agent_portal/src/auth/install_callback.dart';
import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/pages/forges_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../fake_gateway.dart';
import '../fakes/forge_registry_service.dart';
import '../fakes/github_install_registry_service.dart';
import '../recording.dart';
import 'robot.dart';

/// Robot for the **Forges** page (forge-auth F5d1). Hosts the page over a
/// [FakeGateway] serving [FakeGithubInstallRegistryService] and
/// [FakeForgeRegistryService], with a [MemoryAuthPlatform] standing in for the
/// tab's storage and navigation, and a clock fixed at [now]. Script the fakes,
/// [arrangeReturn] a GitHub install return, then [load].
class ForgesRobot extends Robot {
  ForgesRobot._(super.tester, this.gw, this.installs, this.forges, this.platform);

  static const now = 1000000;
  static const installSvc = 'agent.v1.GithubInstallRegistryService';
  static const forgeSvc = 'agent.v1.ForgeRegistryService';

  final FakeGateway gw;
  final FakeGithubInstallRegistryService installs;
  final FakeForgeRegistryService forges;
  final MemoryAuthPlatform platform;
  // The portal's real interceptor with a fixed bearer, so a test can see it on
  // CompleteInstall.
  late final PortalClients clients =
      gw.clients(interceptors: [AuthInterceptor(() => bearer)]);

  static const bearer = 'tok-forges';

  RecordingLog get log => gw.log;

  static Future<ForgesRobot> create(WidgetTester tester,
      {bool canRedirect = true}) async {
    late FakeGithubInstallRegistryService installs;
    late FakeForgeRegistryService forges;
    late FakeGateway gw;
    await tester.runAsync(() async {
      gw = await FakeGateway.start((log) {
        installs = FakeGithubInstallRegistryService(log);
        forges = FakeForgeRegistryService(log);
        return [installs, forges];
      });
    });
    final platform = MemoryAuthPlatform(canRedirect: canRedirect);
    final robot = ForgesRobot._(tester, gw, installs, forges, platform);
    addTearDown(() => tester.runAsync(() async {
          await robot.clients.terminate();
          await gw.shutdown();
        }));
    return robot;
  }

  /// Leave a GitHub install return waiting, as [AuthState.start] would.
  void arrangeReturn(Map<String, String> query) {
    final cb = parseInstallCallback(query);
    if (cb == null) throw ArgumentError('not an install return: $query');
    stashInstallCallback(platform, cb);
  }

  /// Leave this tab waiting on [state], as Connect GitHub would.
  void arrangePending(String state, {int expiresAt = now + 900}) =>
      storeInstallPending(platform, state, expiresAt);

  /// Mount the page (optionally under [caps]) and let its first load settle.
  Future<void> load({Capabilities? caps}) async {
    final page = ForgesPage(clients: clients, platform: platform, now: () => now);
    await pumpPage(
        caps == null ? page : CapabilityScope(capabilities: caps, child: page));
    await pumpUntil(() => isLoaded || isError, reason: 'forges to load');
  }

  /// Mount the page and hold its first (loading) frame.
  Future<void> pumpLoading() => pumpPage(
      ForgesPage(clients: clients, platform: platform, now: () => now));

  bool get isLoaded =>
      exists('forges.install.empty') ||
      find
          .byWidgetPredicate((w) =>
              w.key is ValueKey<String> &&
              (w.key as ValueKey<String>).value.startsWith('forges.install.item.'))
          .evaluate()
          .isNotEmpty;
  bool get isError => exists('forges.error.retry');
  bool get showsSpinner =>
      find.byType(CircularProgressIndicator).evaluate().isNotEmpty;

  String? get bannerText {
    final f = byKey('forges.banner');
    if (f.evaluate().isEmpty) return null;
    return (tester.widget(f) as Text).data;
  }

  bool firedInstall(String method) => log.fired('$installSvc/$method');
  bool firedForge(String method) => log.fired('$forgeSvc/$method');
  RecordedCall? lastInstall(String method) => log.last('$installSvc/$method');
  RecordedCall? lastForge(String method) => log.last('$forgeSvc/$method');
  int countInstall(String method) => log.countOf('$installSvc/$method');
}
