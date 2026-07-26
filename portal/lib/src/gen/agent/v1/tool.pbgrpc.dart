// This is a generated file - do not edit.
//
// Generated from agent/v1/tool.proto.

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

import 'common.pb.dart' as $1;
import 'tool.pb.dart' as $0;

export 'tool.pb.dart';

@$pb.GrpcServiceName('agent.v1.ToolService')
class ToolServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ToolServiceClient(super.channel, {super.options, super.interceptors});

  /// The tools this worker advertises (what the model is told about).
  $grpc.ResponseFuture<$0.DescribeAllResponse> describeAll(
    $0.DescribeAllRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$describeAll, request, options: options);
  }

  /// Run one named tool.
  $grpc.ResponseFuture<$1.Observation> execute(
    $0.ExecuteRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$execute, request, options: options);
  }

  // method descriptors

  static final _$describeAll =
      $grpc.ClientMethod<$0.DescribeAllRequest, $0.DescribeAllResponse>(
          '/agent.v1.ToolService/DescribeAll',
          ($0.DescribeAllRequest value) => value.writeToBuffer(),
          $0.DescribeAllResponse.fromBuffer);
  static final _$execute =
      $grpc.ClientMethod<$0.ExecuteRequest, $1.Observation>(
          '/agent.v1.ToolService/Execute',
          ($0.ExecuteRequest value) => value.writeToBuffer(),
          $1.Observation.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ToolService')
abstract class ToolServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ToolService';

  ToolServiceBase() {
    $addMethod(
        $grpc.ServiceMethod<$0.DescribeAllRequest, $0.DescribeAllResponse>(
            'DescribeAll',
            describeAll_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.DescribeAllRequest.fromBuffer(value),
            ($0.DescribeAllResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.ExecuteRequest, $1.Observation>(
        'Execute',
        execute_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ExecuteRequest.fromBuffer(value),
        ($1.Observation value) => value.writeToBuffer()));
  }

  $async.Future<$0.DescribeAllResponse> describeAll_Pre($grpc.ServiceCall $call,
      $async.Future<$0.DescribeAllRequest> $request) async {
    return describeAll($call, await $request);
  }

  $async.Future<$0.DescribeAllResponse> describeAll(
      $grpc.ServiceCall call, $0.DescribeAllRequest request);

  $async.Future<$1.Observation> execute_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ExecuteRequest> $request) async {
    return execute($call, await $request);
  }

  $async.Future<$1.Observation> execute(
      $grpc.ServiceCall call, $0.ExecuteRequest request);
}
