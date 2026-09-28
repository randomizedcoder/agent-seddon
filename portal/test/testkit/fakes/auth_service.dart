import 'dart:async';

import 'package:agent_portal/src/gen/agent/v1/auth.pbgrpc.dart';
import 'package:grpc/grpc.dart';

import '../recording.dart';

/// In-process fake of `agent.v1.AuthService` — the RPCs browser sign-in uses
/// (`Issuers`, `Begin`, `Exchange`, `WhoAmI`, `Refresh`, `Logout`). Each records
/// into the shared [RecordingLog] and returns its scripted response; set one of
/// the `*Error` fields to make that RPC fail. The admin RPCs the portal does not
/// call answer `UNIMPLEMENTED`.
class FakeAuthService extends AuthServiceBase {
  FakeAuthService(this._log);

  final RecordingLog _log;

  IssuersResponse issuersResponse = IssuersResponse();
  BeginResponse beginResponse = BeginResponse();
  ExchangeResponse exchangeResponse = ExchangeResponse();
  WhoAmIResponse whoAmIResponse = WhoAmIResponse();
  ExchangeResponse refreshResponse = ExchangeResponse();

  GrpcError? issuersError;
  GrpcError? beginError;
  GrpcError? exchangeError;
  GrpcError? whoAmIError;
  GrpcError? refreshError;
  GrpcError? logoutError;

  static const _svc = 'agent.v1.AuthService';

  Future<T> _answer<T>(String method, dynamic request, T reply, GrpcError? error) async {
    _log.record('$_svc/$method', request);
    if (error != null) throw error;
    return reply;
  }

  @override
  Future<IssuersResponse> issuers(ServiceCall call, IssuersRequest request) =>
      _answer('Issuers', request, issuersResponse, issuersError);

  @override
  Future<BeginResponse> begin(ServiceCall call, BeginRequest request) =>
      _answer('Begin', request, beginResponse, beginError);

  @override
  Future<ExchangeResponse> exchange(ServiceCall call, ExchangeRequest request) =>
      _answer('Exchange', request, exchangeResponse, exchangeError);

  @override
  Future<WhoAmIResponse> whoAmI(ServiceCall call, WhoAmIRequest request) =>
      _answer('WhoAmI', request, whoAmIResponse, whoAmIError);

  @override
  Future<ExchangeResponse> refresh(ServiceCall call, RefreshRequest request) =>
      _answer('Refresh', request, refreshResponse, refreshError);

  @override
  Future<LogoutResponse> logout(ServiceCall call, LogoutRequest request) =>
      _answer('Logout', request, LogoutResponse(), logoutError);

  Never _unused() => throw GrpcError.unimplemented('not used by the portal');

  @override
  Future<JwksResponse> jwks(ServiceCall call, JwksRequest request) async => _unused();

  @override
  Future<ListSessionsResponse> listMySessions(
          ServiceCall call, ListMySessionsRequest request) async =>
      _unused();

  @override
  Future<RevokeSessionResponse> revokeMySession(
          ServiceCall call, RevokeMySessionRequest request) async =>
      _unused();

  @override
  Future<ListSessionsResponse> listSessions(
          ServiceCall call, ListSessionsRequest request) async =>
      _unused();

  @override
  Future<RevokeSessionResponse> revokeSession(
          ServiceCall call, RevokeSessionRequest request) async =>
      _unused();

  @override
  Future<ListBindingsResponse> listBindings(
          ServiceCall call, ListBindingsRequest request) async =>
      _unused();

  @override
  Future<GetBindingResponse> getBinding(
          ServiceCall call, GetBindingRequest request) async =>
      _unused();

  @override
  Future<PutBindingResponse> putBinding(
          ServiceCall call, PutBindingRequest request) async =>
      _unused();

  @override
  Future<DeleteBindingResponse> deleteBinding(
          ServiceCall call, DeleteBindingRequest request) async =>
      _unused();
}
