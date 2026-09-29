// This is a generated file - do not edit.
//
// Generated from agent/v1/role.proto.

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

import 'role.pb.dart' as $0;

export 'role.pb.dart';

/// The role control plane (mirrors ProviderRegistryService: one process holds the
/// role store while any number of clients drive it). Mutations are gated by the RBAC
/// enforcement core (C1) — a caller needs a role granting `(write|delete, role)`.
@$pb.GrpcServiceName('agent.v1.RoleService')
class RoleServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  RoleServiceClient(super.channel, {super.options, super.interceptors});

  /// REST mappings (docs/design/rest-openapi/): reads → GET, deletes → DELETE by id,
  /// upsert → POST with `body: "*"`. Paths live under /v1/roles/.
  $grpc.ResponseFuture<$0.RoleList> list(
    $0.RoleListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.RoleCard> get(
    $0.RoleRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  $grpc.ResponseFuture<$0.RoleCard> put(
    $0.RoleCard request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.RoleDeleteReply> delete(
    $0.RoleRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$delete, request, options: options);
  }

  // method descriptors

  static final _$list = $grpc.ClientMethod<$0.RoleListRequest, $0.RoleList>(
      '/agent.v1.RoleService/List',
      ($0.RoleListRequest value) => value.writeToBuffer(),
      $0.RoleList.fromBuffer);
  static final _$get = $grpc.ClientMethod<$0.RoleRef, $0.RoleCard>(
      '/agent.v1.RoleService/Get',
      ($0.RoleRef value) => value.writeToBuffer(),
      $0.RoleCard.fromBuffer);
  static final _$put = $grpc.ClientMethod<$0.RoleCard, $0.RoleCard>(
      '/agent.v1.RoleService/Put',
      ($0.RoleCard value) => value.writeToBuffer(),
      $0.RoleCard.fromBuffer);
  static final _$delete = $grpc.ClientMethod<$0.RoleRef, $0.RoleDeleteReply>(
      '/agent.v1.RoleService/Delete',
      ($0.RoleRef value) => value.writeToBuffer(),
      $0.RoleDeleteReply.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.RoleService')
abstract class RoleServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.RoleService';

  RoleServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.RoleListRequest, $0.RoleList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RoleListRequest.fromBuffer(value),
        ($0.RoleList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RoleRef, $0.RoleCard>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RoleRef.fromBuffer(value),
        ($0.RoleCard value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RoleCard, $0.RoleCard>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RoleCard.fromBuffer(value),
        ($0.RoleCard value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.RoleRef, $0.RoleDeleteReply>(
        'Delete',
        delete_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.RoleRef.fromBuffer(value),
        ($0.RoleDeleteReply value) => value.writeToBuffer()));
  }

  $async.Future<$0.RoleList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.RoleListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.RoleList> list(
      $grpc.ServiceCall call, $0.RoleListRequest request);

  $async.Future<$0.RoleCard> get_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.RoleRef> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.RoleCard> get($grpc.ServiceCall call, $0.RoleRef request);

  $async.Future<$0.RoleCard> put_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.RoleCard> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.RoleCard> put($grpc.ServiceCall call, $0.RoleCard request);

  $async.Future<$0.RoleDeleteReply> delete_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.RoleRef> $request) async {
    return delete($call, await $request);
  }

  $async.Future<$0.RoleDeleteReply> delete(
      $grpc.ServiceCall call, $0.RoleRef request);
}
