import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/forge_registry.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.ForgeRegistryService` (the Forges tab's forge
/// cards). Every RPC records into the shared [RecordingLog] before it may
/// throw, then returns its scripted response; set a `*Error` field to make that
/// RPC fail.
class FakeForgeRegistryService extends ForgeRegistryServiceBase {
  FakeForgeRegistryService(this._log);

  final RecordingLog _log;

  static const _svc = 'agent.v1.ForgeRegistryService';

  ForgeList listResponse = ForgeList();
  ForgeDeleteReply deleteResponse = ForgeDeleteReply(deleted: true);

  GrpcError? listError;
  GrpcError? getError;
  GrpcError? putError;
  GrpcError? deleteError;

  Future<T> _answer<T>(String method, dynamic request, T reply, GrpcError? error) async {
    _log.record('$_svc/$method', request);
    if (error != null) throw error;
    return reply;
  }

  @override
  Future<ForgeList> list(ServiceCall call, ForgeListRequest request) =>
      _answer('List', request, listResponse, listError);

  @override
  Future<ForgeCard> get(ServiceCall call, ForgeRef request) => _answer(
      'Get',
      request,
      listResponse.forges.firstWhere((c) => c.id == request.id,
          orElse: () => ForgeCard(id: request.id)),
      getError);

  /// Echoes the card, as the server does once it validated and stored it.
  @override
  Future<ForgeCard> put(ServiceCall call, ForgeCard request) =>
      _answer('Put', request, request, putError);

  @override
  Future<ForgeDeleteReply> delete(ServiceCall call, ForgeRef request) =>
      _answer('Delete', request, deleteResponse, deleteError);
}
