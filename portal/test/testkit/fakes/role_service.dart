import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/role.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.RoleService` (the Access tab's role catalog).
/// Every RPC records into the shared [RecordingLog] before it may throw, then
/// returns its scripted response; set a `*Error` field to make that RPC fail.
class FakeRoleService extends RoleServiceBase {
  FakeRoleService(this._log);

  final RecordingLog _log;

  static const _svc = 'agent.v1.RoleService';

  RoleList listResponse = RoleList();
  RoleDeleteReply deleteResponse = RoleDeleteReply(deleted: true);

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
  Future<RoleList> list(ServiceCall call, RoleListRequest request) =>
      _answer('List', request, listResponse, listError);

  @override
  Future<RoleCard> get(ServiceCall call, RoleRef request) => _answer(
      'Get',
      request,
      listResponse.roles.firstWhere((r) => r.id == request.id,
          orElse: () => RoleCard(id: request.id)),
      getError);

  /// Echoes the card, as the server does once it validated and stored it.
  @override
  Future<RoleCard> put(ServiceCall call, RoleCard request) =>
      _answer('Put', request, request, putError);

  @override
  Future<RoleDeleteReply> delete(ServiceCall call, RoleRef request) =>
      _answer('Delete', request, deleteResponse, deleteError);
}
