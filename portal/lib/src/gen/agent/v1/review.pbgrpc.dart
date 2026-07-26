// This is a generated file - do not edit.
//
// Generated from agent/v1/review.proto.

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

import 'review.pb.dart' as $0;

export 'review.pb.dart';

@$pb.GrpcServiceName('agent.v1.FactCollectorService')
class FactCollectorServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  FactCollectorServiceClient(super.channel,
      {super.options, super.interceptors});

  /// Collect grounded facts for a target. Fail-soft: a partial bundle is a
  /// success; only an unresolvable target is an error.
  $grpc.ResponseFuture<$0.ReviewFacts> collect(
    $0.ReviewCollectRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$collect, request, options: options);
  }

  // method descriptors

  static final _$collect =
      $grpc.ClientMethod<$0.ReviewCollectRequest, $0.ReviewFacts>(
          '/agent.v1.FactCollectorService/Collect',
          ($0.ReviewCollectRequest value) => value.writeToBuffer(),
          $0.ReviewFacts.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.FactCollectorService')
abstract class FactCollectorServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.FactCollectorService';

  FactCollectorServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ReviewCollectRequest, $0.ReviewFacts>(
        'Collect',
        collect_Pre,
        false,
        false,
        ($core.List<$core.int> value) =>
            $0.ReviewCollectRequest.fromBuffer(value),
        ($0.ReviewFacts value) => value.writeToBuffer()));
  }

  $async.Future<$0.ReviewFacts> collect_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ReviewCollectRequest> $request) async {
    return collect($call, await $request);
  }

  $async.Future<$0.ReviewFacts> collect(
      $grpc.ServiceCall call, $0.ReviewCollectRequest request);
}
