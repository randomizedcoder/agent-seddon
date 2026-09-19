import 'package:agent_portal/src/gen/agent/v1/agent_session.pb.dart';
import 'package:agent_portal/src/pages/agent_view_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:grpc/grpc.dart';

import '../fake_gateway.dart';
import '../fakes/agent_session_service.dart';
import '../fakes/llm_pool_service.dart';
import '../fakes/metrics_proxy_service.dart';
import '../fakes/session_registry_service.dart';
import '../recording.dart';
import 'robot.dart';

/// Robot for the **Agent** view — the mechanically trickiest page: a server-
/// streaming `Subscribe` transcript over two periodic pollers (`LlmPool.Health`
/// @3 s, `MetricsProxy.Query` @5 s) plus a `Registry.Open` → `Session.Send` drive.
///
/// It owns a [FakeGateway] hosting the four fakes the page dials, wired to a
/// dedicated [RecordingLog] (the fakes are handed to the gateway as `extra`
/// services, so `robot.log` — not `gw.log` — is the record to assert against).
/// Everything is torn down in `runAsync` (socket close needs the real loop); the
/// teardown also **unmounts the page first** so its periodic timers + stream
/// subscription are cancelled (`!timersPending`) before the wire closes.
class AgentRobot extends Robot {
  AgentRobot._(super.tester, this.gw, this.log, this.session, this.registry,
      this.pool, this.metrics);

  final FakeGateway gw;
  final RecordingLog log;
  final FakeAgentSessionService session;
  final FakeSessionRegistryService registry;
  final FakeLlmPoolService pool;
  final FakeMetricsProxyService metrics;

  late final clients = gw.clients();

  static const _subscribe = 'agent.v1.AgentSessionService/Subscribe';
  static const _send = 'agent.v1.AgentSessionService/Send';
  static const _health = 'agent.v1.LlmPoolService/Health';
  static const _query = 'agent.v1.MetricsProxyService/Query';

  static Future<AgentRobot> create(WidgetTester tester) async {
    late FakeAgentSessionService session;
    late FakeSessionRegistryService registry;
    late FakeLlmPoolService pool;
    late FakeMetricsProxyService metrics;
    late FakeGateway gw;
    await tester.runAsync(() async {
      gw = await FakeGateway.start((log) {
        session = FakeAgentSessionService(log);
        registry = FakeSessionRegistryService(log);
        pool = FakeLlmPoolService(log);
        metrics = FakeMetricsProxyService(log);
        return [session, registry, pool, metrics];
      });
    });
    final robot =
        AgentRobot._(tester, gw, gw.log, session, registry, pool, metrics);
    addTearDown(() async {
      // Unmount the page FIRST (synchronously, no socket needed) so its
      // initState `Timer.periodic` pollers + live stream subscription are
      // cancelled in `dispose` — these live on the real event loop, so a leaked
      // one keeps the isolate alive and races test finalization. Then close the
      // server streams and the wire on the real loop (socket close needs it).
      await tester.pumpWidget(const SizedBox());
      await tester.runAsync(() async {
        await session.disposeControllers();
        await robot.clients.shutdown();
        await gw.shutdown();
      });
    });
    return robot;
  }

  // ── async-state predicates ────────────────────────────────────────────────
  /// The `stream_down` panel (grey transcript + Reconnect) is showing.
  bool get isStreamDown => exists('agent.reconnect');

  bool get hasTranscript => transcriptCount > 0;

  int get transcriptCount => find
      .byWidgetPredicate((w) =>
          w.key is ValueKey<String> &&
          (w.key as ValueKey<String>).value.startsWith('agent.transcript.'))
      .evaluate()
      .length;

  bool get sendEnabled =>
      tester.widget<FilledButton>(byKey('agent.send')).onPressed != null;

  // ── page pumps ────────────────────────────────────────────────────────────
  Future<void> pump() => pumpPage(AgentViewPage(clients: clients));

  /// Pump and settle to the connected state: the initial `Subscribe` is live and
  /// both pollers have fired once (from initState).
  Future<void> load() async {
    await pump();
    await pumpUntil(
        () =>
            log.fired(_subscribe) &&
            session.lastSubscribe != null &&
            log.fired(_health) &&
            log.fired(_query),
        reason: 'agent initial subscribe + first polls');
  }

  /// Pump and hold in the initial **loading** state — `Subscribe` is in flight
  /// but no events have arrived (transcript empty, not down).
  Future<void> pumpLoading() async {
    await pump();
    await pumpUntil(() => log.fired(_subscribe),
        reason: 'subscribe to be dialed');
  }

  /// Pump straight into the **stream-down** state: the initial `Subscribe` fails.
  Future<void> loadStreamDown() async {
    session.error = const GrpcError.unavailable('sessions gateway down');
    await pump();
    await pumpUntil(() => isStreamDown, reason: 'stream-down panel');
  }

  // ── intent actions ──────────────────────────────────────────────────────────
  Future<void> enterGoal(String goal) => enterText('agent.goal', goal);

  /// Push one [SessionEvent] onto the live subscribe stream and settle a rebuild.
  Future<void> emit(SessionEvent ev) async {
    await real(() async {
      session.lastSubscribe!.add(ev);
      await Future<void>.delayed(const Duration(milliseconds: 10));
    });
    await tester.pump();
  }

  /// Tap Send; settles once the driven `Send` RPC is recorded (which the page
  /// reaches only after `Registry.Open` succeeds first).
  Future<void> send() => tap('agent.send', until: () => log.fired(_send));

  /// Tap Reconnect from the stream-down panel; settles once a *second* Subscribe
  /// is dialed (the re-subscribe).
  Future<void> reconnect() =>
      tap('agent.reconnect', until: () => log.countOf(_subscribe) >= 2);

  // The pollers are `Timer.periodic`s created during initState — which runs inside
  // `runAsync` (see [Robot.pumpPage]), so they live on the **real** event loop,
  // not the test's fake clock. `tester.pump(Duration)` therefore does *not* fire
  // them; only real time does. These helpers let real time elapse past one period
  // and then settle the RPC the timer kicked off.
  Future<void> advancePoolPoll() async {
    final before = log.countOf(_health);
    await real(() =>
        Future<void>.delayed(const Duration(seconds: 3, milliseconds: 300)));
    await pumpUntil(() => log.countOf(_health) > before, reason: 'pool re-poll');
  }

  Future<void> advanceMetricsPoll() async {
    final before = log.countOf(_query);
    await real(() =>
        Future<void>.delayed(const Duration(seconds: 5, milliseconds: 300)));
    await pumpUntil(() => log.countOf(_query) > before,
        reason: 'metrics re-poll');
  }

  bool textShown(String text) =>
      find.textContaining(text).evaluate().isNotEmpty;
}

// ── SessionEvent builders (readable transcript fixtures) ─────────────────────
SessionEvent statusEvent({
  String mode = 'implement',
  int tokens = 0,
  int window = 0,
  int messages = 0,
  bool active = false,
}) =>
    SessionEvent(
      statusSnapshot: StatusSnapshot(
        currentMode: mode,
        contextTokens: tokens,
        contextWindow: window,
        contextMessages: messages,
        active: active,
      ),
    );

SessionEvent runStartedEvent(String goal) =>
    SessionEvent(runStarted: RunStarted(goal: goal));

SessionEvent runFinishedEvent({bool ok = true}) =>
    SessionEvent(runFinished: RunFinished(ok: ok));

SessionEvent iterationEvent(int iter) =>
    SessionEvent(iteration: IterationStart(iter: iter));

SessionEvent tokenEvent(String text) =>
    SessionEvent(token: TokenDelta(text: text));
