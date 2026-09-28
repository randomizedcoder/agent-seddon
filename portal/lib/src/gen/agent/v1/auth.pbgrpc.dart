// This is a generated file - do not edit.
//
// Generated from agent/v1/auth.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:async' as $async;
import 'dart:core' as $core;

import 'package:grpc/service_api.dart' as $grpc;
import 'package:protobuf/protobuf.dart' as $pb;

import 'auth.pb.dart' as $0;

export 'auth.pb.dart';

/// REST mappings (docs/design/rest-openapi/), under /v1/auth/. Token-minting and
/// mutations → POST body:* (Exchange, Begin, Refresh, Logout, PutBinding); reads → GET
/// (Issuers, Jwks, WhoAmI, the list/get RPCs, with `tenant` as a query param);
/// session/binding removals → DELETE by id (the id is a path param, other scalars ride
/// as query params). Every field in these requests is untrusted — Envoy hands it to the
/// same gRPC handler behind the same AuthLayer, so REST bypasses none of the authz.
@$pb.GrpcServiceName('agent.v1.AuthService')
class AuthServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  AuthServiceClient(super.channel, {super.options, super.interceptors});

  /// Trade an identity provider's ID token, a browser sign-in's authorization
  /// code, or a service's client certificate for an agent token.
  $grpc.ResponseFuture<$0.ExchangeResponse> exchange(
    $0.ExchangeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$exchange, request, options: options);
  }

  /// The login issuers a browser can sign in with.
  $grpc.ResponseFuture<$0.IssuersResponse> issuers(
    $0.IssuersRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$issuers, request, options: options);
  }

  /// Start a browser sign-in: where to send the user, and the `state` to expect back.
  $grpc.ResponseFuture<$0.BeginResponse> begin(
    $0.BeginRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$begin, request, options: options);
  }

  /// The public keys agent tokens are signed with, as a JWK Set.
  $grpc.ResponseFuture<$0.JwksResponse> jwks(
    $0.JwksRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$jwks, request, options: options);
  }

  /// Who the caller's agent token says they are, and what it lets them do.
  $grpc.ResponseFuture<$0.WhoAmIResponse> whoAmI(
    $0.WhoAmIRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$whoAmI, request, options: options);
  }

  /// Trade a refresh handle for a new agent token and a new handle.
  $grpc.ResponseFuture<$0.ExchangeResponse> refresh(
    $0.RefreshRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$refresh, request, options: options);
  }

  /// Revoke the caller's own session (the one its token names).
  $grpc.ResponseFuture<$0.LogoutResponse> logout(
    $0.LogoutRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$logout, request, options: options);
  }

  /// The caller's own sessions.
  $grpc.ResponseFuture<$0.ListSessionsResponse> listMySessions(
    $0.ListMySessionsRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listMySessions, request, options: options);
  }

  /// Revoke one of the caller's own sessions.
  $grpc.ResponseFuture<$0.RevokeSessionResponse> revokeMySession(
    $0.RevokeMySessionRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$revokeMySession, request, options: options);
  }

  /// Every session in a tenant; needs `read:binding` there.
  $grpc.ResponseFuture<$0.ListSessionsResponse> listSessions(
    $0.ListSessionsRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listSessions, request, options: options);
  }

  /// Revoke any session in a tenant; needs `write:binding` there.
  $grpc.ResponseFuture<$0.RevokeSessionResponse> revokeSession(
    $0.RevokeSessionRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$revokeSession, request, options: options);
  }

  /// Role bindings in a tenant; needs `read:binding` there.
  $grpc.ResponseFuture<$0.ListBindingsResponse> listBindings(
    $0.ListBindingsRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$listBindings, request, options: options);
  }

  /// One role binding; needs `read:binding` there.
  $grpc.ResponseFuture<$0.GetBindingResponse> getBinding(
    $0.GetBindingRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$getBinding, request, options: options);
  }

  /// Create or replace a role binding; needs `write:binding` there, and may grant
  /// only permissions the caller holds, never to the caller itself.
  $grpc.ResponseFuture<$0.PutBindingResponse> putBinding(
    $0.PutBindingRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$putBinding, request, options: options);
  }

  /// Remove a role binding; needs `delete:binding` there.
  $grpc.ResponseFuture<$0.DeleteBindingResponse> deleteBinding(
    $0.DeleteBindingRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$deleteBinding, request, options: options);
  }

  // method descriptors

  static final _$exchange =
      $grpc.ClientMethod<$0.ExchangeRequest, $0.ExchangeResponse>(
          '/agent.v1.AuthService/Exchange',
          ($0.ExchangeRequest value) => value.writeToBuffer(),
          $0.ExchangeResponse.fromBuffer);
  static final _$issuers =
      $grpc.ClientMethod<$0.IssuersRequest, $0.IssuersResponse>(
          '/agent.v1.AuthService/Issuers',
          ($0.IssuersRequest value) => value.writeToBuffer(),
          $0.IssuersResponse.fromBuffer);
  static final _$begin = $grpc.ClientMethod<$0.BeginRequest, $0.BeginResponse>(
      '/agent.v1.AuthService/Begin',
      ($0.BeginRequest value) => value.writeToBuffer(),
      $0.BeginResponse.fromBuffer);
  static final _$jwks = $grpc.ClientMethod<$0.JwksRequest, $0.JwksResponse>(
      '/agent.v1.AuthService/Jwks',
      ($0.JwksRequest value) => value.writeToBuffer(),
      $0.JwksResponse.fromBuffer);
  static final _$whoAmI =
      $grpc.ClientMethod<$0.WhoAmIRequest, $0.WhoAmIResponse>(
          '/agent.v1.AuthService/WhoAmI',
          ($0.WhoAmIRequest value) => value.writeToBuffer(),
          $0.WhoAmIResponse.fromBuffer);
  static final _$refresh =
      $grpc.ClientMethod<$0.RefreshRequest, $0.ExchangeResponse>(
          '/agent.v1.AuthService/Refresh',
          ($0.RefreshRequest value) => value.writeToBuffer(),
          $0.ExchangeResponse.fromBuffer);
  static final _$logout =
      $grpc.ClientMethod<$0.LogoutRequest, $0.LogoutResponse>(
          '/agent.v1.AuthService/Logout',
          ($0.LogoutRequest value) => value.writeToBuffer(),
          $0.LogoutResponse.fromBuffer);
  static final _$listMySessions =
      $grpc.ClientMethod<$0.ListMySessionsRequest, $0.ListSessionsResponse>(
          '/agent.v1.AuthService/ListMySessions',
          ($0.ListMySessionsRequest value) => value.writeToBuffer(),
          $0.ListSessionsResponse.fromBuffer);
  static final _$revokeMySession =
      $grpc.ClientMethod<$0.RevokeMySessionRequest, $0.RevokeSessionResponse>(
          '/agent.v1.AuthService/RevokeMySession',
          ($0.RevokeMySessionRequest value) => value.writeToBuffer(),
          $0.RevokeSessionResponse.fromBuffer);
  static final _$listSessions =
      $grpc.ClientMethod<$0.ListSessionsRequest, $0.ListSessionsResponse>(
          '/agent.v1.AuthService/ListSessions',
          ($0.ListSessionsRequest value) => value.writeToBuffer(),
          $0.ListSessionsResponse.fromBuffer);
  static final _$revokeSession =
      $grpc.ClientMethod<$0.RevokeSessionRequest, $0.RevokeSessionResponse>(
          '/agent.v1.AuthService/RevokeSession',
          ($0.RevokeSessionRequest value) => value.writeToBuffer(),
          $0.RevokeSessionResponse.fromBuffer);
  static final _$listBindings =
      $grpc.ClientMethod<$0.ListBindingsRequest, $0.ListBindingsResponse>(
          '/agent.v1.AuthService/ListBindings',
          ($0.ListBindingsRequest value) => value.writeToBuffer(),
          $0.ListBindingsResponse.fromBuffer);
  static final _$getBinding =
      $grpc.ClientMethod<$0.GetBindingRequest, $0.GetBindingResponse>(
          '/agent.v1.AuthService/GetBinding',
          ($0.GetBindingRequest value) => value.writeToBuffer(),
          $0.GetBindingResponse.fromBuffer);
  static final _$putBinding =
      $grpc.ClientMethod<$0.PutBindingRequest, $0.PutBindingResponse>(
          '/agent.v1.AuthService/PutBinding',
          ($0.PutBindingRequest value) => value.writeToBuffer(),
          $0.PutBindingResponse.fromBuffer);
  static final _$deleteBinding =
      $grpc.ClientMethod<$0.DeleteBindingRequest, $0.DeleteBindingResponse>(
          '/agent.v1.AuthService/DeleteBinding',
          ($0.DeleteBindingRequest value) => value.writeToBuffer(),
          $0.DeleteBindingResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.AuthService')
abstract class AuthServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.AuthService';

  AuthServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ExchangeRequest, $0.ExchangeResponse>(
        'Exchange',
        exchange_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ExchangeRequest.fromBuffer(value),
        ($0.ExchangeResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.IssuersRequest, $0.IssuersResponse>(
        'Issuers',
        issuers_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.IssuersRequest.fromBuffer(value),
        ($0.IssuersResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.BeginRequest, $0.BeginResponse>(
        'Begin',
        begin_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.BeginRequest.fromBuffer(value),
        ($0.BeginResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.JwksRequest, $0.JwksResponse>(
        'Jwks',
        jwks_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.JwksRequest.fromBuffer(value),
        ($0.JwksResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.WhoAmIRequest, $0.WhoAmIResponse>(
        'WhoAmI',
        whoAmI_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.WhoAmIRequest.fromBuffer(value),
        ($0.WhoAmIResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RefreshRequest, $0.ExchangeResponse>(
        'Refresh',
        refresh_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RefreshRequest.fromBuffer(value),
        ($0.ExchangeResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.LogoutRequest, $0.LogoutResponse>(
        'Logout',
        logout_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.LogoutRequest.fromBuffer(value),
        ($0.LogoutResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ListMySessionsRequest, $0.ListSessionsResponse>(
            'ListMySessions',
            listMySessions_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ListMySessionsRequest.fromBuffer(value),
            ($0.ListSessionsResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RevokeMySessionRequest,
            $0.RevokeSessionResponse>(
        'RevokeMySession',
        revokeMySession_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.RevokeMySessionRequest.fromBuffer(value),
        ($0.RevokeSessionResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ListSessionsRequest, $0.ListSessionsResponse>(
            'ListSessions',
            listSessions_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ListSessionsRequest.fromBuffer(value),
            ($0.ListSessionsResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.RevokeSessionRequest, $0.RevokeSessionResponse>(
            'RevokeSession',
            revokeSession_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.RevokeSessionRequest.fromBuffer(value),
            ($0.RevokeSessionResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ListBindingsRequest, $0.ListBindingsResponse>(
            'ListBindings',
            listBindings_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ListBindingsRequest.fromBuffer(value),
            ($0.ListBindingsResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GetBindingRequest, $0.GetBindingResponse>(
        'GetBinding',
        getBinding_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GetBindingRequest.fromBuffer(value),
        ($0.GetBindingResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PutBindingRequest, $0.PutBindingResponse>(
        'PutBinding',
        putBinding_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PutBindingRequest.fromBuffer(value),
        ($0.PutBindingResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.DeleteBindingRequest, $0.DeleteBindingResponse>(
            'DeleteBinding',
            deleteBinding_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.DeleteBindingRequest.fromBuffer(value),
            ($0.DeleteBindingResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.ExchangeResponse> exchange_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ExchangeRequest> $request) async {
    return exchange($call, await $request);
  }

  $async.Future<$0.ExchangeResponse> exchange(
      $grpc.ServiceCall call, $0.ExchangeRequest request);

  $async.Future<$0.IssuersResponse> issuers_Pre($grpc.ServiceCall $call,
      $async.Future<$0.IssuersRequest> $request) async {
    return issuers($call, await $request);
  }

  $async.Future<$0.IssuersResponse> issuers(
      $grpc.ServiceCall call, $0.IssuersRequest request);

  $async.Future<$0.BeginResponse> begin_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.BeginRequest> $request) async {
    return begin($call, await $request);
  }

  $async.Future<$0.BeginResponse> begin(
      $grpc.ServiceCall call, $0.BeginRequest request);

  $async.Future<$0.JwksResponse> jwks_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.JwksRequest> $request) async {
    return jwks($call, await $request);
  }

  $async.Future<$0.JwksResponse> jwks(
      $grpc.ServiceCall call, $0.JwksRequest request);

  $async.Future<$0.WhoAmIResponse> whoAmI_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.WhoAmIRequest> $request) async {
    return whoAmI($call, await $request);
  }

  $async.Future<$0.WhoAmIResponse> whoAmI(
      $grpc.ServiceCall call, $0.WhoAmIRequest request);

  $async.Future<$0.ExchangeResponse> refresh_Pre($grpc.ServiceCall $call,
      $async.Future<$0.RefreshRequest> $request) async {
    return refresh($call, await $request);
  }

  $async.Future<$0.ExchangeResponse> refresh(
      $grpc.ServiceCall call, $0.RefreshRequest request);

  $async.Future<$0.LogoutResponse> logout_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.LogoutRequest> $request) async {
    return logout($call, await $request);
  }

  $async.Future<$0.LogoutResponse> logout(
      $grpc.ServiceCall call, $0.LogoutRequest request);

  $async.Future<$0.ListSessionsResponse> listMySessions_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.ListMySessionsRequest> $request) async {
    return listMySessions($call, await $request);
  }

  $async.Future<$0.ListSessionsResponse> listMySessions(
      $grpc.ServiceCall call, $0.ListMySessionsRequest request);

  $async.Future<$0.RevokeSessionResponse> revokeMySession_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.RevokeMySessionRequest> $request) async {
    return revokeMySession($call, await $request);
  }

  $async.Future<$0.RevokeSessionResponse> revokeMySession(
      $grpc.ServiceCall call, $0.RevokeMySessionRequest request);

  $async.Future<$0.ListSessionsResponse> listSessions_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.ListSessionsRequest> $request) async {
    return listSessions($call, await $request);
  }

  $async.Future<$0.ListSessionsResponse> listSessions(
      $grpc.ServiceCall call, $0.ListSessionsRequest request);

  $async.Future<$0.RevokeSessionResponse> revokeSession_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.RevokeSessionRequest> $request) async {
    return revokeSession($call, await $request);
  }

  $async.Future<$0.RevokeSessionResponse> revokeSession(
      $grpc.ServiceCall call, $0.RevokeSessionRequest request);

  $async.Future<$0.ListBindingsResponse> listBindings_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.ListBindingsRequest> $request) async {
    return listBindings($call, await $request);
  }

  $async.Future<$0.ListBindingsResponse> listBindings(
      $grpc.ServiceCall call, $0.ListBindingsRequest request);

  $async.Future<$0.GetBindingResponse> getBinding_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GetBindingRequest> $request) async {
    return getBinding($call, await $request);
  }

  $async.Future<$0.GetBindingResponse> getBinding(
      $grpc.ServiceCall call, $0.GetBindingRequest request);

  $async.Future<$0.PutBindingResponse> putBinding_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PutBindingRequest> $request) async {
    return putBinding($call, await $request);
  }

  $async.Future<$0.PutBindingResponse> putBinding(
      $grpc.ServiceCall call, $0.PutBindingRequest request);

  $async.Future<$0.DeleteBindingResponse> deleteBinding_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.DeleteBindingRequest> $request) async {
    return deleteBinding($call, await $request);
  }

  $async.Future<$0.DeleteBindingResponse> deleteBinding(
      $grpc.ServiceCall call, $0.DeleteBindingRequest request);
}
