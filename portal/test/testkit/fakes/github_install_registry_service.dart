import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/github_install_registry.pbgrpc.dart';
import 'package:fixnum/fixnum.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.GithubInstallRegistryService` (the Forges tab's
/// installs and the BeginInstall/CompleteInstall onboarding round trip). Every
/// RPC records into the shared [RecordingLog] before it may throw, then returns
/// its scripted response; set a `*Error` field to make that RPC fail.
class FakeGithubInstallRegistryService extends GithubInstallRegistryServiceBase {
  FakeGithubInstallRegistryService(this._log);

  final RecordingLog _log;

  static const _svc = 'agent.v1.GithubInstallRegistryService';

  GithubInstallList listResponse = GithubInstallList();
  GithubInstallDeleteReply deleteResponse = GithubInstallDeleteReply(deleted: true);
  BeginInstallReply beginResponse = BeginInstallReply(
    installUrl: 'https://github.com/apps/seddon/installations/new?state=st-1',
    state: 'st-1',
    expiresAt: Int64(2000000),
  );

  /// `null` ⇒ echo the request's installation id as a fresh install.
  GithubInstall? completeResponse;

  GrpcError? listError;
  GrpcError? getError;
  GrpcError? putError;
  GrpcError? deleteError;
  GrpcError? beginError;
  GrpcError? completeError;

  Future<T> _answer<T>(String method, dynamic request, T reply, GrpcError? error) async {
    _log.record('$_svc/$method', request);
    if (error != null) throw error;
    return reply;
  }

  @override
  Future<GithubInstallList> list(ServiceCall call, GithubInstallListRequest request) =>
      _answer('List', request, listResponse, listError);

  @override
  Future<GithubInstall> get(ServiceCall call, GithubInstallRef request) => _answer(
      'Get',
      request,
      listResponse.installs.firstWhere((i) => i.installationId == request.installationId,
          orElse: () => GithubInstall(installationId: request.installationId)),
      getError);

  @override
  Future<GithubInstall> put(ServiceCall call, GithubInstall request) =>
      _answer('Put', request, request, putError);

  @override
  Future<GithubInstallDeleteReply> delete(ServiceCall call, GithubInstallRef request) =>
      _answer('Delete', request, deleteResponse, deleteError);

  @override
  Future<BeginInstallReply> beginInstall(ServiceCall call, BeginInstallRequest request) =>
      _answer('BeginInstall', request, beginResponse, beginError);

  @override
  Future<GithubInstall> completeInstall(
          ServiceCall call, CompleteInstallRequest request) =>
      _answer(
          'CompleteInstall',
          request,
          completeResponse ?? GithubInstall(installationId: request.installationId),
          completeError);
}
