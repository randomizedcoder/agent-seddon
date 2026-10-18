// This is a generated file - do not edit.
//
// Generated from agent/v1/forge_registry.proto.

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

import 'forge_registry.pb.dart' as $0;

export 'forge_registry.pb.dart';

/// The forge control plane (mirrors ProviderRegistryService: one process holds the
/// forge store while any number of clients drive it). Mutations are gated by the RBAC
/// enforcement core (C1) — a caller needs a role granting `(write|delete,
/// forge_registry)`.
@$pb.GrpcServiceName('agent.v1.ForgeRegistryService')
class ForgeRegistryServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ForgeRegistryServiceClient(super.channel,
      {super.options, super.interceptors});

  /// REST mappings (docs/design/rest-openapi/): reads → GET, deletes → DELETE by id,
  /// upsert → POST with `body: "*"`. Paths live under /v1/forges/.
  $grpc.ResponseFuture<$0.ForgeList> list(
    $0.ForgeListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgeCard> get(
    $0.ForgeRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgeCard> put(
    $0.ForgeCard request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.ForgeDeleteReply> delete(
    $0.ForgeRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$delete, request, options: options);
  }

  // method descriptors

  static final _$list = $grpc.ClientMethod<$0.ForgeListRequest, $0.ForgeList>(
      '/agent.v1.ForgeRegistryService/List',
      ($0.ForgeListRequest value) => value.writeToBuffer(),
      $0.ForgeList.fromBuffer);
  static final _$get = $grpc.ClientMethod<$0.ForgeRef, $0.ForgeCard>(
      '/agent.v1.ForgeRegistryService/Get',
      ($0.ForgeRef value) => value.writeToBuffer(),
      $0.ForgeCard.fromBuffer);
  static final _$put = $grpc.ClientMethod<$0.ForgeCard, $0.ForgeCard>(
      '/agent.v1.ForgeRegistryService/Put',
      ($0.ForgeCard value) => value.writeToBuffer(),
      $0.ForgeCard.fromBuffer);
  static final _$delete = $grpc.ClientMethod<$0.ForgeRef, $0.ForgeDeleteReply>(
      '/agent.v1.ForgeRegistryService/Delete',
      ($0.ForgeRef value) => value.writeToBuffer(),
      $0.ForgeDeleteReply.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ForgeRegistryService')
abstract class ForgeRegistryServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ForgeRegistryService';

  ForgeRegistryServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ForgeListRequest, $0.ForgeList>(
        'List',
        list_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgeListRequest.fromBuffer(value),
        ($0.ForgeList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgeRef, $0.ForgeCard>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgeRef.fromBuffer(value),
        ($0.ForgeCard value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgeCard, $0.ForgeCard>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgeCard.fromBuffer(value),
        ($0.ForgeCard value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ForgeRef, $0.ForgeDeleteReply>(
        'Delete',
        delete_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ForgeRef.fromBuffer(value),
        ($0.ForgeDeleteReply value) => value.writeToBuffer()));
  }

  $async.Future<$0.ForgeList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ForgeListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.ForgeList> list(
      $grpc.ServiceCall call, $0.ForgeListRequest request);

  $async.Future<$0.ForgeCard> get_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ForgeRef> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.ForgeCard> get($grpc.ServiceCall call, $0.ForgeRef request);

  $async.Future<$0.ForgeCard> put_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ForgeCard> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.ForgeCard> put($grpc.ServiceCall call, $0.ForgeCard request);

  $async.Future<$0.ForgeDeleteReply> delete_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ForgeRef> $request) async {
    return delete($call, await $request);
  }

  $async.Future<$0.ForgeDeleteReply> delete(
      $grpc.ServiceCall call, $0.ForgeRef request);
}
