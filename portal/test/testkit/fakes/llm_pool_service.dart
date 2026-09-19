import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/llm_pool.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.LlmPoolService` — the Agent view polls `Health`
/// every 3 s to drive the GPU-pool status cell. Every RPC records into the shared
/// [RecordingLog] **before** the guard, then applies a one-shot [error] /
/// [responseDelay].
class FakeLlmPoolService extends LlmPoolServiceBase {
  FakeLlmPoolService(this._log);

  final RecordingLog _log;

  PoolHealthReport healthResponse = PoolHealthReport();
  PoolCompleteResponse completeResponse = PoolCompleteResponse();

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
  Future<PoolHealthReport> health(
      ServiceCall call, PoolHealthRequest request) async {
    _log.record('agent.v1.LlmPoolService/Health', request);
    await _guard();
    return healthResponse;
  }

  @override
  Future<PoolCompleteResponse> complete(
      ServiceCall call, PoolCompleteRequest request) async {
    _log.record('agent.v1.LlmPoolService/Complete', request);
    await _guard();
    return completeResponse;
  }
}
