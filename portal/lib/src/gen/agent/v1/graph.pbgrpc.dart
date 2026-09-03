// This is a generated file - do not edit.
//
// Generated from agent/v1/graph.proto.

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

import 'graph.pb.dart' as $0;

export 'graph.pb.dart';

@$pb.GrpcServiceName('agent.v1.GraphService')
class GraphServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  GraphServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.GetGraphResponse> get(
    $0.GetGraphRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$get, request, options: options);
  }

  /// Validates before accepting; an invalid document is rejected wholesale
  /// (`INVALID_ARGUMENT` carrying the first issues), never partially stored.
  $grpc.ResponseFuture<$0.PutGraphResponse> put(
    $0.PutGraphRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$put, request, options: options);
  }

  $grpc.ResponseFuture<$0.ValidateGraphResponse> validate(
    $0.ValidateGraphRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$validate, request, options: options);
  }

  $grpc.ResponseFuture<$0.DescribeNodeTypesResponse> describeNodeTypes(
    $0.DescribeNodeTypesRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$describeNodeTypes, request, options: options);
  }

  // method descriptors

  static final _$get =
      $grpc.ClientMethod<$0.GetGraphRequest, $0.GetGraphResponse>(
          '/agent.v1.GraphService/Get',
          ($0.GetGraphRequest value) => value.writeToBuffer(),
          $0.GetGraphResponse.fromBuffer);
  static final _$put =
      $grpc.ClientMethod<$0.PutGraphRequest, $0.PutGraphResponse>(
          '/agent.v1.GraphService/Put',
          ($0.PutGraphRequest value) => value.writeToBuffer(),
          $0.PutGraphResponse.fromBuffer);
  static final _$validate =
      $grpc.ClientMethod<$0.ValidateGraphRequest, $0.ValidateGraphResponse>(
          '/agent.v1.GraphService/Validate',
          ($0.ValidateGraphRequest value) => value.writeToBuffer(),
          $0.ValidateGraphResponse.fromBuffer);
  static final _$describeNodeTypes = $grpc.ClientMethod<
          $0.DescribeNodeTypesRequest, $0.DescribeNodeTypesResponse>(
      '/agent.v1.GraphService/DescribeNodeTypes',
      ($0.DescribeNodeTypesRequest value) => value.writeToBuffer(),
      $0.DescribeNodeTypesResponse.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.GraphService')
abstract class GraphServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.GraphService';

  GraphServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.GetGraphRequest, $0.GetGraphResponse>(
        'Get',
        get_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.GetGraphRequest.fromBuffer(value),
        ($0.GetGraphResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PutGraphRequest, $0.PutGraphResponse>(
        'Put',
        put_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PutGraphRequest.fromBuffer(value),
        ($0.PutGraphResponse value) => value.writeToBuffer()));
    $addMethod(
        $grpc.ServiceMethod<$0.ValidateGraphRequest, $0.ValidateGraphResponse>(
            'Validate',
            validate_Pre,
            false,
            false,
            ($core.List<$core.int> value) =>
                $0.ValidateGraphRequest.fromBuffer(value),
            ($0.ValidateGraphResponse value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.DescribeNodeTypesRequest,
            $0.DescribeNodeTypesResponse>(
        'DescribeNodeTypes',
        describeNodeTypes_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.DescribeNodeTypesRequest.fromBuffer(value),
        ($0.DescribeNodeTypesResponse value) => value.writeToBuffer()));
  }

  $async.Future<$0.GetGraphResponse> get_Pre($grpc.ServiceCall $call,
      $async.Future<$0.GetGraphRequest> $request) async {
    return get($call, await $request);
  }

  $async.Future<$0.GetGraphResponse> get(
      $grpc.ServiceCall call, $0.GetGraphRequest request);

  $async.Future<$0.PutGraphResponse> put_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PutGraphRequest> $request) async {
    return put($call, await $request);
  }

  $async.Future<$0.PutGraphResponse> put(
      $grpc.ServiceCall call, $0.PutGraphRequest request);

  $async.Future<$0.ValidateGraphResponse> validate_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ValidateGraphRequest> $request) async {
    return validate($call, await $request);
  }

  $async.Future<$0.ValidateGraphResponse> validate(
      $grpc.ServiceCall call, $0.ValidateGraphRequest request);

  $async.Future<$0.DescribeNodeTypesResponse> describeNodeTypes_Pre(
      $grpc.ServiceCall $call,
      $async.Future<$0.DescribeNodeTypesRequest> $request) async {
    return describeNodeTypes($call, await $request);
  }

  $async.Future<$0.DescribeNodeTypesResponse> describeNodeTypes(
      $grpc.ServiceCall call, $0.DescribeNodeTypesRequest request);
}
