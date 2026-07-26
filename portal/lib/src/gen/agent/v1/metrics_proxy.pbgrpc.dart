// This is a generated file - do not edit.
//
// Generated from agent/v1/metrics_proxy.proto.

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

import 'metrics_proxy.pb.dart' as $0;

export 'metrics_proxy.pb.dart';

@$pb.GrpcServiceName('agent.v1.MetricsProxyService')
class MetricsProxyServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  MetricsProxyServiceClient(super.channel, {super.options, super.interceptors});

  /// Instant query at `time_unix_ms` (or now).
  $grpc.ResponseFuture<$0.PromResult> query(
    $0.PromQuery request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$query, request, options: options);
  }

  /// Range query over [start, end] at `step_secs` resolution.
  $grpc.ResponseFuture<$0.PromResult> queryRange(
    $0.PromRangeQuery request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$queryRange, request, options: options);
  }

  // method descriptors

  static final _$query = $grpc.ClientMethod<$0.PromQuery, $0.PromResult>(
      '/agent.v1.MetricsProxyService/Query',
      ($0.PromQuery value) => value.writeToBuffer(),
      $0.PromResult.fromBuffer);
  static final _$queryRange =
      $grpc.ClientMethod<$0.PromRangeQuery, $0.PromResult>(
          '/agent.v1.MetricsProxyService/QueryRange',
          ($0.PromRangeQuery value) => value.writeToBuffer(),
          $0.PromResult.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.MetricsProxyService')
abstract class MetricsProxyServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.MetricsProxyService';

  MetricsProxyServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.PromQuery, $0.PromResult>(
        'Query',
        query_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PromQuery.fromBuffer(value),
        ($0.PromResult value) => value.writeToBuffer()));
    $addMethod($grpc.ServiceMethod<$0.PromRangeQuery, $0.PromResult>(
        'QueryRange',
        queryRange_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.PromRangeQuery.fromBuffer(value),
        ($0.PromResult value) => value.writeToBuffer()));
  }

  $async.Future<$0.PromResult> query_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.PromQuery> $request) async {
    return query($call, await $request);
  }

  $async.Future<$0.PromResult> query(
      $grpc.ServiceCall call, $0.PromQuery request);

  $async.Future<$0.PromResult> queryRange_Pre($grpc.ServiceCall $call,
      $async.Future<$0.PromRangeQuery> $request) async {
    return queryRange($call, await $request);
  }

  $async.Future<$0.PromResult> queryRange(
      $grpc.ServiceCall call, $0.PromRangeQuery request);
}
