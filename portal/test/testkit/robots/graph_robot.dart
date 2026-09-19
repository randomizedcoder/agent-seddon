import 'dart:io';

import 'package:agent_portal/src/clients.dart';
import 'package:agent_portal/src/config.dart';
import 'package:agent_portal/src/gen/agent/v1/graph.pb.dart';
import 'package:agent_portal/src/graph_library.dart';
import 'package:agent_portal/src/io/graph_platform.dart' as platform;
import 'package:agent_portal/src/pages/graph_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../fakes/graph_service.dart';
import '../recording.dart';
import 'robot.dart';

/// Robot for the Graph page. It stands up a **real** gRPC [Server] on an ephemeral
/// loopback port hosting a [FakeGraphService] (the only seam the Graph tab dials),
/// and the `PortalClients` the page uses; both are torn down — in `runAsync`,
/// since socket close needs the real event loop — automatically.
///
/// It builds the server directly rather than via `FakeGateway`, because that
/// shared helper hard-wires the Prompts fake and gives no hook to bind a new
/// seam's fake to its [log]; the wire path is identical (same `channel_io`
/// loopback the native app uses), so the realism claim is unchanged.
///
/// Many Graph actions are **local**: the graph library persists to
/// `io/graph_platform`, which on the Dart VM is the in-memory stub, and file
/// import/export are inert (`pickJsonFile` → null, `downloadJson` → no-op). This
/// robot [seed]s that store *before* pumping (so `initState` reads a known library
/// instead of the flaky asset-seed path), asserts local mutations via the widget
/// tree, and resets the store in [create] so tests don't leak.
class GraphRobot extends Robot {
  GraphRobot._(super.tester, this.log, this.graph, this.clients);

  /// The ordered record of every RPC served — assert against this.
  final RecordingLog log;

  /// The `GraphService` fake (scriptable responses + fault/slow injection).
  final FakeGraphService graph;

  /// The clients the page dials (all channels point at the fake server).
  final PortalClients clients;

  /// The private storage key `graph_library.dart` uses.
  static const storageKey = 'agent_portal.graph_library.v1';

  static Future<GraphRobot> create(WidgetTester tester) async {
    // Reset the in-memory stub store so a prior test's library never leaks in.
    platform.saveRaw(storageKey, '');
    final log = RecordingLog();
    final graph = FakeGraphService(log);
    late Server server;
    await tester.runAsync(() async {
      server = Server.create(services: [graph]);
      await server.serve(address: InternetAddress.loopbackIPv4, port: 0);
    });
    final port = server.port!;
    final clients = PortalClients(PortalConfig(
      gatewayHost: '127.0.0.1',
      gatewayPort: port,
      sessionsHost: '127.0.0.1',
      sessionsPort: port,
      fleetHost: '127.0.0.1',
      fleetPort: port,
    ));
    final robot = GraphRobot._(tester, log, graph, clients);
    addTearDown(() => tester.runAsync(() async {
          await clients.shutdown();
          await server.shutdown();
        }));
    return robot;
  }

  // ── fixtures ────────────────────────────────────────────────────────────
  /// Persist [entries] to the (stub) library store so the page loads them on
  /// init. Call BEFORE [load]. Skips the asset-seed path (empty library only).
  void seed(List<GraphLibraryEntry> entries) => GraphLibrary(entries).save();

  /// A small, fully-populated graph: two nodes (a `critic_gate` with the default
  /// simple schema → form editor; a schema-less `generate` → raw editor) and one
  /// edge, so node/edge tiles, remove buttons, and both param editors render.
  static GraphLibraryEntry sampleEntry([String name = 'alpha']) =>
      GraphLibraryEntry(
        name,
        CognitionGraph(
          version: 1,
          nodes: [
            MapEntry('cfg', GraphNode(type: 'critic_gate', typeVersion: 1)),
            MapEntry('gen', GraphNode(type: 'generate', typeVersion: 1)),
          ],
          edges: [
            GraphEdge(from: 'gen', to: 'cfg', kind: GraphEdge_Kind.KIND_MAIN),
          ],
        ),
      );

  // ── async-state predicates ────────────────────────────────────────────────
  /// The library pane's New button is present whenever the page has finished
  /// loading (both the editor and the empty-state render it alongside).
  bool get isLoaded => exists('graph.new');
  bool get showsSpinner =>
      find.byType(CircularProgressIndicator).evaluate().isNotEmpty;
  bool get hasServerBanner => exists('graph.retry');

  int get libraryItemCount => find
      .byWidgetPredicate((w) =>
          w.key is ValueKey<String> &&
          (w.key as ValueKey<String>).value.startsWith('graph.library.item.'))
      .evaluate()
      .length;

  bool snackContains(String text) =>
      find.textContaining(text).evaluate().isNotEmpty;

  // ── page pumping ──────────────────────────────────────────────────────────
  /// Pump and hold the initial loading frame (RPCs still in flight).
  Future<void> pumpLoading() => pumpPage(GraphPage(clients: clients));

  /// Pump the page and settle to its loaded state.
  Future<void> load() async {
    await pumpPage(GraphPage(clients: clients));
    await pumpUntil(() => isLoaded, reason: 'graph page to load');
  }

  /// Drain the timers a settled page leaves behind (SnackBar auto-dismiss ~4 s,
  /// grpc-dart's HTTP/2 idle close ~5 min) so none outlive the test.
  Future<void> settle() async {
    await tester.pump(const Duration(seconds: 5));
    await tester.pump(const Duration(minutes: 6));
    await tester.pumpAndSettle();
  }

  // ── local intent actions (no RPC) ─────────────────────────────────────────
  Future<void> selectItem(String name) async {
    await tester.tap(byKey('graph.library.item.$name'));
    await tester.pumpAndSettle();
  }

  /// Tap a local control (dialog opener, list mutation) and settle its
  /// transition/animation. No gRPC is triggered by these.
  Future<void> tapLocal(String key) async {
    await tester.tap(byKey(key));
    await tester.pumpAndSettle();
  }

  /// Open a dialog-bearing local control and settle its transition.
  Future<void> openDialog(String key) => tapLocal(key);

  /// Expand a node's ExpansionTile (its children — the param editor — are not in
  /// the tree until expanded) by tapping its id header.
  Future<void> expandNode(String id) async {
    await tester.tap(find.text(id).first);
    await tester.pumpAndSettle();
  }

  // ── server intent actions (RPC) ───────────────────────────────────────────
  Future<void> validate() async {
    await tester.runAsync(() async => tester.tap(byKey('graph.validate')));
    await tester.pump();
    await pumpUntil(() => log.fired('agent.v1.GraphService/Validate'),
        reason: 'Validate to fire');
  }

  Future<void> setActive() async {
    await tester.runAsync(() async => tester.tap(byKey('graph.setActive')));
    await tester.pump();
    await pumpUntil(() => log.fired('agent.v1.GraphService/Put'),
        reason: 'Put to fire');
  }

  Future<void> retry() async {
    await tester.runAsync(() async => tester.tap(byKey('graph.retry')));
    await tester.pump();
    await pumpUntil(
        () => log.countOf('agent.v1.GraphService/DescribeNodeTypes') >= 2,
        reason: 'Retry re-probes node types');
    await tester.pumpAndSettle();
  }
}
