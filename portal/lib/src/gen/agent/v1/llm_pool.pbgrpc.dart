// This is a generated file - do not edit.
//
// Generated from agent/v1/llm_pool.proto.

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

import 'llm_pool.pb.dart' as $0;

export 'llm_pool.pb.dart';

@$pb.GrpcServiceName('agent.v1.LlmPoolService')
class LlmPoolServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  LlmPoolServiceClient(super.channel, {super.options, super.interceptors});

  /// Current liveness of every pool member (from the active probe).
  $grpc.ResponseFuture<$0.PoolHealthReport> health(
    $0.PoolHealthRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$health, request, options: options);
  }

  /// Fan out one request to up to `fanout` healthy members at/above `tier`.
  $grpc.ResponseFuture<$0.PoolCompleteResponse> complete(
    $0.PoolCompleteRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$complete, request, options: options);
  }

  // method descriptors

  static final _$health =
      $grpc.ClientMethod<$0.PoolHealthRequest, $0.PoolHealthReport>(
          '/agent.v1.LlmPoolService/Health',
          ($0.PoolHealthRequest value) => value.writeToBuffer(),
          $0.PoolHealthReport.fromBuffer);
  static final _$complete =
      $grpc.ClientMethod<$0.PoolCompleteRequest, $0.PoolCompleteResponse>(
          '/agent.v1.LlmPoolService/Complete',
          ($0.PoolCompleteRequest value) => value.writeToBuffer(),
          $0.PoolCompleteResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.LlmPoolService')
abstract class LlmPoolServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.LlmPoolService';

  LlmPoolServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.PoolHealthRequest, $0.PoolHealthReport>(
        'Health',
        health_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PoolHealthRequest.fromBuffer(value),
        ($0.PoolHealthReport value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.PoolCompleteRequest, $0.PoolCompleteResponse>(
            'Complete',
            complete_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.PoolCompleteRequest.fromBuffer(value),
            ($0.PoolCompleteResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.PoolHealthReport> health_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PoolHealthRequest> $request) async {
    return health($call, await $request);
  }

  $async.Future<$0.PoolHealthReport> health(
      $grpc.ServiceCall call, $0.PoolHealthRequest request);

  $async.Future<$0.PoolCompleteResponse> complete_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PoolCompleteRequest> $request) async {
    return complete($call, await $request);
  }

  $async.Future<$0.PoolCompleteResponse> complete(
      $grpc.ServiceCall call, $0.PoolCompleteRequest request);
}
