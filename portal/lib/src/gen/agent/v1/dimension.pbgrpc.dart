// This is a generated file - do not edit.
//
// Generated from agent/v1/dimension.proto.

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

import 'dimension.pb.dart' as $0;

export 'dimension.pb.dart';

@$pb.GrpcServiceName('agent.v1.DimensionService')
class DimensionServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  DimensionServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.DimensionStep> summarize(
    $0.SummarizeRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$summarize, request, options: options);
  }

  $grpc.ResponseFuture<$0.DimensionRecallResponse> recall(
    $0.DimensionRecallRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$recall, request, options: options);
  }

  // method descriptors

  static final _$summarize =
      $grpc.ClientMethod<$0.SummarizeRequest, $0.DimensionStep>(
          '/agent.v1.DimensionService/Summarize',
          ($0.SummarizeRequest value) => value.writeToBuffer(),
          $0.DimensionStep.fromBuffer);
  static final _$recall =
      $grpc.ClientMethod<$0.DimensionRecallRequest, $0.DimensionRecallResponse>(
          '/agent.v1.DimensionService/Recall',
          ($0.DimensionRecallRequest value) => value.writeToBuffer(),
          $0.DimensionRecallResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.DimensionService')
abstract class DimensionServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.DimensionService';

  DimensionServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.SummarizeRequest, $0.DimensionStep>(
        'Summarize',
        summarize_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.SummarizeRequest.fromBuffer(value),
        ($0.DimensionStep value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.DimensionRecallRequest,
            $0.DimensionRecallResponse>(
        'Recall',
        recall_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.DimensionRecallRequest.fromBuffer(value),
        ($0.DimensionRecallResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.DimensionStep> summarize_Pre($grpc.ServiceCall $call,
      $async.Future<$0.SummarizeRequest> $request) async {
    return summarize($call, await $request);
  }

  $async.Future<$0.DimensionStep> summarize(
      $grpc.ServiceCall call, $0.SummarizeRequest request);

  $async.Future<$0.DimensionRecallResponse> recall_Pre($grpc.ServiceCall $call,
      $async.Future<$0.DimensionRecallRequest> $request) async {
    return recall($call, await $request);
  }

  $async.Future<$0.DimensionRecallResponse> recall(
      $grpc.ServiceCall call, $0.DimensionRecallRequest request);
}
