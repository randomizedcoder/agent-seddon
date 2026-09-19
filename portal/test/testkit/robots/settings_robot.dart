import 'dart:io';

import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/config.dart';
import 'package:agent_portal/src/pages/settings_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../fakes/config_service.dart';
import '../recording.dart';
import 'robot.dart';

/// Robot for the Settings page. It stands up a **real** gRPC [Server] on an
/// ephemeral loopback port hosting [FakeConfigService] and hands the page a
/// [PortalClients] dialing it over the true `channel_io` VM transport — so a test
/// drives the schema-driven config editor across the real wire, proving the exact
/// `ConfigService` RPCs fired with the encoded edits.
///
/// (The shared [FakeGateway] bundles only the prompt seam and builds its own log
/// internally, so — since inc-4 rules forbid editing it — this robot owns an
/// equivalent server it can register the config fake and shared [RecordingLog]
/// on; the loopback-realism recipe is identical.)
///
/// Script responses via [config] before calling [load].
class SettingsRobot extends Robot {
  SettingsRobot._(super.tester, this.log, this.config, this.clients);

  /// The ordered record of every RPC served — assert against this.
  final RecordingLog log;

  /// The `ConfigService` fake (scriptable responses + fault injection).
  final FakeConfigService config;

  /// The clients the page dials.
  final PortalClients clients;

  static Future<SettingsRobot> create(WidgetTester tester) async {
    final log = RecordingLog();
    final config = FakeConfigService(log);
    late Server server;
    late PortalClients clients;
    await tester.runAsync(() async {
      server = Server.create(services: [config]);
      await server.serve(address: InternetAddress.loopbackIPv4, port: 0);
      final port = server.port!;
      clients = PortalClients(PortalConfig(
        gatewayHost: '127.0.0.1',
        gatewayPort: port,
        sessionsHost: '127.0.0.1',
        sessionsPort: port,
        fleetHost: '127.0.0.1',
        fleetPort: port,
      ));
    });
    final robot = SettingsRobot._(tester, log, config, clients);
    addTearDown(() => tester.runAsync(() async {
          await clients.shutdown();
          await server.shutdown();
        }));
    return robot;
  }

  // ── async-state predicates ────────────────────────────────────────────────
  bool get showsSpinner =>
      find.byType(CircularProgressIndicator).evaluate().isNotEmpty;
  bool get isError => exists('settings.retry');
  bool get isLoaded => !showsSpinner && !isError;

  bool sectionExists(String name) => exists('settings.section.item.$name');
  bool fieldExists(String key) => exists('settings.form.field.$key');

  int get sectionItemCount => find
      .byWidgetPredicate((w) =>
          w.key is ValueKey<String> &&
          (w.key as ValueKey<String>)
              .value
              .startsWith('settings.section.item.'))
      .evaluate()
      .length;

  bool get saveEnabled => _enabled('settings.save');
  bool get validateEnabled => _enabled('settings.validate');
  bool get revertEnabled => _enabled('settings.revert');

  bool _enabled(String key) {
    final w = tester.widget(byKey(key));
    // TextButton / OutlinedButton / FilledButton all expose `onPressed`.
    final dynamic btn = w;
    return (btn as dynamic).onPressed != null;
  }

  bool snackContains(String text) =>
      find.textContaining(text).evaluate().isNotEmpty;

  bool dialogContains(String text) =>
      find.descendant(
        of: find.byType(AlertDialog),
        matching: find.textContaining(text),
      ).evaluate().isNotEmpty;

  // ── page pumps ────────────────────────────────────────────────────────────

  /// Pump the page and hold it in the initial **loading** state (no settle).
  Future<void> pumpLoading() => pumpPage(SettingsPage(clients: clients));

  /// Pump the page and settle to its terminal state (loaded or error). On the
  /// success path also wait for the trailing `Status` RPC to *fire* (it is issued
  /// after the schema + values resolve) so the fired-Status assertions are
  /// deterministic. Call [quiesce] before the test ends to drain its response.
  Future<void> load() async {
    await pumpPage(SettingsPage(clients: clients));
    await pumpUntil(() => isLoaded || isError, reason: 'settings to settle');
    if (isLoaded) {
      await pumpUntil(() => log.fired('agent.v1.ConfigService/Status'),
          reason: 'status to fire');
    }
  }

  /// Drain the in-flight RPC tail before teardown. grpc-dart's
  /// `channel.shutdown()` **wedges** on any still-in-flight call, so a test that
  /// leaves an RPC's response mid-flight (e.g. the `Status` a load/reload issues
  /// last, or a `Validate`/`Put` whose reply is still arriving) hangs at teardown.
  /// The caller has already waited for its terminal RPC to *fire*; a few real-time
  /// windows let every fired call's response land (and its `setState` run) so the
  /// channel is idle when it is shut down. Cheap and unconditional — end every
  /// test with it.
  Future<void> quiesce() async {
    for (var i = 0; i < 4; i++) {
      await real(() => Future<void>.delayed(const Duration(milliseconds: 60)));
      await tester.pump();
    }
  }

  // ── intent actions ────────────────────────────────────────────────────────
  Future<void> selectSection(String name) =>
      tap('settings.section.item.$name');

  Future<void> editField(String key, String text) =>
      enterText('settings.form.field.$key', text);

  /// Toggle a boolean (SwitchListTile) field.
  Future<void> toggleField(String key) => tap('settings.form.field.$key');

  Future<void> tapRetry() => tap('settings.retry', until: () => isLoaded);

  Future<void> tapRevert() => tap('settings.revert');

  Future<void> tapValidate() => tap('settings.validate',
      until: () => log.fired('agent.v1.ConfigService/Validate'));

  Future<void> tapSave() =>
      tap('settings.save', until: () => log.fired('agent.v1.ConfigService/Put'));

  /// Wait for the post-save reload (a second GetSchema).
  Future<void> waitForReload(int schemaCount) => pumpUntil(
      () => log.countOf('agent.v1.ConfigService/GetSchema') >= schemaCount,
      reason: 'reload (GetSchema x$schemaCount)');
}
