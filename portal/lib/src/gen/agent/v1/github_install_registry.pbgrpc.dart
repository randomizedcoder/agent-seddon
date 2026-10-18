// This is a generated file - do not edit.
//
// Generated from agent/v1/github_install_registry.proto.

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

import 'github_install_registry.pb.dart' as $0;

export 'github_install_registry.pb.dart';

/// The GitHub-App install control plane (mirrors ForgeRegistryService). Mutations are
/// gated by the RBAC enforcement core (C1) on `(write|delete, forge_registry)`.
@$pb.GrpcServiceName('agent.v1.GithubInstallRegistryService')
class GithubInstallRegistryServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  GithubInstallRegistryServiceClient(super.channel,
      {super.options, super.interceptors});

  /// REST mappings (docs/design/rest-openapi/): reads → GET, deletes → DELETE by id,
  /// upsert → POST with `body: "*"`. Paths live under /v1/github-installs/.
  $grpc.ResponseFuture<$0.GithubInstallList> list(
    $0.GithubInstallListRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$list, request, options: options);
  }

  $grpc.ResponseFuture<$0.GithubInstall> get(
    $0.GithubInstallRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  $grpc.ResponseFuture<$0.GithubInstall> put(
    $0.GithubInstall request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.GithubInstallDeleteReply> delete(
    $0.GithubInstallRef request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$delete, request, options: options);
  }

  /// Self-service onboarding (forge-auth F5c2), gated on `(write, forge_registry)` and a
  /// verified principal. `CompleteInstall` is the ONLY path that authorizes `ghapp:`
  /// minting for a tenant-onboarded installation; `Put` never does.
  $grpc.ResponseFuture<$0.BeginInstallReply> beginInstall(
    $0.BeginInstallRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$beginInstall, request, options: options);
  }

  $grpc.ResponseFuture<$0.GithubInstall> completeInstall(
    $0.CompleteInstallRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$completeInstall, request, options: options);
  }

  // method descriptors

  static final _$list =
      $grpc.ClientMethod<$0.GithubInstallListRequest, $0.GithubInstallList>(
          '/agent.v1.GithubInstallRegistryService/List',
          ($0.GithubInstallListRequest value) => value.writeToBuffer(),
          $0.GithubInstallList.fromBuffer);
  static final _$get =
      $grpc.ClientMethod<$0.GithubInstallRef, $0.GithubInstall>(
          '/agent.v1.GithubInstallRegistryService/Get',
          ($0.GithubInstallRef value) => value.writeToBuffer(),
          $0.GithubInstall.fromBuffer);
  static final _$put = $grpc.ClientMethod<$0.GithubInstall, $0.GithubInstall>(
      '/agent.v1.GithubInstallRegistryService/Put',
      ($0.GithubInstall value) => value.writeToBuffer(),
      $0.GithubInstall.fromBuffer);
  static final _$delete =
      $grpc.ClientMethod<$0.GithubInstallRef, $0.GithubInstallDeleteReply>(
          '/agent.v1.GithubInstallRegistryService/Delete',
          ($0.GithubInstallRef value) => value.writeToBuffer(),
          $0.GithubInstallDeleteReply.fromBuffer);
  static final _$beginInstall =
      $grpc.ClientMethod<$0.BeginInstallRequest, $0.BeginInstallReply>(
          '/agent.v1.GithubInstallRegistryService/BeginInstall',
          ($0.BeginInstallRequest value) => value.writeToBuffer(),
          $0.BeginInstallReply.fromBuffer);
  static final _$completeInstall =
      $grpc.ClientMethod<$0.CompleteInstallRequest, $0.GithubInstall>(
          '/agent.v1.GithubInstallRegistryService/CompleteInstall',
          ($0.CompleteInstallRequest value) => value.writeToBuffer(),
          $0.GithubInstall.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.GithubInstallRegistryService')
abstract class GithubInstallRegistryServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.GithubInstallRegistryService';

  GithubInstallRegistryServiceBase() {
    $addMethod(
        $grpc.ServiceMethod<$0.GithubInstallListRequest, $0.GithubInstallList>(
            'List',
            list_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.GithubInstallListRequest.fromBuffer(value),
            ($0.GithubInstallList value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GithubInstallRef, $0.GithubInstall>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GithubInstallRef.fromBuffer(value),
        ($0.GithubInstall value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.GithubInstall, $0.GithubInstall>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GithubInstall.fromBuffer(value),
        ($0.GithubInstall value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.GithubInstallRef, $0.GithubInstallDeleteReply>(
            'Delete',
            delete_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.GithubInstallRef.fromBuffer(value),
            ($0.GithubInstallDeleteReply value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.BeginInstallRequest, $0.BeginInstallReply>(
            'BeginInstall',
            beginInstall_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.BeginInstallRequest.fromBuffer(value),
            ($0.BeginInstallReply value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.CompleteInstallRequest, $0.GithubInstall>(
        'CompleteInstall',
        completeInstall_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.CompleteInstallRequest.fromBuffer(value),
        ($0.GithubInstall value) => value.writeToBuffer()));
  }

  $async.Future<$0.GithubInstallList> list_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GithubInstallListRequest> $request) async {
    return list($call, await $request);
  }

  $async.Future<$0.GithubInstallList> list(
      $grpc.ServiceCall call, $0.GithubInstallListRequest request);

  $async.Future<$0.GithubInstall> get_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GithubInstallRef> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.GithubInstall> get(
      $grpc.ServiceCall call, $0.GithubInstallRef request);

  $async.Future<$0.GithubInstall> put_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.GithubInstall> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.GithubInstall> put(
      $grpc.ServiceCall call, $0.GithubInstall request);

  $async.Future<$0.GithubInstallDeleteReply> delete_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GithubInstallRef> $request) async {
    return delete($call, await $request);
  }

  $async.Future<$0.GithubInstallDeleteReply> delete(
      $grpc.ServiceCall call, $0.GithubInstallRef request);

  $async.Future<$0.BeginInstallReply> beginInstall_Pre($grpc.ServiceCall $call,
      $async.Future<$0.BeginInstallRequest> $request) async {
    return beginInstall($call, await $request);
  }

  $async.Future<$0.BeginInstallReply> beginInstall(
      $grpc.ServiceCall call, $0.BeginInstallRequest request);

  $async.Future<$0.GithubInstall> completeInstall_Pre($grpc.ServiceCall $call,
      $async.Future<$0.CompleteInstallRequest> $request) async {
    return completeInstall($call, await $request);
  }

  $async.Future<$0.GithubInstall> completeInstall(
      $grpc.ServiceCall call, $0.CompleteInstallRequest request);
}
