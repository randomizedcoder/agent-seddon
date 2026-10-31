import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/theme/theme_controller.dart';
import 'package:agent_portal/src/theme/theme_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../fake_gateway.dart';
import '../fakes/theme_service.dart';
import '../themes.dart';
import 'robot.dart';

/// Robot for the theme pickers (visual/05-portal-integration.md): the nav-rail
/// menu and the Appearance cards, pumped together under a [memoryThemes]
/// controller whose cache lands in [saved]. [signIn] attaches the controller to
/// a [FakeThemeService] over the loopback wire, as `main.dart` does at sign-in.
class ThemeRobot extends Robot {
  ThemeRobot(super.tester, {Map<String, String>? saved, bool scoped = true})
      : saved = saved ?? <String, String>{} {
    if (scoped) themes = memoryThemes(this.saved);
  }

  final Map<String, String> saved;
  FakeGateway? gateway;
  FakeThemeService? server;
  PortalClients? _clients;

  Future<void> load() => pumpPage(const SingleChildScrollView(
        child: Column(children: [ThemeMenuButton(), AppearanceSection()]),
      ));

  /// Serve a [FakeThemeService] (scripted by [arrange]) and attach the
  /// controller for [tenant]/[subject]; returns once `GetActive` has answered.
  Future<void> signIn({
    void Function(FakeThemeService)? arrange,
    String tenant = 'acme',
    String subject = 'alice',
  }) async {
    await real(() async {
      final g = await FakeGateway.start((log) => [server = FakeThemeService(log)]);
      gateway = g;
      _clients = g.clients();
    });
    arrange?.call(server!);
    await real(() => themes!
        .attach(GrpcThemeServer(_clients!.themes), tenant: tenant, subject: subject));
    await tester.pump();
  }

  /// The ordered ThemeService RPCs served so far.
  List<String> get calls => gateway?.log.methods ?? const [];

  Future<void> waitFor(String rpc) =>
      pumpUntil(() => gateway!.log.fired('agent.v1.ThemeService/$rpc'), reason: rpc);

  Future<void> pickFromMenu(String id) async {
    await tester.tap(byKey('theme.menu'));
    await tester.pumpAndSettle();
    await tester.tap(byKey('theme.menu.$id'));
    await tester.pumpAndSettle();
  }

  Future<void> pickCard(String id) async {
    await tester.tap(byKey('theme.card.$id'));
    await tester.pumpAndSettle();
  }

  /// Close the client channel and the fake server.
  Future<void> dispose() => real(() async {
        await _clients?.terminate();
        await gateway?.shutdown();
      });

  /// The theme the app is rendering now.
  ThemeData get rendered => Theme.of(tester.element(byKey('settings.appearance')));
}
