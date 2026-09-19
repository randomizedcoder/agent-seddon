import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/session_registry.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.SessionRegistryService` — the Agent view mints a
/// session once via `Open` (attributing a driven `Send`), then reuses it. Every
/// RPC records into the shared [RecordingLog] **before** the guard, then applies a
/// one-shot [error] / [responseDelay].
class FakeSessionRegistryService extends SessionRegistryServiceBase {
  FakeSessionRegistryService(this._log);

  final RecordingLog _log;

  OpenResponse openResponse = OpenResponse(sessionId: 'portal-session');
  CloseResponse closeResponse = CloseResponse();
  HeartbeatResponse heartbeatResponse = HeartbeatResponse();

  GrpcError? error;
  Duration responseDelay = Duration.zero;

  Future<void> _guard() async {
    if (responseDelay > Duration.zero) {
      await Future<void>.delayed(responseDelay);
    }
    final e = error;
    if (e != null) {
      error = null;
      throw e;
    }
  }

  @override
  Future<OpenResponse> open(ServiceCall call, OpenRequest request) async {
    _log.record('agent.v1.SessionRegistryService/Open', request);
    await _guard();
    return openResponse;
  }

  @override
  Future<CloseResponse> close(ServiceCall call, CloseRequest request) async {
    _log.record('agent.v1.SessionRegistryService/Close', request);
    await _guard();
    return closeResponse;
  }

  @override
  Future<HeartbeatResponse> heartbeat(
      ServiceCall call, HeartbeatRequest request) async {
    _log.record('agent.v1.SessionRegistryService/Heartbeat', request);
    await _guard();
    return heartbeatResponse;
  }
}
