import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/agent_session.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.AgentSessionService` — the *observe + drive* seam
/// the Agent view consumes (`Subscribe` is a **server stream**, `Send` returns the
/// same stream shape, `Snapshot` is one-shot). Every RPC records into the shared
/// [RecordingLog] **before** the guard (so `log.fired(...)` holds even on the
/// injected-fault rows), then applies a one-shot [error] / [responseDelay].
///
/// The two streaming RPCs are backed by test-scriptable [StreamController]s: a
/// fresh controller per call, tracked in [subscribeControllers] / [sendControllers]
/// (with `last…` accessors) so a test can push [SessionEvent]s, close, or error the
/// live transcript. [disposeControllers] closes any still-open controller at test
/// end so no server-side stream outlives the test.
class FakeAgentSessionService extends AgentSessionServiceBase {
  FakeAgentSessionService(this._log);

  final RecordingLog _log;

  /// `Snapshot`'s one-shot reply (unused by the page, but the seam requires it).
  StatusSnapshot snapshotResponse = StatusSnapshot();

  /// When set, the next served RPC fails with this instead of returning — cleared
  /// after it fires, so one injected fault affects exactly one call. For the two
  /// streaming RPCs the fault surfaces as a stream error (→ the page's `onError`,
  /// which greys the transcript into its `stream_down` panel).
  GrpcError? error;

  /// When > zero, every served RPC waits this long before responding — the
  /// `slow(delay)` script (assert the in-flight `_sending` guard, no double-submit).
  Duration responseDelay = Duration.zero;

  final List<StreamController<SessionEvent>> subscribeControllers = [];
  final List<StreamController<SessionEvent>> sendControllers = [];

  /// The controller backing the most recent `Subscribe` stream — push events to
  /// [lastSubscribe]`!.add(ev)` to drive the transcript.
  StreamController<SessionEvent>? get lastSubscribe =>
      subscribeControllers.isEmpty ? null : subscribeControllers.last;

  /// The controller backing the most recent `Send` stream.
  StreamController<SessionEvent>? get lastSend =>
      sendControllers.isEmpty ? null : sendControllers.last;

  GrpcError? _takeError() {
    final e = error;
    error = null;
    return e;
  }

  Future<void> _delay() async {
    if (responseDelay > Duration.zero) {
      await Future<void>.delayed(responseDelay);
    }
  }

  @override
  Stream<SessionEvent> subscribe(
      ServiceCall call, SubscribeRequest request) async* {
    _log.record('agent.v1.AgentSessionService/Subscribe', request);
    final e = _takeError();
    await _delay();
    if (e != null) throw e;
    final c = StreamController<SessionEvent>();
    subscribeControllers.add(c);
    yield* c.stream;
  }

  @override
  Future<StatusSnapshot> snapshot(
      ServiceCall call, SnapshotRequest request) async {
    _log.record('agent.v1.AgentSessionService/Snapshot', request);
    final e = _takeError();
    await _delay();
    if (e != null) throw e;
    return snapshotResponse;
  }

  @override
  Stream<SessionEvent> send(ServiceCall call, GoalRequest request) async* {
    _log.record('agent.v1.AgentSessionService/Send', request);
    final e = _takeError();
    await _delay();
    if (e != null) throw e;
    final c = StreamController<SessionEvent>();
    sendControllers.add(c);
    yield* c.stream;
  }

  /// Close any still-open stream controllers (call from the robot's teardown).
  Future<void> disposeControllers() async {
    for (final c in [...subscribeControllers, ...sendControllers]) {
      if (!c.isClosed) await c.close();
    }
  }
}
