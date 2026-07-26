// This is a generated file - do not edit.
//
// Generated from agent/v1/policy.proto.

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

import 'common.pb.dart' as $0;

export 'policy.pb.dart';

@$pb.GrpcServiceName('agent.v1.Policy')
class PolicyClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  PolicyClient(super.channel, {super.options, super.interceptors});

  $grpc.ResponseFuture<$0.Decision> authorize(
    $0.ToolCall request, {
    $grpc.CallOptions? options,
  }) {
    return $createUnaryCall(_$authorize, request, options: options);
  }

  // method descriptors

  static final _$authorize = $grpc.ClientMethod<$0.ToolCall, $0.Decision>(
      '/agent.v1.Policy/Authorize',
      ($0.ToolCall value) => value.writeToBuffer(),
      $0.Decision.fromBuffer);
}

@$pb.GrpcServiceName('agent.v1.Policy')
abstract class PolicyServiceBase extends $grpc.Service {
  $core.String get $name => 'agent.v1.Policy';

  PolicyServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.ToolCall, $0.Decision>(
        'Authorize',
        authorize_Pre,
        false,
        false,
        ($core.List<$core.int> value) => $0.ToolCall.fromBuffer(value),
        ($0.Decision value) => value.writeToBuffer()));
  }

  $async.Future<$0.Decision> authorize_Pre(
      $grpc.ServiceCall $call, $async.Future<$0.ToolCall> $request) async {
    return authorize($call, await $request);
  }

  $async.Future<$0.Decision> authorize(
      $grpc.ServiceCall call, $0.ToolCall request);
}
