import 'dart:io';

import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/config.dart';
import 'package:agent_portal/src/pages/router_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../fakes/provider_registry_service.dart';
import '../recording.dart';
import 'robot.dart';

/// Robot for the **Router** page. Owns a real in-process gRPC [Server] (on an
/// ephemeral loopback port) hosting a [FakeProviderRegistryService], plus the
/// [PortalClients] the page dials. Both are torn down — in `runAsync`, since a
/// socket close needs the real event loop — automatically.
///
/// It builds its own server rather than reusing the shared `FakeGateway`, because
/// that gateway constructs its `RecordingLog` internally and only registers the
/// `PromptService`; hosting a second recording seam without editing that shared
/// file means owning the log + server here. The wiring is otherwise identical
/// (real `channel_io` loopback), so a test still proves the real RPC fired with
/// the real encoded arguments. Script responses via [providers] before [load].
class RouterRobot extends Robot {
  RouterRobot._(super.tester, this.log, this.providers, this.clients);

  final RecordingLog log;
  final FakeProviderRegistryService providers;
  final PortalClients clients;

  static const _svc = 'agent.v1.ProviderRegistryService';

  static Future<RouterRobot> create(WidgetTester tester) async {
    final log = RecordingLog();
    final providers = FakeProviderRegistryService(log);
    late Server server;
    await tester.runAsync(() async {
      server = Server.create(services: [providers]);
      await server.serve(address: InternetAddress.loopbackIPv4, port: 0);
    });
    final cfg = PortalConfig(
      gatewayHost: '127.0.0.1',
      gatewayPort: server.port!,
      sessionsHost: '127.0.0.1',
      sessionsPort: server.port!,
      fleetHost: '127.0.0.1',
      fleetPort: server.port!,
    );
    final clients = PortalClients(cfg);
    final robot = RouterRobot._(tester, log, providers, clients);
    addTearDown(() => tester.runAsync(() async {
          await clients.shutdown();
          await server.shutdown();
        }));
    return robot;
  }

  // ── async-state predicates (upstreams view) ───────────────────────────────
  /// The upstreams view has settled to its loaded state (the Add button — and
  /// hence the left column — is only built once loading finishes without error).
  bool get isLoaded => exists('router.upstream.add');
  bool get isError => exists('router.error.retry');
  bool get showsSpinner =>
      find.byType(CircularProgressIndicator).evaluate().isNotEmpty;

  int get itemCount => find
      .byWidgetPredicate((w) =>
          w.key is ValueKey<String> &&
          (w.key as ValueKey<String>).value.startsWith('router.upstream.item.'))
      .evaluate()
      .length;

  int countOf(String method) => log.countOf('$_svc/$method');
  bool fired(String method) => log.fired('$_svc/$method');
  RecordedCall? lastCall(String method) => log.last('$_svc/$method');

  // ── page load ─────────────────────────────────────────────────────────────
  /// Pump the page and hold it in the initial **loading** state (one frame).
  Future<void> pumpLoading() => pumpPage(RouterPage(clients: clients));

  /// Pump the page and settle to its terminal upstreams state (loaded or error).
  Future<void> load() async {
    await pumpPage(RouterPage(clients: clients));
    await pumpUntil(() => isLoaded || isError, reason: 'router to settle');
  }

  Future<void> tapRetry() => tap('router.error.retry', until: () => isLoaded);

  // ── view switching (segmented button) ─────────────────────────────────────
  Future<void> showUpstreams() =>
      tap('router.view.upstreams', until: () => isLoaded || isError);

  /// Switch to Health — mounting `_HealthView`, whose initState fires Health.
  /// Waits for the loaded state (the Refresh button only builds once the RPC's
  /// response has landed — `fired` alone is true the instant the server records,
  /// before the table renders).
  Future<void> showHealth() => tap('router.view.health',
      until: () => exists('router.health.refresh'));

  /// Switch to the Route tester — no RPC fires until Run is tapped.
  Future<void> showRoute() =>
      tap('router.view.route', until: () => exists('router.route.run'));

  Future<void> refreshHealth() {
    final before = countOf('Health');
    return tap('router.health.refresh',
        until: () => countOf('Health') > before);
  }

  // ── upstream CRUD ─────────────────────────────────────────────────────────
  Future<void> tapAdd() =>
      tap('router.upstream.add', until: () => exists('router.upstream.field.id'));

  Future<void> selectItem(String id) =>
      tap('router.upstream.item.$id',
          until: () => exists('router.upstream.field.id'));

  /// Flip an upstream's enable switch → fires Enable, then reloads (a 2nd List).
  Future<void> toggleEnable(String id) =>
      tap('router.upstream.enable.$id', until: () => fired('Enable'));

  /// Open the delete confirm dialog for [id] (no RPC yet — the confirm gates it).
  Future<void> openDelete(String id) => tap('router.upstream.delete.$id',
      until: () => exists('router.upstream.delete.confirm'));

  Future<void> confirmDelete() => tap('router.upstream.delete.confirm',
      until: () => fired('Delete'));

  Future<void> cancelDelete() => tap('router.upstream.delete.cancel',
      until: () => !exists('router.upstream.delete.confirm'));

  Future<void> editField(String key, String text) =>
      enterText('router.upstream.field.$key', text);

  Future<void> save() async {
    // Editing fields lower in the editor's ListView auto-scrolls them into view,
    // which can push the top-anchored Save button offscreen — scroll it back
    // before tapping so the hit lands.
    await tester.ensureVisible(byKey('router.upstream.save'));
    await tester.pump();
    await tap('router.upstream.save', until: () => fired('Put'));
  }

  /// Choose a `PoolTier` in the editor's tier dropdown by its visible [label].
  Future<void> chooseEditorTier(String label) =>
      _chooseDropdown('router.upstream.field.tier', label);

  // ── route tester form ─────────────────────────────────────────────────────
  Future<void> chooseTaskMode(String label) =>
      _chooseDropdown('router.route.task_mode', label);
  Future<void> chooseRole(String label) =>
      _chooseDropdown('router.route.role', label);
  Future<void> chooseRouteTier(String label) =>
      _chooseDropdown('router.route.tier', label);

  Future<void> setMinContext(String text) =>
      enterText('router.route.min_context', text);
  Future<void> setMaxCost(String text) =>
      enterText('router.route.max_cost', text);
  Future<void> setOverride(String text) => enterText('router.route.hint', text);

  Future<void> runRoute() =>
      tap('router.route.run', until: () => fired('Route'));

  // ── helpers ───────────────────────────────────────────────────────────────
  /// Open a keyed dropdown and pick the on-screen menu entry by its visible
  /// [label]. The item `Key`/value sits on the (non-hit-testable)
  /// `DropdownMenuItem`, and every item is also rendered offstage for sizing —
  /// so we select the open menu route's `Text` filtered with `.hitTestable()`.
  Future<void> _chooseDropdown(String key, String label) async {
    await tester.tap(byKey(key));
    await tester.pumpAndSettle();
    await tester.tap(find.text(label).hitTestable());
    await tester.pumpAndSettle();
  }

  bool snackContains(String text) =>
      find.textContaining(text).evaluate().isNotEmpty;

  /// Advance the fake clock past the timers a completed RPC leaves behind so
  /// none leaks past the test (`!timersPending`): the SnackBar's ~4 s
  /// auto-dismiss timer, and grpc-dart's HTTP/2 connection idle timeout (5 min).
  Future<void> settle() async {
    await tester.pump(const Duration(seconds: 5));
    await tester.pump(const Duration(minutes: 6));
    await tester.pumpAndSettle();
  }
}
