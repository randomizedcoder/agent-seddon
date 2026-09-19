import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/graph.pbgrpc.dart';
import 'package:agent_portal/src/graph_json.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.GraphService` (the seam the Graph tab consumes).
/// Every RPC records into the shared [RecordingLog] *before* it may throw, then
/// returns a scripted response; set [error] to fail the next served call (the
/// resilience matrix). Responses default to sensible values so an un-scripted
/// test still gets a well-formed reply:
///
///  * [describeResponse] carries one *simple-schema* node type by default, so the
///    palette is populated and `node.add` is enabled (the page blocks add with a
///    SnackBar when node-types are unfetched — the "gateway down" case).
///  * [getResponse] is null by default → `Get` throws `FAILED_PRECONDITION`, the
///    realistic "no active document yet" reply. That keeps the page from adding a
///    phantom `active (server)` library entry during init. Set it to surface an
///    active graph (and drive the active badge).
class FakeGraphService extends GraphServiceBase {
  FakeGraphService(this._log);

  final RecordingLog _log;

  /// A node type whose params schema is *simple* (all scalar properties) so the
  /// per-node param editor renders the schema-driven form (fields + Raw toggle).
  static NodeTypeSchema criticGateType() => NodeTypeSchema(
        type: 'critic_gate',
        typeVersion: 1,
        title: 'Critic gate',
        doc: 'Loops generate→critique until the critic passes.',
        paramsSchema: dartToJsonValue({
          'type': 'object',
          'properties': {
            'critic': {'type': 'string', 'description': 'the critic model'},
            'max_rounds': {'type': 'integer', 'description': 'round cap'},
          },
        }),
      );

  // Scripted responses — assign in a test's `arrange` step.
  DescribeNodeTypesResponse describeResponse =
      DescribeNodeTypesResponse(nodeTypes: [criticGateType()]);
  ValidateGraphResponse validateResponse = ValidateGraphResponse();
  PutGraphResponse putResponse = PutGraphResponse();

  /// When null (default), `Get` throws `FAILED_PRECONDITION` — "nothing active".
  /// Set it to make the server report an active graph.
  GetGraphResponse? getResponse;

  /// When set, the next served RPC throws this instead of returning — cleared
  /// after it fires, so one injected fault affects exactly one call.
  GrpcError? error;

  /// When > zero, every served RPC waits this long before responding — the
  /// `slow(delay)` script for the "slow / hung" resilience row.
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
  Future<GetGraphResponse> get(
      ServiceCall call, GetGraphRequest request) async {
    _log.record('agent.v1.GraphService/Get', request);
    await _guard();
    final r = getResponse;
    if (r == null) {
      throw const GrpcError.failedPrecondition('no active graph');
    }
    return r;
  }

  @override
  Future<PutGraphResponse> put(
      ServiceCall call, PutGraphRequest request) async {
    _log.record('agent.v1.GraphService/Put', request);
    await _guard();
    return putResponse;
  }

  @override
  Future<ValidateGraphResponse> validate(
      ServiceCall call, ValidateGraphRequest request) async {
    _log.record('agent.v1.GraphService/Validate', request);
    await _guard();
    return validateResponse;
  }

  @override
  Future<DescribeNodeTypesResponse> describeNodeTypes(
      ServiceCall call, DescribeNodeTypesRequest request) async {
    _log.record('agent.v1.GraphService/DescribeNodeTypes', request);
    await _guard();
    return describeResponse;
  }
}
