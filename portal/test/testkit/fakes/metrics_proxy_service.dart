import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/metrics_proxy.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.MetricsProxyService` — the Agent view polls
/// `Query` every 5 s (twice per poll: p50 + p99) to drive the gRPC-latency status
/// cell. Every RPC records into the shared [RecordingLog] **before** the guard,
/// then applies a one-shot [error] / [responseDelay].
class FakeMetricsProxyService extends MetricsProxyServiceBase {
  FakeMetricsProxyService(this._log);

  final RecordingLog _log;

  PromResult queryResponse = PromResult();
  PromResult queryRangeResponse = PromResult();

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
  Future<PromResult> query(ServiceCall call, PromQuery request) async {
    _log.record('agent.v1.MetricsProxyService/Query', request);
    await _guard();
    return queryResponse;
  }

  @override
  Future<PromResult> queryRange(
      ServiceCall call, PromRangeQuery request) async {
    _log.record('agent.v1.MetricsProxyService/QueryRange', request);
    await _guard();
    return queryRangeResponse;
  }
}
