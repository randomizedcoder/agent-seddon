// This is a generated file - do not edit.
//
// Generated from agent/v1/mode.proto.

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

import 'mode.pb.dart' as $0;

export 'mode.pb.dart';

@$pb.GrpcServiceName('agent.v1.ModeService')
class ModeServiceClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ModeServiceClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.ModeVerdict> classify(
    $0.ClassifyRequest request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$classify, request, options: options);
  }

  // method descriptors

  static final _$classify =
      $grpc.ClientMethod<$0.ClassifyRequest, $0.ModeVerdict>(
          '/agent.v1.ModeService/Classify',
          ($0.ClassifyRequest value) => value.writeToBuffer(),
          $0.ModeVerdict.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.ModeService')
abstract class ModeServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.ModeService';

  ModeServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ClassifyRequest, $0.ModeVerdict>(
        'Classify',
        classify_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ClassifyRequest.fromBuffer(value),
        ($0.ModeVerdict value) => value.writeToBuffer()));
  }

  $async.Future<$0.ModeVerdict> classify_Pre($grpc.ServiceCall $call,
      $async.Future<$0.ClassifyRequest> $request) async {
    return classify($call, await $request);
  }

  $async.Future<$0.ModeVerdict> classify(
      $grpc.ServiceCall call, $0.ClassifyRequest request);
}
